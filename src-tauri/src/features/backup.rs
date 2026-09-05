//! Backup, restore, export and import.
//!
//! Two distinct formats, because they serve different needs:
//! - **Backup** (`.nxbak`) is a zip holding a consistent SQLite snapshot plus
//!   every referenced blob. It restores the app to an exact prior state.
//! - **Export** (`.json`) is a readable, portable dump of the history, meant
//!   for moving to another tool or keeping an archive you can grep.
//!
//! Neither format ever contains vault key material, and encrypted items are
//! exported only if the vault is unlocked and the user opts in.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::app::state::AppState;
use crate::domain::{Item, Query, SmartFilter};
use crate::error::{Error, Result};
use crate::infra::db::items;
use crate::util;

const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFile {
    pub version: u32,
    pub app: String,
    pub exported_at: i64,
    pub count: usize,
    pub items: Vec<ExportItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportItem {
    pub kind: String,
    pub preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    pub pinned: bool,
    pub favorite: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExportOptions {
    /// Include items flagged sensitive. Requires an unlocked vault.
    pub include_secrets: bool,
    /// Include binary payloads (images) as base64. Makes files much larger.
    pub include_blobs: bool,
    /// Only export pinned and favourited items.
    pub only_starred: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { include_secrets: false, include_blobs: false, only_starred: false }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    pub read: usize,
    pub imported: usize,
    pub duplicates: usize,
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Write a JSON export to `path`.
pub fn export_json(state: &AppState, path: &Path, options: ExportOptions) -> Result<usize> {
    let conn = state.conn()?;

    let filter = if options.only_starred { SmartFilter::Favorites } else { SmartFilter::All };
    let mut query = Query { filter, limit: Query::MAX_LIMIT, ..Default::default() };

    let mut exported = Vec::new();

    // Page through the whole history rather than loading it at once: an export
    // of a million rows must not need a million rows of RAM.
    loop {
        let page = items::list(&conn, &query)?;
        if page.items.is_empty() {
            break;
        }

        for item in &page.items {
            if item.sensitive && !options.include_secrets {
                continue;
            }
            match to_export_item(state, &conn, item, options) {
                Ok(entry) => exported.push(entry),
                Err(e) => tracing::warn!(id = item.id, ?e, "skipping an item during export"),
            }
        }

        match page.next {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }

    let file = ExportFile {
        version: EXPORT_VERSION,
        app: "nexus-clipboard".into(),
        exported_at: util::now_ms(),
        count: exported.len(),
        items: exported,
    };

    let json = serde_json::to_string_pretty(&file)?;
    write_atomic(path, json.as_bytes())?;

    Ok(file.count)
}

fn to_export_item(
    state: &AppState,
    conn: &rusqlite::Connection,
    item: &Item,
    options: ExportOptions,
) -> Result<ExportItem> {
    let body = if item.encrypted {
        if !options.include_secrets {
            None
        } else {
            let cipher = items::cipher_of(conn, item.id)?;
            Some(state.decrypt(&cipher)?)
        }
    } else if let Some(key) = &item.blob {
        if item.kind.is_binary() {
            if options.include_blobs {
                use base64::Engine;
                let bytes = state.blobs.get(key)?;
                Some(base64::engine::general_purpose::STANDARD.encode(bytes))
            } else {
                None
            }
        } else {
            // Oversized text lives in the blob store too, and is plain UTF-8.
            let bytes = state.blobs.get(key)?;
            Some(String::from_utf8_lossy(&bytes).to_string())
        }
    } else {
        // The list query omits `body`; fetch it now.
        items::get(conn, item.id)?.body
    };

    Ok(ExportItem {
        kind: item.kind.as_str().to_string(),
        preview: item.preview.clone(),
        body,
        tags: item.tags.iter().map(|t| t.name.clone()).collect(),
        source_app: item.source_app.clone(),
        pinned: item.pinned,
        favorite: item.favorite,
        created_at: item.created_at,
    })
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Import a JSON export. Existing content is deduplicated by hash, so importing
/// the same file twice is harmless.
pub fn import_json(state: &AppState, path: &Path) -> Result<ImportReport> {
    let raw = std::fs::read_to_string(path)?;
    let file: ExportFile = serde_json::from_str(&raw)
        .map_err(|e| Error::invalid(format!("not a valid Nexus export: {e}")))?;

    if file.version > EXPORT_VERSION {
        return Err(Error::invalid(format!(
            "export format v{} is newer than this build supports (v{EXPORT_VERSION})",
            file.version
        )));
    }

    let mut report = ImportReport { read: file.items.len(), ..Default::default() };
    let mut conn = state.conn()?;
    let tx = conn.transaction()?;

    for entry in &file.items {
        let Some(body) = entry.body.as_deref().filter(|b| !b.is_empty()) else {
            report.skipped += 1;
            continue;
        };

        let snapshot = crate::domain::Snapshot::Text { text: body.to_string(), html: None };
        let hash = util::hash_bytes(&snapshot.hash_material());
        let mut item = crate::domain::classify::classify(&snapshot, hash);
        item.source_app = entry.source_app.clone();

        // Imported secrets need the vault; without it, skip rather than store
        // a credential in the clear.
        let cipher = if item.sensitive && state.settings().encrypt_secrets {
            match state.vault_key() {
                Some(key) => {
                    let blob = crate::infra::crypto::encrypt_str(&key, body)?;
                    item.body = None;
                    Some(blob)
                }
                None => {
                    report.skipped += 1;
                    continue;
                }
            }
        } else {
            None
        };

        // Preserve the original capture time so imported history lands in the
        // right place on the timeline.
        let at = if entry.created_at > 0 { entry.created_at } else { util::now_ms() };

        match items::insert(&tx, &item, cipher.as_deref(), at)? {
            items::Captured::Inserted(id) => {
                report.imported += 1;
                if entry.pinned {
                    items::set_pinned(&tx, id, true)?;
                }
                if entry.favorite {
                    items::set_favorite(&tx, id, true)?;
                }
                for tag_name in &entry.tags {
                    let tag = crate::infra::db::meta::upsert_tag(&tx, tag_name, "#6366f1", at)?;
                    crate::infra::db::meta::tag_items(&tx, &[id], tag.id)?;
                }
            }
            items::Captured::Deduped(_) => report.duplicates += 1,
        }
    }

    tx.commit()?;
    Ok(report)
}

// ---------------------------------------------------------------------------
// Backup / restore
// ---------------------------------------------------------------------------

/// Write a full backup archive. Uses SQLite's online backup API so the snapshot
/// is consistent even while the watcher is writing.
pub fn backup(state: &AppState, path: &Path) -> Result<u64> {
    let staging = state.data_dir.join("backup-staging.db");
    // A leftover staging file from a crashed run would corrupt the snapshot.
    let _ = std::fs::remove_file(&staging);

    {
        let source = state.conn()?;
        let mut dest = rusqlite::Connection::open(&staging)?;
        let backup = rusqlite::backup::Backup::new(&source, &mut dest)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
    }

    let file = File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("history.db", options)?;
    let mut db_bytes = Vec::new();
    File::open(&staging)?.read_to_end(&mut db_bytes)?;
    zip.write_all(&db_bytes)?;

    // Blobs are already compressed (PNG), so store them without a second pass.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let referenced = {
        let conn = state.conn()?;
        items::referenced_blobs(&conn)?
    };
    for key in &referenced {
        let Ok(bytes) = state.blobs.get(key) else { continue };
        zip.start_file(format!("blobs/{key}.bin"), stored)?;
        zip.write_all(&bytes)?;
    }

    zip.start_file("manifest.json", options)?;
    let manifest = serde_json::json!({
        "app": "nexus-clipboard",
        "version": EXPORT_VERSION,
        "created_at": util::now_ms(),
        "blobs": referenced.len(),
    });
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    zip.finish()?;
    let _ = std::fs::remove_file(&staging);

    Ok(std::fs::metadata(path)?.len())
}

/// Restore from a backup archive.
///
/// The current database is moved aside rather than deleted, so a restore that
/// turns out to be the wrong archive is recoverable. The caller must restart
/// the app afterwards — open connections still point at the old file.
pub fn restore(state: &AppState, path: &Path) -> Result<PathBuf> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::invalid(format!("not a valid backup archive: {e}")))?;

    // Validate before touching anything on disk.
    archive
        .by_name("history.db")
        .map_err(|_| Error::invalid("archive does not contain history.db"))?;

    let incoming = state.data_dir.join("history.restored.db");
    {
        let mut entry = archive.by_name("history.db").map_err(|e| Error::other(e.to_string()))?;
        let mut out = File::create(&incoming)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| Error::other(e.to_string()))?;
        let Some(name) = entry.enclosed_name().map(|p| p.to_path_buf()) else { continue };
        let Some(stem) = name.strip_prefix("blobs").ok().and_then(|p| p.file_stem()) else {
            continue;
        };
        let key = stem.to_string_lossy().to_string();
        if state.blobs.exists(&key) {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        state.blobs.put(&bytes)?;
    }

    // Swap the live database aside; the caller restarts into the restored one.
    let live = state.data_dir.join("history.db");
    let archived = state
        .data_dir
        .join(format!("history.replaced-{}.db", util::now_ms()));
    if live.exists() {
        std::fs::rename(&live, &archived)?;
    }
    std::fs::rename(&incoming, &live)?;

    Ok(archived)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::item::{Kind, Meta, NewItem};

    fn temp_state() -> (std::sync::Arc<AppState>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nexus-backup-{}", uuid::Uuid::new_v4()));
        (AppState::new(dir.clone()).unwrap(), dir)
    }

    fn text_item(hash: &str, body: &str) -> NewItem {
        NewItem {
            kind: Kind::Text,
            preview: body.into(),
            body: Some(body.into()),
            blob: None,
            bytes: body.len() as i64,
            meta: Meta::default(),
            hash: hash.into(),
            source_app: Some("test.exe".into()),
            source_title: None,
            sensitive: false,
        }
    }

    #[test]
    fn export_then_import_round_trips() {
        let (source, source_dir) = temp_state();
        {
            let conn = source.conn().unwrap();
            items::insert(&conn, &text_item("h1", "first entry"), None, 1_000).unwrap();
            items::insert(&conn, &text_item("h2", "second entry"), None, 2_000).unwrap();
        }

        let file = source_dir.join("export.json");
        let count = export_json(&source, &file, ExportOptions::default()).unwrap();
        assert_eq!(count, 2);

        let (target, target_dir) = temp_state();
        let report = import_json(&target, &file).unwrap();
        assert_eq!(report.imported, 2);

        // A second import is fully deduplicated.
        let again = import_json(&target, &file).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(again.duplicates, 2);

        drop(source);
        drop(target);
        std::fs::remove_dir_all(source_dir).ok();
        std::fs::remove_dir_all(target_dir).ok();
    }

    #[test]
    fn export_omits_secrets_by_default() {
        let (state, dir) = temp_state();
        {
            let conn = state.conn().unwrap();
            let mut secret = text_item("s1", "masked");
            secret.sensitive = true;
            secret.kind = Kind::Secret;
            items::insert(&conn, &secret, Some(b"cipher"), 1_000).unwrap();
            items::insert(&conn, &text_item("h1", "public"), None, 2_000).unwrap();
        }

        let file = dir.join("export.json");
        let count = export_json(&state, &file, ExportOptions::default()).unwrap();
        assert_eq!(count, 1, "the sensitive item should not be exported");

        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn import_rejects_a_foreign_file() {
        let (state, dir) = temp_state();
        let bad = dir.join("bad.json");
        std::fs::write(&bad, "{\"hello\":\"world\"}").unwrap();
        assert!(import_json(&state, &bad).is_err());
        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn backup_produces_a_readable_archive() {
        let (state, dir) = temp_state();
        {
            let conn = state.conn().unwrap();
            items::insert(&conn, &text_item("h1", "backed up"), None, 1_000).unwrap();
        }

        let archive = dir.join("snapshot.nxbak");
        let size = backup(&state, &archive).unwrap();
        assert!(size > 0);

        let file = File::open(&archive).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert!(zip.by_name("history.db").is_ok());
        assert!(zip.by_name("manifest.json").is_ok());

        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }
}
