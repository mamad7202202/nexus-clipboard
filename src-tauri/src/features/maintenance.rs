//! Background housekeeping: retention, trash expiry, blob garbage collection
//! and index optimisation.
//!
//! All of it runs on a timer off the UI thread and is written to be
//! interruptible — each pass does a bounded amount of work, so a history with
//! ten million rows never produces a multi-minute stall.

use std::sync::Arc;
use std::time::Duration;

use crate::app::state::AppState;
use crate::error::Result;
use crate::infra::db::{self, items};
use crate::util;

/// Maximum rows a single retention pass will trash. Bounded so the pass stays
/// short even on a badly overgrown history; the next tick picks up the rest.
const BATCH: u32 = 5_000;

/// How often housekeeping runs.
const INTERVAL: Duration = Duration::from_secs(15 * 60);

/// A short delay before the first pass so startup stays snappy.
const STARTUP_DELAY: Duration = Duration::from_secs(45);

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct MaintenanceReport {
    /// Items moved to the trash because they aged out.
    pub expired: usize,
    /// Items trashed because the history exceeded `max_items`.
    pub overflowed: usize,
    /// Trash entries permanently removed.
    pub purged: usize,
    /// Blob files deleted.
    pub blobs_removed: usize,
    /// Bytes reclaimed from the blob store.
    pub bytes_freed: u64,
}

/// Run one housekeeping pass.
pub fn run_once(state: &AppState) -> Result<MaintenanceReport> {
    let settings = state.settings();
    let now = util::now_ms();
    let mut report = MaintenanceReport::default();

    {
        let conn = state.conn()?;

        // 1. Age-based retention.
        if let Some(cutoff) = settings.retention_cutoff(now) {
            let ids = items::expired(&conn, cutoff, BATCH)?;
            report.expired = items::soft_delete(&conn, &ids, now)?;
        }

        // 2. Count-based cap.
        if settings.max_items > 0 {
            let ids = items::overflow(&conn, settings.max_items as i64)?;
            let ids: Vec<i64> = ids.into_iter().take(BATCH as usize).collect();
            report.overflowed = items::soft_delete(&conn, &ids, now)?;
        }

        // 3. Permanently remove trash past its grace period.
        let stale = items::stale_trash(&conn, settings.trash_cutoff(now))?;
        let stale: Vec<i64> = stale.into_iter().take(BATCH as usize).collect();
        if !stale.is_empty() {
            report.purged = stale.len();
            let orphans = items::purge(&conn, &stale)?;
            for key in &orphans {
                // A failed blob delete is not fatal; the sweep below catches it.
                let _ = state.blobs.remove(key);
            }
        }

        // 4. Sweep blobs no row references any more. Cheap because the DB side
        //    is a single indexed scan.
        let referenced = items::referenced_blobs(&conn)?;
        let (removed, freed) = state.blobs.gc(&referenced)?;
        report.blobs_removed = removed;
        report.bytes_freed = freed;

        // 5. Let SQLite refresh its planner statistics.
        db::optimize(&conn)?;
    }

    if report.expired + report.overflowed + report.purged + report.blobs_removed > 0 {
        tracing::info!(?report, "maintenance pass complete");
    }

    Ok(report)
}

/// Spawn the recurring housekeeping task.
pub fn spawn(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            let state = state.clone();
            // Housekeeping is blocking (SQLite + file IO); keep it off the
            // async reactor.
            let result = tauri::async_runtime::spawn_blocking(move || run_once(&state)).await;
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!(?e, "maintenance pass failed"),
                Err(e) => tracing::warn!(?e, "maintenance task panicked"),
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

/// Compact the database. Exposed in settings; too expensive to run on a timer.
pub fn compact(state: &AppState) -> Result<i64> {
    let conn = state.conn()?;
    db::vacuum(&conn)?;
    db::size_bytes(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Retention;
    use crate::domain::item::{Kind, Meta, NewItem};

    fn temp_state() -> (Arc<AppState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("nexus-maint-{}", uuid::Uuid::new_v4()));
        (AppState::new(dir.clone()).unwrap(), dir)
    }

    fn item(hash: &str) -> NewItem {
        NewItem {
            kind: Kind::Text,
            preview: "x".into(),
            body: Some("x".into()),
            blob: None,
            bytes: 1,
            meta: Meta::default(),
            hash: hash.into(),
            source_app: None,
            source_title: None,
            sensitive: false,
        }
    }

    #[test]
    fn retention_trashes_only_old_unpinned_items() {
        let (state, dir) = temp_state();
        let now = util::now_ms();
        let old = now - 10 * 86_400_000;

        {
            let conn = state.conn().unwrap();
            let stale = items::insert(&conn, &item("old"), None, old).unwrap().id();
            let pinned = items::insert(&conn, &item("pinned"), None, old).unwrap().id();
            items::set_pinned(&conn, pinned, true).unwrap();
            items::insert(&conn, &item("fresh"), None, now).unwrap();
            assert!(stale > 0);
        }

        state.update_settings(|s| s.retention = Retention::Days(3)).unwrap();
        let report = run_once(&state).unwrap();
        assert_eq!(report.expired, 1, "only the old unpinned item should expire");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn max_items_trims_the_oldest() {
        let (state, dir) = temp_state();
        {
            let conn = state.conn().unwrap();
            for i in 0..10 {
                items::insert(&conn, &item(&format!("h{i}")), None, 1_000 + i).unwrap();
            }
        }

        state.update_settings(|s| s.max_items = 4).unwrap();
        let report = run_once(&state).unwrap();
        assert_eq!(report.overflowed, 6);

        let conn = state.conn().unwrap();
        let stats = items::stats(&conn).unwrap();
        assert_eq!(stats.total, 4);

        drop(conn);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn orphan_blobs_are_collected() {
        let (state, dir) = temp_state();
        let key = state.blobs.put(b"unreferenced payload").unwrap();
        assert!(state.blobs.exists(&key));

        let report = run_once(&state).unwrap();
        assert_eq!(report.blobs_removed, 1);
        assert!(!state.blobs.exists(&key));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pass_is_safe_on_an_empty_history() {
        let (state, dir) = temp_state();
        let report = run_once(&state).unwrap();
        assert_eq!(report.expired, 0);
        std::fs::remove_dir_all(dir).ok();
    }
}
