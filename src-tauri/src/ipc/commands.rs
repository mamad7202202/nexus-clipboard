//! Tauri commands — the complete backend surface available to the UI.
//!
//! Conventions used throughout:
//! - Every command takes `State<'_, Arc<AppState>>` and returns
//!   `Result<T, Error>`, which serializes to `{code, message}`.
//! - Blocking work (SQLite, file IO) runs inside the command directly; Tauri
//!   already dispatches commands on a worker thread, so this does not stall the
//!   UI. Only genuinely async work (`ai`) is `async fn`.
//! - Nothing here contains business logic; commands validate, delegate, and
//!   emit events.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::app::state::AppState;
use crate::app::window;
use crate::config::Settings;
use crate::domain::{Collection, Item, Page, Query, Stats, Tag};
use crate::error::{Error, Result};
use crate::features::{ai, backup, capture, maintenance, paste};
use crate::infra::db::{items, meta};
use crate::infra::{crypto, platform};
use crate::ipc::events;
use crate::util;

type St<'a> = State<'a, Arc<AppState>>;

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// Page through the history under the given filters.
#[tauri::command]
pub fn list_items(state: St<'_>, query: Query) -> Result<Page<Item>> {
    let conn = state.conn()?;
    items::list(&conn, &query)
}

/// Load one item together with its full payload.
///
/// `reveal` decrypts a sensitive item; it fails when the vault is locked, which
/// the UI turns into an unlock prompt.
#[tauri::command]
pub fn get_item(state: St<'_>, id: i64, reveal: bool) -> Result<Item> {
    let conn = state.conn()?;
    let mut item = items::get(&conn, id)?;

    if item.encrypted {
        item.body = if reveal {
            let cipher = items::cipher_of(&conn, id)?;
            Some(state.decrypt(&cipher)?)
        } else {
            None
        };
    } else if item.body.is_none() {
        if let Some(key) = &item.blob {
            if !item.kind.is_binary() {
                let bytes = state.blobs.get(key)?;
                item.body = Some(String::from_utf8_lossy(&bytes).to_string());
            }
        }
    }

    items::touch(&conn, id, "view", util::now_ms())?;
    Ok(item)
}

/// Raw bytes of an image item, as a data URI the webview can render.
#[tauri::command]
pub fn get_image(state: St<'_>, id: i64) -> Result<String> {
    use base64::Engine;
    let conn = state.conn()?;
    let item = items::get(&conn, id)?;
    let key = item.blob.as_deref().ok_or(Error::NotFound)?;
    let bytes = state.blobs.get(key)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub fn get_stats(state: St<'_>) -> Result<Stats> {
    let conn = state.conn()?;
    items::stats(&conn)
}

#[tauri::command]
pub fn set_pinned(app: AppHandle, state: St<'_>, id: i64, pinned: bool) -> Result<()> {
    let conn = state.conn()?;
    items::set_pinned(&conn, id, pinned)?;
    events::history_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn set_favorite(app: AppHandle, state: St<'_>, id: i64, favorite: bool) -> Result<()> {
    let conn = state.conn()?;
    items::set_favorite(&conn, id, favorite)?;
    events::history_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn update_item(app: AppHandle, state: St<'_>, id: i64, body: String) -> Result<Item> {
    let conn = state.conn()?;
    let existing = items::get(&conn, id)?;
    let preview = capture::preview_for(existing.kind, &body);
    items::update_body(&conn, id, &body, &preview, util::now_ms())?;
    events::history_changed(&app);
    items::get(&conn, id)
}

#[tauri::command]
pub fn delete_items(app: AppHandle, state: St<'_>, ids: Vec<i64>) -> Result<usize> {
    let conn = state.conn()?;
    let n = items::soft_delete(&conn, &ids, util::now_ms())?;
    events::history_changed(&app);
    Ok(n)
}

#[tauri::command]
pub fn restore_items(app: AppHandle, state: St<'_>, ids: Vec<i64>) -> Result<usize> {
    let conn = state.conn()?;
    let n = items::restore(&conn, &ids)?;
    events::history_changed(&app);
    Ok(n)
}

#[tauri::command]
pub fn purge_items(app: AppHandle, state: St<'_>, ids: Vec<i64>) -> Result<usize> {
    let conn = state.conn()?;
    let orphans = items::purge(&conn, &ids)?;
    for key in &orphans {
        let _ = state.blobs.remove(key);
    }
    events::history_changed(&app);
    Ok(ids.len())
}

#[tauri::command]
pub fn empty_trash(app: AppHandle, state: St<'_>) -> Result<usize> {
    let conn = state.conn()?;
    let orphans = items::empty_trash(&conn)?;
    let n = orphans.len();
    for key in &orphans {
        let _ = state.blobs.remove(key);
    }
    events::history_changed(&app);
    Ok(n)
}

/// Trash the whole history. `keep_starred` protects pinned and favourite items.
#[tauri::command]
pub fn clear_history(app: AppHandle, state: St<'_>, keep_starred: bool) -> Result<usize> {
    let conn = state.conn()?;
    let n = items::clear_history(&conn, keep_starred, util::now_ms())?;
    events::history_changed(&app);
    Ok(n)
}

// ---------------------------------------------------------------------------
// Clipboard actions
// ---------------------------------------------------------------------------

/// Copy an item back to the clipboard, and optionally paste it.
#[tauri::command]
pub fn use_item(
    app: AppHandle,
    state: St<'_>,
    id: i64,
    mode: paste::PasteMode,
) -> Result<()> {
    // Hide the launcher before restoring focus, otherwise Windows will refuse
    // the SetForegroundWindow call and the keystroke lands nowhere.
    if state.settings().hide_after_paste {
        window::hide_launcher(&app);
    }
    paste::paste_item(&state, id, mode)?;
    events::history_changed(&app);
    Ok(())
}

/// Put arbitrary text on the clipboard (transform results, AI output).
#[tauri::command]
pub fn copy_text(state: St<'_>, text: String) -> Result<()> {
    paste::copy_text(&state, &text)
}

/// Capture whatever is on the clipboard right now, on demand.
#[tauri::command]
pub fn capture_now(app: AppHandle, state: St<'_>) -> Result<String> {
    let outcome = capture::capture(&state)?;
    events::history_changed(&app);
    Ok(match outcome {
        capture::Outcome::Stored(_) => "stored".into(),
        capture::Outcome::Duplicate(_) => "duplicate".into(),
        capture::Outcome::Skipped(reason) => format!("skipped:{}", reason.as_str()),
    })
}

/// Pause or resume clipboard monitoring.
#[tauri::command]
pub fn set_capture_paused(app: AppHandle, state: St<'_>, paused: bool) -> Result<()> {
    if let Some(watcher) = state.watcher() {
        watcher.set_paused(paused);
    }
    state.update_settings(|s| s.capture_enabled = !paused)?;
    let _ = app.emit(events::CAPTURE_STATE, !paused);
    Ok(())
}

#[tauri::command]
pub fn is_capture_paused(state: St<'_>) -> bool {
    state.watcher().map(|w| w.is_paused()).unwrap_or(false) || !state.settings().capture_enabled
}

// ---------------------------------------------------------------------------
// Tags & collections
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_tags(state: St<'_>) -> Result<Vec<Tag>> {
    let conn = state.conn()?;
    meta::list_tags(&conn)
}

#[tauri::command]
pub fn create_tag(state: St<'_>, name: String, color: String) -> Result<Tag> {
    let conn = state.conn()?;
    meta::upsert_tag(&conn, &name, &color, util::now_ms())
}

#[tauri::command]
pub fn update_tag(state: St<'_>, id: i64, name: String, color: String) -> Result<()> {
    let conn = state.conn()?;
    meta::rename_tag(&conn, id, &name, &color)
}

#[tauri::command]
pub fn delete_tag(state: St<'_>, id: i64) -> Result<()> {
    let conn = state.conn()?;
    meta::delete_tag(&conn, id)
}

#[tauri::command]
pub fn tag_items(app: AppHandle, state: St<'_>, ids: Vec<i64>, tag_id: i64) -> Result<()> {
    let conn = state.conn()?;
    meta::tag_items(&conn, &ids, tag_id)?;
    events::history_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn untag_items(app: AppHandle, state: St<'_>, ids: Vec<i64>, tag_id: i64) -> Result<()> {
    let conn = state.conn()?;
    meta::untag_items(&conn, &ids, tag_id)?;
    events::history_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn list_collections(state: St<'_>) -> Result<Vec<Collection>> {
    let conn = state.conn()?;
    meta::list_collections(&conn)
}

#[tauri::command]
pub fn create_collection(
    state: St<'_>,
    name: String,
    icon: String,
    color: String,
) -> Result<Collection> {
    let conn = state.conn()?;
    meta::create_collection(&conn, &name, &icon, &color, util::now_ms())
}

#[tauri::command]
pub fn update_collection(
    state: St<'_>,
    id: i64,
    name: String,
    icon: String,
    color: String,
) -> Result<()> {
    let conn = state.conn()?;
    meta::update_collection(&conn, id, &name, &icon, &color)
}

#[tauri::command]
pub fn delete_collection(state: St<'_>, id: i64) -> Result<()> {
    let conn = state.conn()?;
    meta::delete_collection(&conn, id)
}

#[tauri::command]
pub fn add_to_collection(app: AppHandle, state: St<'_>, ids: Vec<i64>, collection_id: i64) -> Result<()> {
    let conn = state.conn()?;
    meta::add_to_collection(&conn, &ids, collection_id)?;
    events::history_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn remove_from_collection(
    app: AppHandle,
    state: St<'_>,
    ids: Vec<i64>,
    collection_id: i64,
) -> Result<()> {
    let conn = state.conn()?;
    meta::remove_from_collection(&conn, &ids, collection_id)?;
    events::history_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn reorder_collection(state: St<'_>, collection_id: i64, ordered: Vec<i64>) -> Result<()> {
    let conn = state.conn()?;
    meta::reorder_collection(&conn, collection_id, &ordered)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: St<'_>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: St<'_>, settings: Settings) -> Result<Settings> {
    let previous = state.settings();
    state.save_settings(settings.clone())?;

    // Hotkeys and autostart are OS-level registrations; re-apply them whenever
    // the relevant setting actually changed.
    if previous.hotkey_launcher != settings.hotkey_launcher
        || previous.hotkey_palette != settings.hotkey_palette
        || previous.hotkey_quick_paste != settings.hotkey_quick_paste
    {
        crate::app::hotkeys::reregister(&app, &settings)?;
    }
    if previous.start_with_system != settings.start_with_system {
        crate::app::autostart::apply(&app, settings.start_with_system)?;
    }
    if let Some(watcher) = state.watcher() {
        watcher.set_paused(!settings.capture_enabled);
    }

    let _ = app.emit(events::SETTINGS_CHANGED, &settings);
    Ok(settings)
}

#[tauri::command]
pub fn list_ignored_apps(state: St<'_>) -> Result<Vec<String>> {
    let conn = state.conn()?;
    meta::list_ignored_apps(&conn)
}

#[tauri::command]
pub fn add_ignored_app(state: St<'_>, name: String) -> Result<Vec<String>> {
    {
        let conn = state.conn()?;
        meta::add_ignored_app(&conn, &name)?;
    }
    state.reload_ignored_apps()
}

#[tauri::command]
pub fn remove_ignored_app(state: St<'_>, name: String) -> Result<Vec<String>> {
    {
        let conn = state.conn()?;
        meta::remove_ignored_app(&conn, &name)?;
    }
    state.reload_ignored_apps()
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct VaultStatus {
    pub configured: bool,
    pub unlocked: bool,
    pub requires_passphrase: bool,
}

#[tauri::command]
pub fn vault_status(state: St<'_>) -> Result<VaultStatus> {
    let conn = state.conn()?;
    let config = crate::config::load_vault(&conn)?;
    Ok(VaultStatus {
        configured: config.is_some(),
        unlocked: state.is_vault_unlocked(),
        requires_passphrase: config.map(|c| c.passphrase).unwrap_or(false),
    })
}

/// Unlock the vault so secrets can be revealed and captured.
#[tauri::command]
pub fn vault_unlock(app: AppHandle, state: St<'_>, passphrase: Option<String>) -> Result<()> {
    let conn = state.conn()?;
    let config = crate::config::load_vault(&conn)?.ok_or_else(|| Error::other("the vault is not set up"))?;
    let key = crypto::unlock(&config, passphrase.as_deref())?;
    state.set_vault_key(Some(key));
    let _ = app.emit(events::VAULT_STATE, true);
    Ok(())
}

#[tauri::command]
pub fn vault_lock(app: AppHandle, state: St<'_>) {
    state.set_vault_key(None);
    let _ = app.emit(events::VAULT_STATE, false);
}

/// Set (or replace) the vault passphrase.
///
/// Existing secrets are re-encrypted under the new key inside one transaction,
/// so a failure part-way through leaves the old key working.
#[tauri::command]
pub fn vault_set_passphrase(
    state: St<'_>,
    passphrase: Option<String>,
    current: Option<String>,
) -> Result<()> {
    let mut conn = state.conn()?;

    let old_key = match crate::config::load_vault(&conn)? {
        Some(config) => Some(crypto::unlock(&config, current.as_deref())?),
        None => None,
    };

    let (config, new_key) = crypto::init(passphrase.as_deref().filter(|p| !p.is_empty()))?;

    let tx = conn.transaction()?;
    if let Some(old_key) = old_key {
        let mut stmt = tx.prepare("SELECT id, cipher FROM items WHERE cipher IS NOT NULL")?;
        let rows: Vec<(i64, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);

        for (id, cipher) in rows {
            let plain = crypto::decrypt(&old_key, &cipher)?;
            let recrypted = crypto::encrypt(&new_key, &plain)?;
            tx.execute(
                "UPDATE items SET cipher = ?2 WHERE id = ?1",
                rusqlite::params![id, recrypted],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO settings (key, value) VALUES ('vault', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![serde_json::to_string(&config)?],
    )?;
    tx.commit()?;

    state.set_vault_key(Some(new_key));
    Ok(())
}

// ---------------------------------------------------------------------------
// Transforms & AI
// ---------------------------------------------------------------------------

/// Apply a transform to text. Local transforms never touch the network.
#[tauri::command]
pub async fn transform_text(
    state: St<'_>,
    text: String,
    transform: ai::Transform,
) -> Result<String> {
    ai::apply(&state, &text, transform).await
}

/// Summarise an item and cache the result on the row.
#[tauri::command]
pub async fn summarize_item(state: St<'_>, id: i64) -> Result<String> {
    let text = {
        let conn = state.conn()?;
        let item = items::get(&conn, id)?;
        paste::resolve_text(&state, &conn, &item)?
    };

    let summary = ai::apply(&state, &text, ai::Transform::Summarize).await?;

    let conn = state.conn()?;
    items::patch_meta(
        &conn,
        id,
        &crate::domain::Meta { summary: Some(summary.clone()), ..Default::default() },
    )?;
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Backup / export
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn export_history(state: St<'_>, path: String, options: backup::ExportOptions) -> Result<usize> {
    backup::export_json(&state, std::path::Path::new(&path), options)
}

#[tauri::command]
pub fn import_history(app: AppHandle, state: St<'_>, path: String) -> Result<backup::ImportReport> {
    let report = backup::import_json(&state, std::path::Path::new(&path))?;
    events::history_changed(&app);
    Ok(report)
}

#[tauri::command]
pub fn create_backup(state: St<'_>, path: String) -> Result<u64> {
    backup::backup(&state, std::path::Path::new(&path))
}

#[tauri::command]
pub fn restore_backup(state: St<'_>, path: String) -> Result<String> {
    let archived = backup::restore(&state, std::path::Path::new(&path))?;
    Ok(archived.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Maintenance & diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn run_maintenance(app: AppHandle, state: St<'_>) -> Result<maintenance::MaintenanceReport> {
    let report = maintenance::run_once(&state)?;
    events::history_changed(&app);
    Ok(report)
}

#[tauri::command]
pub fn compact_database(state: St<'_>) -> Result<i64> {
    maintenance::compact(&state)
}

#[derive(serde::Serialize)]
pub struct Diagnostics {
    pub version: String,
    pub data_dir: String,
    pub db_bytes: i64,
    pub blob_bytes: u64,
    pub schema_version: i32,
    pub watcher_running: bool,
    pub vault_unlocked: bool,
}

#[tauri::command]
pub fn diagnostics(state: St<'_>) -> Result<Diagnostics> {
    let conn = state.conn()?;
    Ok(Diagnostics {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        db_bytes: crate::infra::db::size_bytes(&conn)?,
        blob_bytes: state.blobs.total_bytes().unwrap_or(0),
        schema_version: conn.query_row("PRAGMA user_version", [], |r| r.get(0))?,
        watcher_running: state.watcher().map(|w| w.is_running()).unwrap_or(false),
        vault_unlocked: state.is_vault_unlocked(),
    })
}

// ---------------------------------------------------------------------------
// Window control
// ---------------------------------------------------------------------------

/// Hide the launcher without quitting.
#[tauri::command]
pub fn hide_launcher(app: AppHandle) {
    window::hide_launcher(&app);
}

#[tauri::command]
pub fn show_main(app: AppHandle) -> Result<()> {
    window::show_main(&app)
}

/// Remember the foreground window so a later paste lands in the right place.
#[tauri::command]
pub fn remember_focus(state: St<'_>) {
    let info = platform::foreground_window();
    state.remember_focus(info.hwnd);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Open the data directory in the system file manager.
#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: St<'_>) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(state.data_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| Error::other(e.to_string()))
}

/// Reveal a copied file in the file manager.
#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    if !std::path::Path::new(&path).exists() {
        return Err(Error::NotFound);
    }
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| Error::other(e.to_string()))
}

/// Open a captured link in the default browser.
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(normalize_url(&url)?, None::<&str>)
        .map_err(|e| Error::other(e.to_string()))
}

/// Reduce arbitrary clipboard text to a URL that is safe to hand the OS.
///
/// Clipboard content is untrusted: it can name any scheme the machine has a
/// handler for, including ones that launch programs. Only `http` and `https`
/// are ever passed through; anything carrying a different explicit scheme is
/// rejected outright, and anything schemeless is forced to `https`, so this
/// function cannot produce a URL that starts anything but a browser.
fn normalize_url(url: &str) -> Result<String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::invalid("no link to open"));
    }

    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(url.to_string());
    }

    // A scheme is `name:` before any `/`, `?` or `#`. Checking for a bare colon
    // this way is what stops `javascript:`, `file:` and `ms-settings:` while
    // still allowing `example.com:8080/path`.
    let authority_end = url.find(['/', '?', '#']).unwrap_or(url.len());
    if let Some(colon) = url[..authority_end].find(':') {
        let scheme = &url[..colon];
        // The part after the colon must be a non-empty run of digits to count
        // as a port. Requiring non-empty matters: `file:///…` has nothing
        // between the colon and the slash, and an empty `all()` is vacuously
        // true, which would let every `scheme:///…` URL through.
        let after_colon = &url[colon + 1..authority_end];
        let port_like = !after_colon.is_empty() && after_colon.chars().all(|c| c.is_ascii_digit());
        let looks_like_scheme = !scheme.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');

        if looks_like_scheme && !port_like {
            return Err(Error::invalid("only http and https links can be opened"));
        }
    }

    Ok(format!("https://{url}"))
}

/// Everything the UI needs on startup, in a single round trip.
#[derive(serde::Serialize)]
pub struct Bootstrap {
    pub settings: Settings,
    pub stats: Stats,
    pub tags: Vec<Tag>,
    pub collections: Vec<Collection>,
    pub vault: VaultStatus,
    pub version: String,
    pub platform: String,
}

#[tauri::command]
pub fn bootstrap(state: St<'_>) -> Result<Bootstrap> {
    let conn = state.conn()?;
    let vault_config = crate::config::load_vault(&conn)?;
    Ok(Bootstrap {
        settings: state.settings(),
        stats: items::stats(&conn)?,
        tags: meta::list_tags(&conn)?,
        collections: meta::list_collections(&conn)?,
        vault: VaultStatus {
            configured: vault_config.is_some(),
            unlocked: state.is_vault_unlocked(),
            requires_passphrase: vault_config.map(|c| c.passphrase).unwrap_or(false),
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
    })
}

/// Registration list. Kept in one place so a new command cannot be written and
/// then silently forgotten.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        bootstrap,
        list_items,
        get_item,
        get_image,
        get_stats,
        set_pinned,
        set_favorite,
        update_item,
        delete_items,
        restore_items,
        purge_items,
        empty_trash,
        clear_history,
        use_item,
        copy_text,
        capture_now,
        set_capture_paused,
        is_capture_paused,
        list_tags,
        create_tag,
        update_tag,
        delete_tag,
        tag_items,
        untag_items,
        list_collections,
        create_collection,
        update_collection,
        delete_collection,
        add_to_collection,
        remove_from_collection,
        reorder_collection,
        get_settings,
        save_settings,
        list_ignored_apps,
        add_ignored_app,
        remove_ignored_app,
        vault_status,
        vault_unlock,
        vault_lock,
        vault_set_passphrase,
        transform_text,
        summarize_item,
        export_history,
        import_history,
        create_backup,
        restore_backup,
        run_maintenance,
        compact_database,
        diagnostics,
        hide_launcher,
        show_main,
        remember_focus,
        quit_app,
        open_data_dir,
        reveal_path,
        open_url,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_http_and_https_through() {
        assert_eq!(normalize_url("https://example.com/a?b=1").unwrap(), "https://example.com/a?b=1");
        assert_eq!(normalize_url("http://example.com").unwrap(), "http://example.com");
        // Scheme matching must be case-insensitive.
        assert_eq!(normalize_url("HTTPS://example.com").unwrap(), "HTTPS://example.com");
    }

    #[test]
    fn upgrades_schemeless_links() {
        assert_eq!(normalize_url("www.example.com").unwrap(), "https://www.example.com");
        assert_eq!(normalize_url("example.com/path").unwrap(), "https://example.com/path");
        assert_eq!(normalize_url("  example.com  ").unwrap(), "https://example.com");
    }

    #[test]
    fn keeps_explicit_ports() {
        assert_eq!(normalize_url("example.com:8080").unwrap(), "https://example.com:8080");
        assert_eq!(normalize_url("localhost:1420/x").unwrap(), "https://localhost:1420/x");
    }

    #[test]
    fn refuses_every_other_scheme() {
        // Clipboard content can name any handler the machine has registered.
        for hostile in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "ms-settings:privacy",
            "vbscript:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "shell:startup",
            "search-ms:query=x",
        ] {
            assert!(
                normalize_url(hostile).is_err(),
                "{hostile:?} was not rejected",
            );
        }
    }

    #[test]
    fn refuses_empty_input() {
        assert!(normalize_url("").is_err());
        assert!(normalize_url("   ").is_err());
    }

    #[test]
    fn never_yields_a_non_http_scheme() {
        // The property that matters: whatever comes out can only start a browser.
        for input in ["example.com", "www.a.b", "sub.domain.co.uk:443/p", "1.2.3.4"] {
            let out = normalize_url(input).unwrap().to_ascii_lowercase();
            assert!(
                out.starts_with("http://") || out.starts_with("https://"),
                "{input:?} produced {out:?}",
            );
        }
    }
}
