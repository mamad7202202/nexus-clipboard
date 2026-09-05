//! SQLite access layer: connection pool, pragmas and repositories.

pub mod items;
pub mod meta;
pub mod migrations;
pub mod search;

use std::path::Path;
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use crate::error::Result;

pub type Db = Pool<SqliteConnectionManager>;
pub type Conn = r2d2::PooledConnection<SqliteConnectionManager>;

/// Open (creating if needed) the history database and bring it up to date.
///
/// WAL plus a generous page cache is what keeps reads fast while the capture
/// thread writes concurrently — readers never block on the writer.
pub fn open(path: &Path) -> Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let manager = SqliteConnectionManager::file(path).with_init(|conn| {
        configure(conn)?;
        Ok(())
    });

    let pool = Pool::builder()
        // One writer plus a handful of readers; SQLite serialises writes anyway.
        .max_size(8)
        .min_idle(Some(1))
        .connection_timeout(Duration::from_secs(10))
        .build(manager)
        .map_err(crate::error::Error::Pool)?;

    let mut conn = pool.get()?;
    migrations::run(&mut conn)?;
    drop(conn);

    Ok(pool)
}

/// An in-memory database, used by tests and by the import dry-run path.
#[allow(dead_code)]
pub fn open_memory() -> Result<Db> {
    let manager = SqliteConnectionManager::memory().with_init(|conn| {
        configure(conn)?;
        Ok(())
    });
    let pool = Pool::builder().max_size(1).build(manager).map_err(crate::error::Error::Pool)?;
    let mut conn = pool.get()?;
    migrations::run(&mut conn)?;
    drop(conn);
    Ok(pool)
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL is the right trade-off under WAL: durable across app crashes,
    // only at risk on a hard power loss, and dramatically faster than FULL.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // Negative value = KiB of page cache rather than a page count. 64 MB.
    conn.pragma_update(None, "cache_size", -64_000)?;
    // Memory-map up to 256 MB of the database for read-heavy list queries.
    conn.pragma_update(None, "mmap_size", 268_435_456i64)?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    // Keep the WAL from growing unbounded during heavy capture bursts.
    conn.pragma_update(None, "wal_autocheckpoint", 1_000)?;

    // List queries are assembled per filter combination, so the default cache
    // of 16 statements thrashes as soon as the user starts toggling kinds and
    // tags. 64 covers the realistic combinations without meaningful memory cost.
    conn.set_prepared_statement_cache_capacity(64);

    Ok(())
}

/// Reclaim space and refresh the query planner's statistics. Called by the
/// maintenance task, never on the hot path.
pub fn optimize(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

/// Full rebuild. Expensive; only invoked explicitly from settings.
pub fn vacuum(conn: &Connection) -> Result<()> {
    conn.execute_batch("VACUUM; INSERT INTO items_fts(items_fts) VALUES ('optimize');")?;
    Ok(())
}

/// Current on-disk size in bytes.
pub fn size_bytes(conn: &Connection) -> Result<i64> {
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    Ok(page_count * page_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_cleanly() {
        let db = open_memory().expect("open");
        let conn = db.get().expect("conn");
        let v: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, migrations::target_version());
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = open_memory().unwrap();
        let mut conn = db.get().unwrap();
        migrations::run(&mut conn).expect("second run is a no-op");
    }
}
