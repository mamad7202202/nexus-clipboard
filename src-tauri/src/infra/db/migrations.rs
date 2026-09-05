//! Versioned schema migrations.
//!
//! Each entry is applied exactly once, in order, inside a transaction, and the
//! applied version is recorded in `user_version`. Migrations are append-only:
//! never edit a shipped one, add a new entry instead.

use rusqlite::Connection;

use crate::error::Result;

/// `(version, sql)` pairs. `version` must be contiguous and ascending.
const MIGRATIONS: &[(i32, &str)] = &[
    (
        1,
        r#"
-- ---------------------------------------------------------------------------
-- Core history table
-- ---------------------------------------------------------------------------
CREATE TABLE items (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid          TEXT    NOT NULL UNIQUE,
    kind          TEXT    NOT NULL,
    -- Short excerpt; always plaintext-safe (secrets are masked before storing).
    preview       TEXT    NOT NULL,
    -- Full payload for text-like kinds. NULL when blob-backed or encrypted.
    body          TEXT,
    -- Ciphertext for encrypted items (nonce is prefixed inside the blob).
    cipher        BLOB,
    -- Content-addressed key into the blob store for binary payloads.
    blob          TEXT,
    bytes         INTEGER NOT NULL DEFAULT 0,
    -- BLAKE3 of the canonical content; drives deduplication.
    hash          TEXT    NOT NULL,
    meta          TEXT    NOT NULL DEFAULT '{}',
    source_app    TEXT,
    source_title  TEXT,
    pinned        INTEGER NOT NULL DEFAULT 0,
    favorite      INTEGER NOT NULL DEFAULT 0,
    encrypted     INTEGER NOT NULL DEFAULT 0,
    sensitive     INTEGER NOT NULL DEFAULT 0,
    use_count     INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    last_used_at  INTEGER NOT NULL,
    -- Soft delete. NULL means live; a timestamp means it is in the trash.
    deleted_at    INTEGER
) STRICT;

-- Dedupe lookups hit this constantly; partial index keeps it tiny by excluding
-- trashed rows, which are never dedupe targets.
CREATE UNIQUE INDEX idx_items_hash_live ON items(hash) WHERE deleted_at IS NULL;

-- The default timeline: live rows ordered by recency.
CREATE INDEX idx_items_timeline   ON items(updated_at DESC, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_items_created    ON items(created_at DESC, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_items_kind       ON items(kind, updated_at DESC)    WHERE deleted_at IS NULL;
CREATE INDEX idx_items_pinned     ON items(updated_at DESC)          WHERE pinned = 1 AND deleted_at IS NULL;
CREATE INDEX idx_items_favorite   ON items(updated_at DESC)          WHERE favorite = 1 AND deleted_at IS NULL;
CREATE INDEX idx_items_sensitive  ON items(updated_at DESC)          WHERE sensitive = 1 AND deleted_at IS NULL;
CREATE INDEX idx_items_app        ON items(source_app, updated_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_items_frequency  ON items(use_count DESC, id DESC)  WHERE deleted_at IS NULL;
CREATE INDEX idx_items_size       ON items(bytes DESC, id DESC)      WHERE deleted_at IS NULL;
CREATE INDEX idx_items_trash      ON items(deleted_at DESC)          WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_items_blob       ON items(blob)                     WHERE blob IS NOT NULL;

-- ---------------------------------------------------------------------------
-- Full-text search (external content: the index stores no duplicate payload)
-- ---------------------------------------------------------------------------
CREATE VIRTUAL TABLE items_fts USING fts5(
    preview,
    body,
    content='items',
    content_rowid='id',
    tokenize="unicode61 remove_diacritics 2"
);

-- Encrypted rows deliberately contribute only their masked preview to the
-- index, so the plaintext of a secret is never searchable on disk.
CREATE TRIGGER items_ai AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, preview, body)
    VALUES (new.id, new.preview, CASE WHEN new.encrypted = 1 THEN '' ELSE new.body END);
END;

CREATE TRIGGER items_ad AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, preview, body)
    VALUES ('delete', old.id, old.preview, CASE WHEN old.encrypted = 1 THEN '' ELSE old.body END);
END;

CREATE TRIGGER items_au AFTER UPDATE OF preview, body, encrypted ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, preview, body)
    VALUES ('delete', old.id, old.preview, CASE WHEN old.encrypted = 1 THEN '' ELSE old.body END);
    INSERT INTO items_fts(rowid, preview, body)
    VALUES (new.id, new.preview, CASE WHEN new.encrypted = 1 THEN '' ELSE new.body END);
END;

-- ---------------------------------------------------------------------------
-- Tags
-- ---------------------------------------------------------------------------
CREATE TABLE tags (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    name   TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color  TEXT NOT NULL DEFAULT '#6366f1',
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE item_tags (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (item_id, tag_id)
) STRICT;

CREATE INDEX idx_item_tags_tag ON item_tags(tag_id, item_id);

-- ---------------------------------------------------------------------------
-- Collections (ordered, user-curated groupings)
-- ---------------------------------------------------------------------------
CREATE TABLE collections (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    name   TEXT NOT NULL UNIQUE COLLATE NOCASE,
    icon   TEXT NOT NULL DEFAULT 'folder',
    color  TEXT NOT NULL DEFAULT '#8b5cf6',
    sort   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE item_collections (
    item_id       INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    position      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, collection_id)
) STRICT;

CREATE INDEX idx_item_collections ON item_collections(collection_id, position);

-- ---------------------------------------------------------------------------
-- Settings: a single-row-per-key JSON store
-- ---------------------------------------------------------------------------
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- Applications whose clipboard writes are ignored entirely (password managers).
CREATE TABLE ignored_apps (
    name TEXT PRIMARY KEY COLLATE NOCASE
) STRICT;
"#,
    ),
    (
        2,
        r#"
-- Semantic search support: one embedding per item, written lazily by the AI
-- feature. Stored as raw little-endian f32 so it can be memory-mapped cheaply.
CREATE TABLE embeddings (
    item_id   INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    model     TEXT NOT NULL,
    dim       INTEGER NOT NULL,
    vector    BLOB NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;
"#,
    ),
    (
        3,
        r#"
-- Usage journal: every paste is recorded so the timeline and the "frequent"
-- ranking survive item edits, and so analytics never has to mutate items.
CREATE TABLE usage (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    at      INTEGER NOT NULL,
    -- 'paste' | 'copy' | 'view'
    action  TEXT NOT NULL
) STRICT;

CREATE INDEX idx_usage_item ON usage(item_id, at DESC);
CREATE INDEX idx_usage_time ON usage(at DESC);
"#,
    ),
];

/// Latest schema version this build knows how to produce.
pub fn target_version() -> i32 {
    MIGRATIONS.last().map(|(v, _)| *v).unwrap_or(0)
}

/// Bring `conn` up to [`target_version`]. Safe to call on every startup.
pub fn run(conn: &mut Connection) -> Result<()> {
    let current: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let target = target_version();

    if current > target {
        // The database was written by a newer build. Refusing here is safer than
        // running a downgrade we have no migration for.
        return Err(crate::error::Error::other(format!(
            "database schema v{current} is newer than this build (v{target}); update the app"
        )));
    }

    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        tracing::info!(version, "applying migration");
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        // PRAGMA cannot be parameterised.
        tx.pragma_update(None, "user_version", *version)?;
        tx.commit()?;
    }

    Ok(())
}
