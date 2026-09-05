//! Putting an item back on the clipboard, and optionally pasting it.
//!
//! The tricky part is not the clipboard write — it is making sure that write
//! does not come straight back to us as a new capture, and that focus returns
//! to the window the user was actually typing in.

use crate::app::state::AppState;
use crate::domain::Kind;
use crate::error::{Error, Result};
use crate::infra::clipboard::writer;
use crate::infra::db::items;
use crate::util;

/// What to do after the item reaches the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    /// Only copy — the user pastes manually.
    CopyOnly,
    /// Copy, then synthesise Ctrl+V into the previously focused window.
    Paste,
    /// Copy the plain-text flavour even for rich items.
    CopyPlain,
}

/// Resolve an item's payload and place it on the clipboard.
pub fn copy_to_clipboard(state: &AppState, id: i64, mode: PasteMode) -> Result<()> {
    let conn = state.conn()?;
    let item = items::get(&conn, id)?;

    // Tell the watcher to ignore the change we are about to make, otherwise
    // every paste would append a duplicate to the history.
    state.expect_self_write();

    match item.kind {
        Kind::Image => {
            let key = item.blob.as_deref().ok_or(Error::NotFound)?;
            let png = state.blobs.get(key)?;
            writer::write_image(&png)?;
        }
        Kind::Files => {
            if item.meta.files.is_empty() {
                let body = resolve_text(state, &conn, &item)?;
                writer::write_text(&body)?;
            } else {
                writer::write_files(&item.meta.files)?;
            }
        }
        Kind::Rich if mode != PasteMode::CopyPlain => {
            let plain = resolve_text(state, &conn, &item)?;
            match item.meta.html.as_deref() {
                Some(html) => writer::write_html(html, &plain)?,
                None => writer::write_text(&plain)?,
            }
        }
        _ => {
            let body = resolve_text(state, &conn, &item)?;
            writer::write_text(&body)?;
        }
    }

    items::touch(&conn, id, if mode == PasteMode::Paste { "paste" } else { "copy" }, util::now_ms())?;
    Ok(())
}

/// Copy, then paste into the window that had focus before the launcher opened.
pub fn paste_item(state: &AppState, id: i64, mode: PasteMode) -> Result<()> {
    copy_to_clipboard(state, id, mode)?;
    if mode == PasteMode::Paste {
        crate::infra::platform::paste_into(state.last_focus())?;
    }
    Ok(())
}

/// Put arbitrary text on the clipboard (used by transforms and the AI panel).
pub fn copy_text(state: &AppState, text: &str) -> Result<()> {
    state.expect_self_write();
    writer::write_text(text)
}

/// Fetch an item's text, transparently handling blob-backed and encrypted
/// payloads.
pub fn resolve_text(
    state: &AppState,
    conn: &rusqlite::Connection,
    item: &crate::domain::Item,
) -> Result<String> {
    if item.encrypted {
        let cipher = items::cipher_of(conn, item.id)?;
        return state.decrypt(&cipher);
    }
    if let Some(body) = &item.body {
        return Ok(body.clone());
    }
    if let Some(key) = &item.blob {
        let bytes = state.blobs.get(key)?;
        return Ok(String::from_utf8_lossy(&bytes).to_string());
    }
    // Nothing left to read: the payload was dropped (e.g. a secret captured
    // while the vault was locked). The masked preview is all that exists.
    Err(Error::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::item::{Meta, NewItem};

    fn temp_state() -> (std::sync::Arc<AppState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("nexus-paste-{}", uuid::Uuid::new_v4()));
        (AppState::new(dir.clone()).unwrap(), dir)
    }

    #[test]
    fn resolves_inline_text() {
        let (state, dir) = temp_state();
        let conn = state.conn().unwrap();
        let id = items::insert(
            &conn,
            &NewItem {
                kind: Kind::Text,
                preview: "hi".into(),
                body: Some("hi there".into()),
                blob: None,
                bytes: 8,
                meta: Meta::default(),
                hash: "h1".into(),
                source_app: None,
                source_title: None,
                sensitive: false,
            },
            None,
            1,
        )
        .unwrap()
        .id();

        let item = items::get(&conn, id).unwrap();
        assert_eq!(resolve_text(&state, &conn, &item).unwrap(), "hi there");

        drop(conn);
        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn resolves_blob_backed_text() {
        let (state, dir) = temp_state();
        let key = state.blobs.put(b"a very long payload").unwrap();
        let conn = state.conn().unwrap();
        let id = items::insert(
            &conn,
            &NewItem {
                kind: Kind::Text,
                preview: "a very long…".into(),
                body: None,
                blob: Some(key),
                bytes: 19,
                meta: Meta::default(),
                hash: "h2".into(),
                source_app: None,
                source_title: None,
                sensitive: false,
            },
            None,
            1,
        )
        .unwrap()
        .id();

        let item = items::get(&conn, id).unwrap();
        assert_eq!(resolve_text(&state, &conn, &item).unwrap(), "a very long payload");

        drop(conn);
        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn encrypted_item_needs_an_unlocked_vault() {
        let (state, dir) = temp_state();
        // A fresh install opens a machine-keyed vault; lock it so this exercises
        // the passphrase-protected case.
        state.set_vault_key(None);

        let conn = state.conn().unwrap();
        let id = items::insert(
            &conn,
            &NewItem {
                kind: Kind::Secret,
                preview: "abcd••••••••".into(),
                body: None,
                blob: None,
                bytes: 10,
                meta: Meta::default(),
                hash: "h3".into(),
                source_app: None,
                source_title: None,
                sensitive: true,
            },
            Some(b"ciphertext-placeholder"),
            1,
        )
        .unwrap()
        .id();

        let item = items::get(&conn, id).unwrap();
        assert!(matches!(resolve_text(&state, &conn, &item), Err(Error::VaultLocked)));

        drop(conn);
        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }
}
