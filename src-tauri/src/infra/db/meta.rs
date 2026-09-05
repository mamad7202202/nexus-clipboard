//! Repositories for everything around items: tags, collections, settings and
//! the ignore-list of applications we refuse to capture from.

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};

use crate::domain::{Collection, Tag};
use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

pub fn list_tags(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.color,
                (SELECT COUNT(*) FROM item_tags it
                   JOIN items i ON i.id = it.item_id
                  WHERE it.tag_id = t.id AND i.deleted_at IS NULL) AS cnt
           FROM tags t ORDER BY cnt DESC, t.name",
    )?;
    let tags = stmt
        .query_map([], |r| {
            Ok(Tag { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, count: r.get(3)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(tags)
}

/// Create a tag, or return the existing one with that name. Tag names are
/// case-insensitively unique so "Work" and "work" never diverge.
pub fn upsert_tag(conn: &Connection, name: &str, color: &str, now: i64) -> Result<Tag> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("tag name cannot be empty"));
    }
    if name.chars().count() > 48 {
        return Err(Error::invalid("tag name is too long"));
    }

    conn.execute(
        "INSERT INTO tags (name, color, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET color = excluded.color",
        params![name, color, now],
    )?;

    let tag = conn.query_row(
        "SELECT id, name, color FROM tags WHERE name = ?1",
        params![name],
        |r| Ok(Tag { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, count: 0 }),
    )?;
    Ok(tag)
}

pub fn rename_tag(conn: &Connection, id: i64, name: &str, color: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("tag name cannot be empty"));
    }
    conn.execute(
        "UPDATE tags SET name = ?2, color = ?3 WHERE id = ?1",
        params![id, name, color],
    )?;
    Ok(())
}

pub fn delete_tag(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(())
}

/// Attach a tag to many items at once. Already-tagged items are left alone.
pub fn tag_items(conn: &Connection, item_ids: &[i64], tag_id: i64) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO item_tags (item_id, tag_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
    )?;
    for id in item_ids {
        stmt.execute(params![id, tag_id])?;
    }
    Ok(())
}

pub fn untag_items(conn: &Connection, item_ids: &[i64], tag_id: i64) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let ph = (0..item_ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM item_tags WHERE tag_id = ?1 AND item_id IN ({ph})");
    let mut args: Vec<Box<dyn ToSql>> = vec![Box::new(tag_id)];
    args.extend(item_ids.iter().map(|i| Box::new(*i) as Box<dyn ToSql>));
    conn.execute(&sql, params_from_iter(args.iter().map(|b| b.as_ref())))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

pub fn list_collections(conn: &Connection) -> Result<Vec<Collection>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.icon, c.color, c.sort,
                (SELECT COUNT(*) FROM item_collections ic
                   JOIN items i ON i.id = ic.item_id
                  WHERE ic.collection_id = c.id AND i.deleted_at IS NULL) AS cnt
           FROM collections c ORDER BY c.sort, c.name",
    )?;
    let out = stmt
        .query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                icon: r.get(2)?,
                color: r.get(3)?,
                sort: r.get(4)?,
                count: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(out)
}

pub fn create_collection(
    conn: &Connection,
    name: &str,
    icon: &str,
    color: &str,
    now: i64,
) -> Result<Collection> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("collection name cannot be empty"));
    }
    let next_sort: i64 = conn
        .query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM collections", [], |r| r.get(0))
        .unwrap_or(1);

    conn.execute(
        "INSERT INTO collections (name, icon, color, sort, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, icon, color, next_sort, now],
    )?;

    Ok(Collection {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        icon: icon.to_string(),
        color: color.to_string(),
        sort: next_sort,
        count: 0,
    })
}

pub fn update_collection(
    conn: &Connection,
    id: i64,
    name: &str,
    icon: &str,
    color: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE collections SET name = ?2, icon = ?3, color = ?4 WHERE id = ?1",
        params![id, name.trim(), icon, color],
    )?;
    Ok(())
}

pub fn delete_collection(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn add_to_collection(conn: &Connection, item_ids: &[i64], collection_id: i64) -> Result<()> {
    let mut position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM item_collections WHERE collection_id = ?1",
            params![collection_id],
            |r| r.get(0),
        )
        .unwrap_or(1);

    let mut stmt = conn.prepare_cached(
        "INSERT INTO item_collections (item_id, collection_id, position)
         VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
    )?;
    for id in item_ids {
        stmt.execute(params![id, collection_id, position])?;
        position += 1;
    }
    Ok(())
}

pub fn remove_from_collection(conn: &Connection, item_ids: &[i64], collection_id: i64) -> Result<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let ph = (0..item_ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM item_collections WHERE collection_id = ?1 AND item_id IN ({ph})");
    let mut args: Vec<Box<dyn ToSql>> = vec![Box::new(collection_id)];
    args.extend(item_ids.iter().map(|i| Box::new(*i) as Box<dyn ToSql>));
    conn.execute(&sql, params_from_iter(args.iter().map(|b| b.as_ref())))?;
    Ok(())
}

/// Persist a user-defined ordering inside a collection.
pub fn reorder_collection(conn: &Connection, collection_id: i64, ordered: &[i64]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "UPDATE item_collections SET position = ?3 WHERE collection_id = ?1 AND item_id = ?2",
    )?;
    for (pos, item_id) in ordered.iter().enumerate() {
        stmt.execute(params![collection_id, item_id, pos as i64])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Settings (JSON key/value)
// ---------------------------------------------------------------------------

pub fn get_setting_raw(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting_raw(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Ignored applications
// ---------------------------------------------------------------------------

pub fn list_ignored_apps(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM ignored_apps ORDER BY name")?;
    let out = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(out)
}

pub fn add_ignored_app(conn: &Connection, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("application name cannot be empty"));
    }
    conn.execute(
        "INSERT INTO ignored_apps (name) VALUES (?1) ON CONFLICT DO NOTHING",
        params![name],
    )?;
    Ok(())
}

pub fn remove_ignored_app(conn: &Connection, name: &str) -> Result<()> {
    conn.execute("DELETE FROM ignored_apps WHERE name = ?1", params![name])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db;

    #[test]
    fn tags_are_case_insensitively_unique() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let a = upsert_tag(&conn, "Work", "#fff", 1).unwrap();
        let b = upsert_tag(&conn, "work", "#000", 2).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(list_tags(&conn).unwrap().len(), 1);
    }

    #[test]
    fn empty_tag_name_is_rejected() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        assert!(upsert_tag(&conn, "   ", "#fff", 1).is_err());
    }

    #[test]
    fn collections_round_trip() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        let c = create_collection(&conn, "Snippets", "code", "#f00", 1).unwrap();
        update_collection(&conn, c.id, "Code Snippets", "code", "#0f0").unwrap();
        let all = list_collections(&conn).unwrap();
        assert_eq!(all[0].name, "Code Snippets");
        delete_collection(&conn, c.id).unwrap();
        assert!(list_collections(&conn).unwrap().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        assert!(get_setting_raw(&conn, "theme").unwrap().is_none());
        set_setting_raw(&conn, "theme", "\"dark\"").unwrap();
        set_setting_raw(&conn, "theme", "\"light\"").unwrap();
        assert_eq!(get_setting_raw(&conn, "theme").unwrap().unwrap(), "\"light\"");
    }
}
