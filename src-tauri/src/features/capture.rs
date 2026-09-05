//! The capture pipeline.
//!
//! A clipboard event flows through here: read → dedupe-hash → policy checks →
//! classify → encrypt if sensitive → store blob → persist → notify the UI.
//!
//! Ordering is deliberate. Hashing happens before classification because the
//! cheapest way to handle a re-copy is to notice it immediately and skip all
//! remaining work. Policy checks happen before classification so a copy from an
//! ignored password manager never even gets scanned.

use crate::app::state::AppState;
use crate::domain::classify::{self, PREVIEW_LIMIT};
use crate::domain::{Kind, Snapshot};
use crate::error::Result;
use crate::infra::clipboard::reader;
use crate::infra::db::items;
use crate::infra::{crypto, platform};
use crate::util;

/// Text longer than this moves to the blob store instead of living in the row.
/// Keeping huge payloads out of the table is what keeps list queries fast.
const INLINE_TEXT_LIMIT: usize = 256 * 1024;

/// Longest edge of the thumbnail generated for image items.
const THUMB_EDGE: u32 = 220;

/// Outcome of handling one clipboard event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// A new item was stored.
    Stored(i64),
    /// The content already existed and was moved to the top.
    Duplicate(i64),
    /// Deliberately not captured (paused, ignored app, oversized, empty).
    Skipped(SkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SkipReason {
    CaptureDisabled,
    IgnoredApp,
    TooLarge,
    EmptyOrUnsupported,
    KindDisabled,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::CaptureDisabled => "capture_disabled",
            SkipReason::IgnoredApp => "ignored_app",
            SkipReason::TooLarge => "too_large",
            SkipReason::EmptyOrUnsupported => "empty_or_unsupported",
            SkipReason::KindDisabled => "kind_disabled",
        }
    }
}

/// Handle a single clipboard change.
pub fn capture(state: &AppState) -> Result<Outcome> {
    let settings = state.settings();

    if !settings.capture_enabled {
        return Ok(Outcome::Skipped(SkipReason::CaptureDisabled));
    }

    // Attribute the capture before touching the clipboard: reading it can take
    // a few milliseconds, by which time focus may have moved.
    let window = platform::foreground_window();

    if settings.respect_ignore_list {
        if let Some(app) = window.app.as_deref() {
            if state.is_ignored_app(app) {
                tracing::debug!(app, "skipping capture from an ignored application");
                return Ok(Outcome::Skipped(SkipReason::IgnoredApp));
            }
        }
    }

    let Some(snapshot) = reader::read_snapshot()? else {
        return Ok(Outcome::Skipped(SkipReason::EmptyOrUnsupported));
    };

    if snapshot.is_empty() {
        return Ok(Outcome::Skipped(SkipReason::EmptyOrUnsupported));
    }

    match &snapshot {
        Snapshot::Image { .. } if !settings.capture_images => {
            return Ok(Outcome::Skipped(SkipReason::KindDisabled));
        }
        Snapshot::Files(_) if !settings.capture_files => {
            return Ok(Outcome::Skipped(SkipReason::KindDisabled));
        }
        _ => {}
    }

    let material = snapshot.hash_material();
    if material.len() > settings.max_capture_bytes {
        tracing::info!(bytes = material.len(), "capture exceeds the size limit");
        return Ok(Outcome::Skipped(SkipReason::TooLarge));
    }

    let hash = util::hash_bytes(&material);
    let now = util::now_ms();

    // Fast path: identical content already in the history. `insert` handles the
    // bump, so we do not classify, encrypt or write a blob for a re-copy.
    {
        let conn = state.conn()?;
        if let Some(id) = existing_id(&conn, &hash)? {
            items::touch(&conn, id, "copy", now)?;
            return Ok(Outcome::Duplicate(id));
        }
    }

    let mut item = classify::classify(&snapshot, hash);
    item.source_app = window.app.clone();
    item.source_title = window.title.clone();

    // Binary payloads and oversized text go to the content-addressed store.
    match &snapshot {
        Snapshot::Image { png, .. } => {
            item.blob = Some(state.blobs.put(png)?);
            item.meta.thumb = reader::thumbnail(png, THUMB_EDGE);
            item.body = None;
        }
        _ => {
            if let Some(body) = &item.body {
                if body.len() > INLINE_TEXT_LIMIT {
                    item.blob = Some(state.blobs.put(body.as_bytes())?);
                    item.body = None;
                }
            }
        }
    }

    // Encrypt credentials before they ever reach the database file.
    let cipher = if item.sensitive && settings.encrypt_secrets {
        match (state.vault_key(), item.body.as_deref()) {
            (Some(key), Some(body)) => {
                let blob = crypto::encrypt_str(&key, body)?;
                item.body = None;
                Some(blob)
            }
            // Without an unlocked vault we still capture the item, but only its
            // masked preview — losing the payload beats storing it in the clear.
            (None, _) => {
                tracing::warn!("vault is locked; storing only the masked preview of a secret");
                item.body = None;
                None
            }
            (_, None) => None,
        }
    } else {
        None
    };

    let conn = state.conn()?;
    let captured = items::insert(&conn, &item, cipher.as_deref(), now)?;

    Ok(match captured {
        items::Captured::Inserted(id) => Outcome::Stored(id),
        items::Captured::Deduped(id) => Outcome::Duplicate(id),
    })
}

fn existing_id(conn: &rusqlite::Connection, hash: &str) -> Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT id FROM items WHERE hash = ?1 AND deleted_at IS NULL",
            rusqlite::params![hash],
            |r| r.get(0),
        )
        .optional()?)
}

/// Rebuild the preview line for edited text. Kept next to capture so the list
/// view renders edited items exactly like captured ones.
pub fn preview_for(kind: Kind, body: &str) -> String {
    match kind {
        Kind::Secret => format!("{}{} ({} chars)", &body.chars().take(4).collect::<String>(), "•".repeat(8), body.chars().count()),
        _ => classify::truncate(body.trim(), PREVIEW_LIMIT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_previews_stay_masked() {
        let preview = preview_for(Kind::Secret, "ghp_supersecrettokenvalue");
        assert!(!preview.contains("supersecret"));
        assert!(preview.contains('•'));
    }

    #[test]
    fn text_previews_are_truncated() {
        let long = "x".repeat(PREVIEW_LIMIT * 2);
        let preview = preview_for(Kind::Text, &long);
        assert!(preview.chars().count() <= PREVIEW_LIMIT + 1);
    }

    #[test]
    fn skip_reasons_have_stable_names() {
        assert_eq!(SkipReason::IgnoredApp.as_str(), "ignored_app");
    }
}
