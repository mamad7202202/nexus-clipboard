//! Item repository: every read and write of the history table.
//!
//! All SQL lives here. Callers work in domain types and never see a `Row`.
//! Queries are built by [`compile`] from a [`Query`], which keeps the filter
//! logic in one auditable place and guarantees parameters are always bound
//! rather than interpolated.

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, ToSql};

use crate::domain::{
    item::{Kind, Meta, NewItem, Tag},
    Cursor, Item, Page, Query, SmartFilter, SortBy, Stats,
};
use crate::domain::query::{AppCount, KindCount};
use crate::error::{Error, Result};

use super::search;

/// Columns needed to render a list row. Deliberately excludes `body` and
/// `cipher`: a 10 MB paste must not be read to draw a 40 px row.
const LIST_COLS: &str = "i.id, i.uuid, i.kind, i.preview, NULL as body, i.blob, i.bytes, i.meta, \
     i.source_app, i.source_title, i.pinned, i.favorite, i.encrypted, i.sensitive, \
     i.use_count, i.created_at, i.updated_at, i.last_used_at";

/// Everything, including the payload. Used when opening a single item.
const FULL_COLS: &str = "i.id, i.uuid, i.kind, i.preview, i.body, i.blob, i.bytes, i.meta, \
     i.source_app, i.source_title, i.pinned, i.favorite, i.encrypted, i.sensitive, \
     i.use_count, i.created_at, i.updated_at, i.last_used_at";

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

fn map_row(row: &Row<'_>) -> rusqlite::Result<Item> {
    let kind_str: String = row.get("kind")?;
    let meta_str: String = row.get("meta")?;

    Ok(Item {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        kind: Kind::parse(&kind_str).unwrap_or(Kind::Text),
        preview: row.get("preview")?,
        body: row.get("body")?,
        blob: row.get("blob")?,
        bytes: row.get("bytes")?,
        // A corrupt meta blob must not take down the whole list.
        meta: serde_json::from_str(&meta_str).unwrap_or_default(),
        source_app: row.get("source_app")?,
        source_title: row.get("source_title")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        favorite: row.get::<_, i64>("favorite")? != 0,
        encrypted: row.get::<_, i64>("encrypted")? != 0,
        sensitive: row.get::<_, i64>("sensitive")? != 0,
        use_count: row.get("use_count")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_used_at: row.get("last_used_at")?,
        tags: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// Result of offering a capture to the store.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Captured {
    /// A brand new row was created.
    Inserted(i64),
    /// The content already existed; its recency was refreshed instead.
    Deduped(i64),
}

impl Captured {
    pub fn id(self) -> i64 {
        match self {
            Captured::Inserted(id) | Captured::Deduped(id) => id,
        }
    }

    pub fn is_new(self) -> bool {
        matches!(self, Captured::Inserted(_))
    }
}

/// Insert a capture, or bump the existing row when the same content is copied
/// again. `cipher` carries the ciphertext for sensitive items, in which case
/// `item.body` is not persisted in the clear.
pub fn insert(conn: &Connection, item: &NewItem, cipher: Option<&[u8]>, now: i64) -> Result<Captured> {
    // Dedupe first: re-copying something should move it to the top of the
    // timeline, not create a duplicate row.
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM items WHERE hash = ?1 AND deleted_at IS NULL",
            params![item.hash],
            |r| r.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        conn.execute(
            "UPDATE items
                SET updated_at = ?2,
                    last_used_at = ?2,
                    use_count = use_count + 1,
                    -- Later captures often carry better source info.
                    source_app = COALESCE(?3, source_app),
                    source_title = COALESCE(?4, source_title)
              WHERE id = ?1",
            params![id, now, item.source_app, item.source_title],
        )?;
        return Ok(Captured::Deduped(id));
    }

    let encrypted = cipher.is_some();
    let uuid = uuid::Uuid::new_v4().to_string();
    let meta = serde_json::to_string(&item.meta)?;
    let body: Option<&str> = if encrypted { None } else { item.body.as_deref() };

    conn.execute(
        "INSERT INTO items (
            uuid, kind, preview, body, cipher, blob, bytes, hash, meta,
            source_app, source_title, encrypted, sensitive,
            created_at, updated_at, last_used_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?14)",
        params![
            uuid,
            item.kind.as_str(),
            item.preview,
            body,
            cipher,
            item.blob,
            item.bytes,
            item.hash,
            meta,
            item.source_app,
            item.source_title,
            encrypted as i64,
            item.sensitive as i64,
            now,
        ],
    )?;

    Ok(Captured::Inserted(conn.last_insert_rowid()))
}

/// Fetch one item with its full payload and tags.
pub fn get(conn: &Connection, id: i64) -> Result<Item> {
    let sql = format!("SELECT {FULL_COLS} FROM items i WHERE i.id = ?1");
    let mut item = conn
        .query_row(&sql, params![id], map_row)
        .optional()?
        .ok_or(Error::NotFound)?;
    item.tags = tags_for(conn, id)?;
    Ok(item)
}

/// The raw ciphertext for an encrypted item.
pub fn cipher_of(conn: &Connection, id: i64) -> Result<Vec<u8>> {
    conn.query_row("SELECT cipher FROM items WHERE id = ?1", params![id], |r| {
        r.get::<_, Option<Vec<u8>>>(0)
    })
    .optional()?
    .flatten()
    .ok_or(Error::NotFound)
}

/// Record that an item was pasted or re-copied: bumps recency and usage.
pub fn touch(conn: &Connection, id: i64, action: &str, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE items SET use_count = use_count + 1, last_used_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    conn.execute(
        "INSERT INTO usage (item_id, at, action) VALUES (?1, ?2, ?3)",
        params![id, now, action],
    )?;
    Ok(())
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<()> {
    conn.execute("UPDATE items SET pinned = ?2 WHERE id = ?1", params![id, pinned as i64])?;
    Ok(())
}

pub fn set_favorite(conn: &Connection, id: i64, favorite: bool) -> Result<()> {
    conn.execute("UPDATE items SET favorite = ?2 WHERE id = ?1", params![id, favorite as i64])?;
    Ok(())
}

/// Replace the text payload of an item (used by the inline editor and by AI
/// transforms). Preview and FTS follow automatically via triggers.
pub fn update_body(conn: &Connection, id: i64, body: &str, preview: &str, now: i64) -> Result<()> {
    let encrypted: i64 = conn.query_row("SELECT encrypted FROM items WHERE id = ?1", params![id], |r| r.get(0))?;
    if encrypted != 0 {
        return Err(Error::invalid("cannot edit an encrypted item in place"));
    }
    conn.execute(
        "UPDATE items SET body = ?2, preview = ?3, bytes = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, body, preview, body.len() as i64, now],
    )?;
    Ok(())
}

/// Merge new fields into an item's `meta` JSON without clobbering the rest.
pub fn patch_meta(conn: &Connection, id: i64, patch: &Meta) -> Result<()> {
    let current: String = conn.query_row("SELECT meta FROM items WHERE id = ?1", params![id], |r| r.get(0))?;
    let mut merged: Meta = serde_json::from_str(&current).unwrap_or_default();

    if patch.summary.is_some() {
        merged.summary = patch.summary.clone();
    }
    if patch.thumb.is_some() {
        merged.thumb = patch.thumb.clone();
    }
    if patch.language.is_some() {
        merged.language = patch.language.clone();
    }
    if patch.host.is_some() {
        merged.host = patch.host.clone();
    }

    conn.execute(
        "UPDATE items SET meta = ?2 WHERE id = ?1",
        params![id, serde_json::to_string(&merged)?],
    )?;
    Ok(())
}

/// Move items to the trash. Pinned items are protected from bulk deletes.
pub fn soft_delete(conn: &Connection, ids: &[i64], now: i64) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders = placeholders(ids.len());
    let sql = format!("UPDATE items SET deleted_at = ?1 WHERE id IN ({placeholders}) AND deleted_at IS NULL");
    let mut args: Vec<Box<dyn ToSql>> = vec![Box::new(now)];
    args.extend(ids.iter().map(|id| Box::new(*id) as Box<dyn ToSql>));
    Ok(conn.execute(&sql, params_from_iter(args.iter().map(|b| b.as_ref())))?)
}

pub fn restore(conn: &Connection, ids: &[i64]) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders = placeholders(ids.len());
    // Restoring must not violate the live-hash uniqueness index, so drop any
    // restore whose content has since been re-captured.
    let sql = format!(
        "UPDATE items SET deleted_at = NULL
          WHERE id IN ({placeholders})
            AND deleted_at IS NOT NULL
            AND hash NOT IN (SELECT hash FROM items WHERE deleted_at IS NULL)"
    );
    let args: Vec<Box<dyn ToSql>> = ids.iter().map(|id| Box::new(*id) as Box<dyn ToSql>).collect();
    Ok(conn.execute(&sql, params_from_iter(args.iter().map(|b| b.as_ref())))?)
}

/// Permanently remove rows. Returns the blob keys that are now unreferenced so
/// the caller can delete the files.
pub fn purge(conn: &Connection, ids: &[i64]) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = placeholders(ids.len());
    let args: Vec<Box<dyn ToSql>> = ids.iter().map(|id| Box::new(*id) as Box<dyn ToSql>).collect();

    let sql = format!("SELECT DISTINCT blob FROM items WHERE id IN ({placeholders}) AND blob IS NOT NULL");
    let mut stmt = conn.prepare(&sql)?;
    let candidates: Vec<String> = stmt
        .query_map(params_from_iter(args.iter().map(|b| b.as_ref())), |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let sql = format!("DELETE FROM items WHERE id IN ({placeholders})");
    conn.execute(&sql, params_from_iter(args.iter().map(|b| b.as_ref())))?;

    // A blob is shared by every item with the same content hash; only remove it
    // once nothing references it any more.
    let mut orphans = Vec::new();
    for key in candidates {
        let still_used: i64 = conn.query_row(
            "SELECT COUNT(*) FROM items WHERE blob = ?1",
            params![key],
            |r| r.get(0),
        )?;
        if still_used == 0 {
            orphans.push(key);
        }
    }
    Ok(orphans)
}

/// Empty the trash entirely.
pub fn empty_trash(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM items WHERE deleted_at IS NOT NULL")?;
    let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    purge(conn, &ids)
}

/// Delete every unpinned, unfavourited live item.
pub fn clear_history(conn: &Connection, keep_pinned: bool, now: i64) -> Result<usize> {
    let sql = if keep_pinned {
        "UPDATE items SET deleted_at = ?1 WHERE deleted_at IS NULL AND pinned = 0 AND favorite = 0"
    } else {
        "UPDATE items SET deleted_at = ?1 WHERE deleted_at IS NULL"
    };
    Ok(conn.execute(sql, params![now])?)
}

// ---------------------------------------------------------------------------
// Query compilation
// ---------------------------------------------------------------------------

struct Compiled {
    sql: String,
    args: Vec<Box<dyn ToSql>>,
}

/// Column used for keyset pagination under each sort mode.
fn sort_column(sort: SortBy) -> &'static str {
    match sort {
        SortBy::Recent => "i.updated_at",
        SortBy::Created => "i.created_at",
        SortBy::Frequency => "i.use_count",
        SortBy::Size => "i.bytes",
        // Relevance paginates on rank via a bm25 expression; see compile().
        SortBy::Relevance => "i.updated_at",
    }
}

fn compile(q: &Query, cols: &str, for_count: bool) -> Result<Compiled> {
    let sort = q.effective_sort();
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();
    let mut wheres: Vec<String> = Vec::new();
    let mut joins = String::new();

    // --- full-text ---------------------------------------------------------
    let searching = if q.is_searching() {
        match search::build(&q.text) {
            Some(expr) => {
                joins.push_str(" JOIN items_fts f ON f.rowid = i.id");
                wheres.push("items_fts MATCH ?".into());
                args.push(Box::new(expr));
                true
            }
            None => false,
        }
    } else {
        false
    };

    // --- smart filter ------------------------------------------------------
    match q.filter {
        SmartFilter::Trash => wheres.push("i.deleted_at IS NOT NULL".into()),
        _ => wheres.push("i.deleted_at IS NULL".into()),
    }
    match q.filter {
        SmartFilter::All | SmartFilter::Trash => {}
        SmartFilter::Pinned => wheres.push("i.pinned = 1".into()),
        SmartFilter::Favorites => wheres.push("i.favorite = 1".into()),
        SmartFilter::Vault => wheres.push("i.sensitive = 1".into()),
        SmartFilter::Frequent => wheres.push("i.use_count > 1".into()),
        SmartFilter::Unused => wheres.push("i.use_count = 0".into()),
        SmartFilter::Large => wheres.push("i.bytes > 1048576".into()),
        SmartFilter::Today => {
            wheres.push("i.updated_at >= ?".into());
            args.push(Box::new(crate::util::now_ms() - 86_400_000));
        }
    }

    // --- kinds -------------------------------------------------------------
    if !q.kinds.is_empty() {
        let ph = placeholders(q.kinds.len());
        wheres.push(format!("i.kind IN ({ph})"));
        for k in &q.kinds {
            args.push(Box::new(k.as_str().to_string()));
        }
    }

    // --- source app --------------------------------------------------------
    if let Some(app) = &q.source_app {
        wheres.push("i.source_app = ?".into());
        args.push(Box::new(app.clone()));
    }

    // --- time range --------------------------------------------------------
    if let Some(after) = q.after {
        wheres.push("i.updated_at >= ?".into());
        args.push(Box::new(after));
    }
    if let Some(before) = q.before_time {
        wheres.push("i.updated_at <= ?".into());
        args.push(Box::new(before));
    }

    // --- tags (AND semantics: an item must carry every selected tag) --------
    if !q.tags.is_empty() {
        let ph = placeholders(q.tags.len());
        wheres.push(format!(
            "(SELECT COUNT(DISTINCT tag_id) FROM item_tags WHERE item_id = i.id AND tag_id IN ({ph})) = ?"
        ));
        for t in &q.tags {
            args.push(Box::new(*t));
        }
        args.push(Box::new(q.tags.len() as i64));
    }

    // --- collection --------------------------------------------------------
    if let Some(cid) = q.collection {
        joins.push_str(" JOIN item_collections ic ON ic.item_id = i.id AND ic.collection_id = ?");
        args.push(Box::new(cid));
    }

    let where_clause = if wheres.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", wheres.join(" AND "))
    };

    if for_count {
        return Ok(Compiled {
            sql: format!("SELECT COUNT(*) FROM items i{joins}{where_clause}"),
            args,
        });
    }

    // --- ordering + pagination --------------------------------------------
    // Pinned items float to the top of every view except the explicit trash and
    // pinned views, where that would be meaningless.
    let pin_first = !matches!(q.filter, SmartFilter::Trash | SmartFilter::Pinned) && !searching;
    let limit = q.effective_limit();
    let mut where_clause = where_clause;

    // Relevance ordering paginates with OFFSET rather than a keyset cursor.
    //
    // A keyset needs the sort key of the last row, and bm25's rank is computed
    // per query — it is not a column we can carry back and compare against on
    // the next request. OFFSET is the right trade-off here precisely because a
    // relevance page is already bounded by the FTS match: we are stepping
    // through the handful of rows that matched, not through the whole history.
    // The timeline, which really is unbounded, keeps its keyset cursor below.
    let (order, suffix) = if sort == SortBy::Relevance && searching {
        let offset = q.cursor.map(|c| c.key.max(0)).unwrap_or(0);
        (
            "ORDER BY bm25(items_fts) ASC, i.id DESC".to_string(),
            format!("LIMIT {limit} OFFSET {offset}"),
        )
    } else {
        let col = sort_column(sort);
        let pin = if pin_first { "i.pinned DESC, " } else { "" };

        if let Some(cur) = q.cursor {
            if where_clause.is_empty() {
                where_clause = " WHERE 1=1".into();
            }
            where_clause.push_str(&format!(" AND ({col} < ? OR ({col} = ? AND i.id < ?))"));
            args.push(Box::new(cur.key));
            args.push(Box::new(cur.key));
            args.push(Box::new(cur.id));
        }

        (format!("ORDER BY {pin}{col} DESC, i.id DESC"), format!("LIMIT {limit}"))
    };

    let sql = format!("SELECT {cols} FROM items i{joins}{where_clause} {order} {suffix}");

    Ok(Compiled { sql, args })
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// Run a list query and return one page plus its next cursor.
pub fn list(conn: &Connection, q: &Query) -> Result<Page<Item>> {
    let compiled = compile(q, LIST_COLS, false)?;
    let mut stmt = conn.prepare_cached(&compiled.sql)?;
    let items: Vec<Item> = stmt
        .query_map(
            params_from_iter(compiled.args.iter().map(|b| b.as_ref())),
            map_row,
        )?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let sort = q.effective_sort();
    let limit = q.effective_limit() as usize;

    // Only emit a cursor when the page was full; a short page is the end.
    let next = if items.len() == limit {
        if sort == SortBy::Relevance && q.is_searching() {
            // Relevance pages by OFFSET, so the cursor carries a row count
            // rather than a sort key. `id` is unused in this mode.
            let consumed = q.cursor.map(|c| c.key.max(0)).unwrap_or(0) + items.len() as i64;
            Some(Cursor { key: consumed, id: 0 })
        } else {
            items.last().map(|last| Cursor {
                key: match sort {
                    SortBy::Recent | SortBy::Relevance => last.updated_at,
                    SortBy::Created => last.created_at,
                    SortBy::Frequency => last.use_count,
                    SortBy::Size => last.bytes,
                },
                id: last.id,
            })
        }
    } else {
        None
    };

    // Counting is a full scan, so it is only worth doing for the first page.
    let total = if q.cursor.is_none() {
        let counted = compile(q, "", true)?;
        let mut stmt = conn.prepare_cached(&counted.sql)?;
        let n: i64 = stmt.query_row(
            params_from_iter(counted.args.iter().map(|b| b.as_ref())),
            |r| r.get(0),
        )?;
        Some(n)
    } else {
        None
    };

    let mut items = items;
    attach_tags(conn, &mut items)?;

    Ok(Page { items, next, total })
}

/// Bulk-load tags for a page in one query rather than N.
fn attach_tags(conn: &Connection, items: &mut [Item]) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let ph = placeholders(items.len());
    let sql = format!(
        "SELECT it.item_id, t.id, t.name, t.color
           FROM item_tags it JOIN tags t ON t.id = it.tag_id
          WHERE it.item_id IN ({ph})"
    );
    let args: Vec<Box<dyn ToSql>> = items.iter().map(|i| Box::new(i.id) as Box<dyn ToSql>).collect();

    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Tag { id: r.get(1)?, name: r.get(2)?, color: r.get(3)?, count: 0 },
        ))
    })?;

    let mut by_item: std::collections::HashMap<i64, Vec<Tag>> = std::collections::HashMap::new();
    for row in rows {
        let (item_id, tag) = row?;
        by_item.entry(item_id).or_default().push(tag);
    }
    for item in items.iter_mut() {
        if let Some(tags) = by_item.remove(&item.id) {
            item.tags = tags;
        }
    }
    Ok(())
}

fn tags_for(conn: &Connection, item_id: i64) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.id, t.name, t.color FROM item_tags it
           JOIN tags t ON t.id = it.tag_id
          WHERE it.item_id = ?1 ORDER BY t.name",
    )?;
    let tags = stmt
        .query_map(params![item_id], |r| {
            Ok(Tag { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, count: 0 })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(tags)
}

/// Sidebar counters. One pass per counter, all cheap thanks to partial indexes.
pub fn stats(conn: &Connection) -> Result<Stats> {
    let day_ago = crate::util::now_ms() - 86_400_000;

    let one = |sql: &str, args: &[&dyn ToSql]| -> Result<i64> {
        Ok(conn.query_row(sql, args, |r| r.get(0))?)
    };

    let mut stats = Stats {
        total: one("SELECT COUNT(*) FROM items WHERE deleted_at IS NULL", &[])?,
        pinned: one("SELECT COUNT(*) FROM items WHERE pinned = 1 AND deleted_at IS NULL", &[])?,
        favorites: one("SELECT COUNT(*) FROM items WHERE favorite = 1 AND deleted_at IS NULL", &[])?,
        today: one(
            "SELECT COUNT(*) FROM items WHERE updated_at >= ?1 AND deleted_at IS NULL",
            &[&day_ago],
        )?,
        vault: one("SELECT COUNT(*) FROM items WHERE sensitive = 1 AND deleted_at IS NULL", &[])?,
        trash: one("SELECT COUNT(*) FROM items WHERE deleted_at IS NOT NULL", &[])?,
        bytes: one("SELECT COALESCE(SUM(bytes), 0) FROM items WHERE deleted_at IS NULL", &[])?,
        ..Default::default()
    };

    let mut stmt = conn.prepare(
        "SELECT kind, COUNT(*) FROM items WHERE deleted_at IS NULL GROUP BY kind ORDER BY 2 DESC",
    )?;
    stats.by_kind = stmt
        .query_map([], |r| {
            let k: String = r.get(0)?;
            Ok(KindCount { kind: Kind::parse(&k).unwrap_or(Kind::Text), count: r.get(1)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let mut stmt = conn.prepare(
        "SELECT source_app, COUNT(*) FROM items
          WHERE deleted_at IS NULL AND source_app IS NOT NULL
          GROUP BY source_app ORDER BY 2 DESC LIMIT 12",
    )?;
    stats.by_app = stmt
        .query_map([], |r| Ok(AppCount { app: r.get(0)?, count: r.get(1)? }))?
        .collect::<rusqlite::Result<_>>()?;

    Ok(stats)
}

/// Ids whose blobs are referenced, used by the blob-store garbage collector.
pub fn referenced_blobs(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT blob FROM items WHERE blob IS NOT NULL")?;
    let keys = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(keys)
}

/// Items eligible for automatic cleanup: older than `cutoff`, not pinned, not
/// favourited, not tagged and not in any collection.
pub fn expired(conn: &Connection, cutoff: i64, limit: u32) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT i.id FROM items i
          WHERE i.deleted_at IS NULL
            AND i.pinned = 0 AND i.favorite = 0
            AND i.updated_at < ?1
            AND NOT EXISTS (SELECT 1 FROM item_tags        WHERE item_id = i.id)
            AND NOT EXISTS (SELECT 1 FROM item_collections WHERE item_id = i.id)
          ORDER BY i.updated_at ASC
          LIMIT ?2",
    )?;
    let ids = stmt
        .query_map(params![cutoff, limit], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// Oldest ids beyond a maximum row count, for the "keep at most N" policy.
pub fn overflow(conn: &Connection, max_items: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id FROM items
          WHERE deleted_at IS NULL AND pinned = 0 AND favorite = 0
          ORDER BY updated_at DESC
          LIMIT -1 OFFSET ?1",
    )?;
    let ids = stmt
        .query_map(params![max_items], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

/// Trash entries older than `cutoff`, ready for permanent removal.
pub fn stale_trash(conn: &Connection, cutoff: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id FROM items WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
    )?;
    let ids = stmt
        .query_map(params![cutoff], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

fn placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::item::{Kind, Meta};
    use crate::infra::db;

    fn sample(hash: &str, kind: Kind, preview: &str) -> NewItem {
        NewItem {
            kind,
            preview: preview.into(),
            body: Some(preview.into()),
            blob: None,
            bytes: preview.len() as i64,
            meta: Meta::default(),
            hash: hash.into(),
            source_app: Some("test.exe".into()),
            source_title: None,
            sensitive: false,
        }
    }

    #[test]
    fn insert_then_dedupe() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();

        let a = insert(&conn, &sample("h1", Kind::Text, "hello"), None, 1_000).unwrap();
        assert!(a.is_new());

        let b = insert(&conn, &sample("h1", Kind::Text, "hello"), None, 2_000).unwrap();
        assert!(!b.is_new());
        assert_eq!(a.id(), b.id());

        let item = get(&conn, a.id()).unwrap();
        assert_eq!(item.updated_at, 2_000);
        assert_eq!(item.use_count, 1);
    }

    #[test]
    fn list_paginates_without_gaps() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        for i in 0..25 {
            insert(&conn, &sample(&format!("h{i}"), Kind::Text, &format!("item {i}")), None, 1_000 + i).unwrap();
        }

        let mut q = Query { limit: 10, ..Default::default() };
        let mut seen = Vec::new();
        loop {
            let page = list(&conn, &q).unwrap();
            seen.extend(page.items.iter().map(|i| i.id));
            match page.next {
                Some(c) => q.cursor = Some(c),
                None => break,
            }
        }

        assert_eq!(seen.len(), 25);
        let unique: std::collections::HashSet<_> = seen.iter().collect();
        assert_eq!(unique.len(), 25, "pagination repeated a row");
    }

    #[test]
    fn search_finds_items() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        insert(&conn, &sample("h1", Kind::Text, "the quick brown fox"), None, 1).unwrap();
        insert(&conn, &sample("h2", Kind::Text, "lazy dog sleeping"), None, 2).unwrap();

        let q = Query { text: "quick".into(), ..Default::default() };
        let page = list(&conn, &q).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].preview.contains("quick"));
    }

    #[test]
    fn relevance_search_paginates_past_the_first_page() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        for i in 0..25 {
            insert(
                &conn,
                &sample(&format!("h{i}"), Kind::Text, &format!("widget number {i}")),
                None,
                1_000 + i,
            )
            .unwrap();
        }

        let mut q = Query {
            text: "widget".into(),
            sort: SortBy::Relevance,
            limit: 10,
            ..Default::default()
        };

        let first = list(&conn, &q).unwrap();
        assert_eq!(first.items.len(), 10);
        let cursor = first.next.expect("a full page must offer a cursor");

        q.cursor = Some(cursor);
        let second = list(&conn, &q).unwrap();
        assert_eq!(second.items.len(), 10, "the second relevance page came back empty");

        // The two pages must not overlap.
        let firsts: std::collections::HashSet<i64> = first.items.iter().map(|i| i.id).collect();
        for item in &second.items {
            assert!(!firsts.contains(&item.id), "row {} appeared on both pages", item.id);
        }

        // And the walk must terminate having seen everything exactly once.
        let mut seen: std::collections::HashSet<i64> = firsts;
        seen.extend(second.items.iter().map(|i| i.id));
        let mut next = second.next;
        while let Some(cursor) = next {
            q.cursor = Some(cursor);
            let page = list(&conn, &q).unwrap();
            for item in &page.items {
                assert!(seen.insert(item.id), "row {} was returned twice", item.id);
            }
            next = page.next;
        }
        assert_eq!(seen.len(), 25);
    }

    #[test]
    fn search_survives_operator_characters() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        insert(&conn, &sample("h1", Kind::Text, "hello world"), None, 1).unwrap();

        for text in ["(", "\"", "a AND", "*", "NEAR(", "foo:bar"] {
            let q = Query { text: text.into(), ..Default::default() };
            list(&conn, &q).unwrap_or_else(|e| panic!("query {text:?} failed: {e}"));
        }
    }

    #[test]
    fn encrypted_body_is_not_searchable() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let mut item = sample("h1", Kind::Secret, "abcd•••••••• (36 chars)");
        item.body = Some("ghp_supersecrettoken".into());
        item.sensitive = true;
        insert(&conn, &item, Some(b"ciphertext"), 1).unwrap();

        let q = Query { text: "supersecrettoken".into(), ..Default::default() };
        let page = list(&conn, &q).unwrap();
        assert!(page.items.is_empty(), "secret plaintext leaked into the FTS index");
    }

    #[test]
    fn kind_and_filter_combine() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let a = insert(&conn, &sample("h1", Kind::Link, "https://a.dev"), None, 1).unwrap().id();
        insert(&conn, &sample("h2", Kind::Text, "plain"), None, 2).unwrap();
        set_pinned(&conn, a, true).unwrap();

        let q = Query {
            kinds: vec![Kind::Link],
            filter: SmartFilter::Pinned,
            ..Default::default()
        };
        let page = list(&conn, &q).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, a);
    }

    #[test]
    fn soft_delete_then_restore() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let id = insert(&conn, &sample("h1", Kind::Text, "x"), None, 1).unwrap().id();

        soft_delete(&conn, &[id], 5).unwrap();
        let live = list(&conn, &Query::default()).unwrap();
        assert!(live.items.is_empty());

        let trash = list(&conn, &Query { filter: SmartFilter::Trash, ..Default::default() }).unwrap();
        assert_eq!(trash.items.len(), 1);

        restore(&conn, &[id]).unwrap();
        assert_eq!(list(&conn, &Query::default()).unwrap().items.len(), 1);
    }

    #[test]
    fn purge_reports_orphan_blobs() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let mut item = sample("h1", Kind::Image, "Image 10×10");
        item.body = None;
        item.blob = Some("abc123".into());
        let id = insert(&conn, &item, None, 1).unwrap().id();

        let orphans = purge(&conn, &[id]).unwrap();
        assert_eq!(orphans, vec!["abc123".to_string()]);
    }

    #[test]
    fn stats_are_consistent() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let a = insert(&conn, &sample("h1", Kind::Text, "a"), None, 1).unwrap().id();
        insert(&conn, &sample("h2", Kind::Link, "https://b.dev"), None, 2).unwrap();
        set_pinned(&conn, a, true).unwrap();

        let s = stats(&conn).unwrap();
        assert_eq!(s.total, 2);
        assert_eq!(s.pinned, 1);
        assert_eq!(s.by_kind.len(), 2);
    }
}
