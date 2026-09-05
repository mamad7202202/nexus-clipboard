//! End-to-end tests over the storage stack.
//!
//! These exercise the layers the unit tests deliberately keep apart —
//! classification feeding the repository, encryption round-tripping through
//! SQLite, retention interacting with the blob store — using a real on-disk
//! database rather than an in-memory one, so WAL behaviour and file handling
//! are covered too.

use std::path::PathBuf;
use std::sync::Arc;

use nexus_lib::app::state::AppState;
use nexus_lib::domain::classify;
use nexus_lib::domain::{Kind, Query, SmartFilter, Snapshot, SortBy};
use nexus_lib::features::{backup, maintenance, paste};
use nexus_lib::infra::db::{items, meta};
use nexus_lib::util;

struct TempApp {
    state: Arc<AppState>,
    dir: PathBuf,
}

impl TempApp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("nexus-e2e-{}", uuid::Uuid::new_v4()));
        let state = AppState::new(dir.clone()).expect("state");
        Self { state, dir }
    }

    /// Push a text snapshot through classification and persistence, exactly as
    /// the capture pipeline does.
    fn capture_text(&self, text: &str, at: i64) -> i64 {
        let snapshot = Snapshot::Text { text: text.into(), html: None };
        let hash = util::hash_bytes(&snapshot.hash_material());
        let mut item = classify::classify(&snapshot, hash);
        item.source_app = Some("test.exe".into());

        let cipher = if item.sensitive {
            let key = self.state.vault_key().expect("vault should be open on a fresh install");
            let blob = nexus_lib::infra::crypto::encrypt_str(&key, item.body.as_deref().unwrap())
                .expect("encrypt");
            item.body = None;
            Some(blob)
        } else {
            None
        };

        let conn = self.state.conn().expect("conn");
        items::insert(&conn, &item, cipher.as_deref(), at)
            .expect("insert")
            .id()
    }
}

impl Drop for TempApp {
    fn drop(&mut self) {
        // Best-effort: on Windows the DB file may still be mapped for a moment.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn classifies_and_stores_every_kind() {
    let app = TempApp::new();

    let cases: &[(&str, Kind)] = &[
        ("just some ordinary prose about nothing", Kind::Text),
        ("https://example.com/path?query=1", Kind::Link),
        ("someone@example.com", Kind::Email),
        ("#3366ff", Kind::Color),
        (r#"{"name": "nexus", "version": 1}"#, Kind::Json),
        (
            "pub fn main() {\n    let mut total = 0;\n    println!(\"{}\", total);\n}",
            Kind::Code,
        ),
    ];

    for (i, (text, expected)) in cases.iter().enumerate() {
        let id = app.capture_text(text, 1_000 + i as i64);
        let conn = app.state.conn().unwrap();
        let stored = items::get(&conn, id).unwrap();
        assert_eq!(stored.kind, *expected, "misclassified: {text:?}");
    }

    let conn = app.state.conn().unwrap();
    let stats = items::stats(&conn).unwrap();
    assert_eq!(stats.total, cases.len() as i64);
}

#[test]
fn secrets_are_encrypted_and_unsearchable_but_readable() {
    let app = TempApp::new();
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    let id = app.capture_text(secret, 1_000);
    let conn = app.state.conn().unwrap();
    let stored = items::get(&conn, id).unwrap();

    assert_eq!(stored.kind, Kind::Secret);
    assert!(stored.sensitive);
    assert!(stored.encrypted);
    assert!(stored.body.is_none(), "plaintext was written to the row");
    assert!(!stored.preview.contains("abcdefghij"), "preview leaked the secret");

    // The plaintext must not be reachable through search.
    let hits = items::list(
        &conn,
        &Query { text: "abcdefghijklmnopqrstuvwxyz".into(), ..Default::default() },
    )
    .unwrap();
    assert!(hits.items.is_empty(), "secret plaintext reached the FTS index");

    // But it is still recoverable through the vault.
    let recovered = paste::resolve_text(&app.state, &conn, &stored).unwrap();
    assert_eq!(recovered, secret);
}

#[test]
fn recopying_deduplicates_and_reorders() {
    let app = TempApp::new();

    let first = app.capture_text("shared content", 1_000);
    app.capture_text("something else", 2_000);
    let again = app.capture_text("shared content", 3_000);

    assert_eq!(first, again, "the same content produced two rows");

    let conn = app.state.conn().unwrap();
    assert_eq!(items::stats(&conn).unwrap().total, 2);

    // The re-copied entry should now lead the timeline.
    let page = items::list(&conn, &Query::default()).unwrap();
    assert_eq!(page.items.first().map(|i| i.id), Some(first));
}

#[test]
fn search_ranks_and_paginates_a_large_history() {
    let app = TempApp::new();

    for i in 0..250 {
        app.capture_text(&format!("entry number {i} about widgets"), 1_000 + i);
    }
    app.capture_text("a very specific unicorn sighting", 99_000);

    let conn = app.state.conn().unwrap();

    let hits = items::list(
        &conn,
        &Query { text: "unicorn".into(), sort: SortBy::Relevance, ..Default::default() },
    )
    .unwrap();
    assert_eq!(hits.items.len(), 1);
    assert!(hits.items[0].preview.contains("unicorn"));

    // Walk every page and confirm no row is skipped or repeated.
    let mut query = Query { limit: 40, ..Default::default() };
    let mut seen = std::collections::HashSet::new();
    loop {
        let page = items::list(&conn, &query).unwrap();
        for item in &page.items {
            assert!(seen.insert(item.id), "row {} appeared twice", item.id);
        }
        match page.next {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen.len(), 251);
}

#[test]
fn tags_and_collections_filter_correctly() {
    let app = TempApp::new();

    let a = app.capture_text("first tagged entry", 1_000);
    let b = app.capture_text("second tagged entry", 2_000);
    app.capture_text("untagged entry", 3_000);

    let conn = app.state.conn().unwrap();
    let tag = meta::upsert_tag(&conn, "work", "#ff0000", 1_000).unwrap();
    meta::tag_items(&conn, &[a, b], tag.id).unwrap();

    let collection = meta::create_collection(&conn, "Snippets", "code", "#00ff00", 1_000).unwrap();
    meta::add_to_collection(&conn, &[a], collection.id).unwrap();

    let tagged = items::list(&conn, &Query { tags: vec![tag.id], ..Default::default() }).unwrap();
    assert_eq!(tagged.items.len(), 2);

    let in_collection = items::list(
        &conn,
        &Query { collection: Some(collection.id), ..Default::default() },
    )
    .unwrap();
    assert_eq!(in_collection.items.len(), 1);
    assert_eq!(in_collection.items[0].id, a);

    // Counts surfaced to the sidebar must agree.
    assert_eq!(meta::list_tags(&conn).unwrap()[0].count, 2);
    assert_eq!(meta::list_collections(&conn).unwrap()[0].count, 1);
}

#[test]
fn retention_spares_pinned_tagged_and_collected_entries() {
    let app = TempApp::new();
    let now = util::now_ms();
    let old = now - 30 * 86_400_000;

    let plain = app.capture_text("old and forgettable", old);
    let pinned = app.capture_text("old but pinned", old);
    let tagged = app.capture_text("old but tagged", old);
    let collected = app.capture_text("old but collected", old);
    app.capture_text("recent", now);

    {
        let conn = app.state.conn().unwrap();
        items::set_pinned(&conn, pinned, true).unwrap();

        let tag = meta::upsert_tag(&conn, "keep", "#fff", now).unwrap();
        meta::tag_items(&conn, &[tagged], tag.id).unwrap();

        let collection = meta::create_collection(&conn, "Keep", "folder", "#fff", now).unwrap();
        meta::add_to_collection(&conn, &[collected], collection.id).unwrap();
    }

    app.state
        .update_settings(|s| s.retention = nexus_lib::config::Retention::Days(7))
        .unwrap();

    let report = maintenance::run_once(&app.state).unwrap();
    assert_eq!(report.expired, 1, "only the unprotected old entry should expire");

    let conn = app.state.conn().unwrap();
    let live: Vec<i64> = items::list(&conn, &Query::default())
        .unwrap()
        .items
        .iter()
        .map(|i| i.id)
        .collect();

    assert!(!live.contains(&plain));
    for protected in [pinned, tagged, collected] {
        assert!(live.contains(&protected), "a protected entry was removed");
    }
}

#[test]
fn trash_round_trip_preserves_content() {
    let app = TempApp::new();
    let id = app.capture_text("about to be deleted", 1_000);
    let conn = app.state.conn().unwrap();

    items::soft_delete(&conn, &[id], util::now_ms()).unwrap();
    assert!(items::list(&conn, &Query::default()).unwrap().items.is_empty());

    let trash = items::list(
        &conn,
        &Query { filter: SmartFilter::Trash, ..Default::default() },
    )
    .unwrap();
    assert_eq!(trash.items.len(), 1);

    items::restore(&conn, &[id]).unwrap();
    let restored = items::get(&conn, id).unwrap();
    assert_eq!(restored.body.as_deref(), Some("about to be deleted"));
}

#[test]
fn export_import_preserves_pins_and_tags() {
    let source = TempApp::new();
    let id = source.capture_text("worth keeping", 1_000);

    {
        let conn = source.state.conn().unwrap();
        items::set_pinned(&conn, id, true).unwrap();
        let tag = meta::upsert_tag(&conn, "important", "#ff0000", 1_000).unwrap();
        meta::tag_items(&conn, &[id], tag.id).unwrap();
    }

    let file = source.dir.join("export.json");
    assert_eq!(
        backup::export_json(&source.state, &file, backup::ExportOptions::default()).unwrap(),
        1
    );

    let target = TempApp::new();
    let report = backup::import_json(&target.state, &file).unwrap();
    assert_eq!(report.imported, 1);

    let conn = target.state.conn().unwrap();
    let imported = &items::list(&conn, &Query::default()).unwrap().items[0];
    assert!(imported.pinned);
    assert_eq!(imported.tags.len(), 1);
    assert_eq!(imported.tags[0].name, "important");
}

#[test]
fn oversized_text_moves_to_the_blob_store_and_reads_back() {
    let app = TempApp::new();
    let big = "x".repeat(400_000);

    let key = app.state.blobs.put(big.as_bytes()).unwrap();
    let conn = app.state.conn().unwrap();

    let id = items::insert(
        &conn,
        &nexus_lib::domain::item::NewItem {
            kind: Kind::Text,
            preview: "x…".into(),
            body: None,
            blob: Some(key.clone()),
            bytes: big.len() as i64,
            meta: Default::default(),
            hash: util::hash_bytes(big.as_bytes()),
            source_app: None,
            source_title: None,
            sensitive: false,
        },
        None,
        1_000,
    )
    .unwrap()
    .id();

    let item = items::get(&conn, id).unwrap();
    assert_eq!(paste::resolve_text(&app.state, &conn, &item).unwrap().len(), big.len());

    // Purging the row should release the blob.
    let orphans = items::purge(&conn, &[id]).unwrap();
    assert_eq!(orphans, vec![key.clone()]);
    app.state.blobs.remove(&key).unwrap();
    assert!(!app.state.blobs.exists(&key));
}

#[test]
fn settings_and_ignore_list_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("nexus-restart-{}", uuid::Uuid::new_v4()));

    {
        let state = AppState::new(dir.clone()).unwrap();
        state
            .update_settings(|s| {
                s.accent = "#abcdef".into();
                s.max_items = 12_345;
            })
            .unwrap();
        let conn = state.conn().unwrap();
        meta::add_ignored_app(&conn, "Custom.exe").unwrap();
        drop(conn);
        state.reload_ignored_apps().unwrap();
    }

    let reopened = AppState::new(dir.clone()).unwrap();
    let settings = reopened.settings();
    assert_eq!(settings.accent, "#abcdef");
    assert_eq!(settings.max_items, 12_345);
    assert!(reopened.is_ignored_app("custom.exe"));

    drop(reopened);
    let _ = std::fs::remove_dir_all(dir);
}
