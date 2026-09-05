//! Application state: the single object every command and background task
//! borrows from.
//!
//! Locking policy — settings and the ignore list are read on every clipboard
//! event and written rarely, so both sit behind an `RwLock`. The vault key uses
//! a `Mutex` because it is taken rarely and cloning it is cheap. Nothing here
//! holds a lock across an `await` or a database call.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::config::Settings;
use crate::error::{Error, Result};
use crate::infra::blob::BlobStore;
use crate::infra::clipboard::WatcherHandle;
use crate::infra::crypto::VaultKey;
use crate::infra::db::{self, meta, Conn, Db};

pub struct AppState {
    /// SQLite connection pool.
    pub db: Db,
    /// Content-addressed store for images and oversized text.
    pub blobs: BlobStore,
    /// Root directory holding the database, blobs and backups.
    pub data_dir: PathBuf,

    settings: RwLock<Settings>,
    /// Lower-cased executable names we never capture from.
    ignored_apps: RwLock<Vec<String>>,
    vault: Mutex<Option<VaultKey>>,
    watcher: Mutex<Option<WatcherHandle>>,
    /// Handle of the window that had focus before the launcher appeared, so a
    /// paste can be delivered back to it.
    last_focus: Mutex<isize>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&data_dir)?;

        let db = db::open(&data_dir.join("history.db"))?;
        let blobs = BlobStore::new(data_dir.join("blobs"))?;

        let (settings, ignored) = {
            let conn = db.get()?;
            let settings = Settings::load(&conn);
            let ignored = meta::list_ignored_apps(&conn)?;
            (settings, ignored)
        };

        let state = Arc::new(Self {
            db,
            blobs,
            data_dir,
            settings: RwLock::new(settings),
            ignored_apps: RwLock::new(normalize_apps(ignored)),
            vault: Mutex::new(None),
            watcher: Mutex::new(None),
            last_focus: Mutex::new(0),
        });

        state.seed_default_ignores()?;

        // Opening the vault belongs to state construction, not to the Tauri
        // lifecycle: "a live AppState can encrypt secrets" is an invariant the
        // capture pipeline relies on, and tests must be able to depend on it
        // without booting a window.
        if let Err(e) = state.open_vault() {
            tracing::warn!(?e, "vault unavailable; secrets will not be encrypted");
        }

        Ok(state)
    }

    /// Unlock the vault, creating a machine-keyed one on first run.
    ///
    /// A passphrase-protected vault stays locked until the user unlocks it
    /// explicitly, which is the whole point of setting one.
    fn open_vault(&self) -> Result<()> {
        let conn = self.conn()?;

        match crate::config::load_vault(&conn)? {
            Some(cfg) if !cfg.passphrase => {
                let key = crate::infra::crypto::unlock(&cfg, None)?;
                self.set_vault_key(Some(key));
            }
            Some(_) => {}
            None => {
                let (cfg, key) = crate::infra::crypto::init(None)?;
                crate::config::save_vault(&conn, &cfg)?;
                self.set_vault_key(Some(key));
            }
        }

        Ok(())
    }

    // --- database ---------------------------------------------------------

    pub fn conn(&self) -> Result<Conn> {
        self.db.get().map_err(Error::Pool)
    }

    // --- settings ---------------------------------------------------------

    /// A snapshot of the current settings. Cloning avoids holding the lock
    /// while the caller does real work.
    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn save_settings(&self, next: Settings) -> Result<()> {
        {
            let conn = self.conn()?;
            next.save(&conn)?;
        }
        *self.settings.write() = next;
        Ok(())
    }

    /// Mutate settings in place and persist the result.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut next = self.settings();
        f(&mut next);
        self.save_settings(next.clone())?;
        Ok(next)
    }

    // --- ignore list ------------------------------------------------------

    /// Applications whose clipboard writes we never record. Password managers
    /// ship this behaviour by default and users expect it.
    fn seed_default_ignores(&self) -> Result<()> {
        const DEFAULTS: &[&str] = &[
            "1Password.exe",
            "KeePass.exe",
            "KeePassXC.exe",
            "Bitwarden.exe",
            "LastPass.exe",
            "Dashlane.exe",
            "NordPass.exe",
            "ProtonPass.exe",
            "enpass.exe",
        ];

        let conn = self.conn()?;
        let existing = meta::list_ignored_apps(&conn)?;
        // Only seed on a fresh install: a user who cleared the list should keep
        // it cleared.
        if !existing.is_empty() {
            return Ok(());
        }
        for app in DEFAULTS {
            meta::add_ignored_app(&conn, app)?;
        }
        *self.ignored_apps.write() = normalize_apps(DEFAULTS.iter().map(|s| s.to_string()).collect());
        Ok(())
    }

    pub fn is_ignored_app(&self, app: &str) -> bool {
        let needle = app.to_lowercase();
        self.ignored_apps.read().iter().any(|a| a == &needle)
    }

    pub fn reload_ignored_apps(&self) -> Result<Vec<String>> {
        let conn = self.conn()?;
        let apps = meta::list_ignored_apps(&conn)?;
        *self.ignored_apps.write() = normalize_apps(apps.clone());
        Ok(apps)
    }

    // --- vault ------------------------------------------------------------

    pub fn vault_key(&self) -> Option<VaultKey> {
        self.vault.lock().clone()
    }

    pub fn set_vault_key(&self, key: Option<VaultKey>) {
        *self.vault.lock() = key;
    }

    pub fn is_vault_unlocked(&self) -> bool {
        self.vault.lock().is_some()
    }

    /// Decrypt a stored payload, or explain why it cannot be read.
    pub fn decrypt(&self, cipher: &[u8]) -> Result<String> {
        let key = self.vault_key().ok_or(Error::VaultLocked)?;
        crate::infra::crypto::decrypt_str(&key, cipher)
    }

    // --- watcher ----------------------------------------------------------

    pub fn set_watcher(&self, handle: WatcherHandle) {
        *self.watcher.lock() = Some(handle);
    }

    pub fn watcher(&self) -> Option<WatcherHandle> {
        self.watcher.lock().clone()
    }

    /// Tell the watcher that the next clipboard change is our own write.
    pub fn expect_self_write(&self) {
        if let Some(w) = self.watcher() {
            w.expect_self_write();
        }
    }

    // --- focus ------------------------------------------------------------

    /// Remember which window to paste into. Called just before showing the
    /// launcher, while the target still has focus.
    pub fn remember_focus(&self, hwnd: isize) {
        *self.last_focus.lock() = hwnd;
    }

    pub fn last_focus(&self) -> isize {
        *self.last_focus.lock()
    }
}

fn normalize_apps(apps: Vec<String>) -> Vec<String> {
    apps.into_iter().map(|a| a.trim().to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state() -> (Arc<AppState>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nexus-state-{}", uuid::Uuid::new_v4()));
        let state = AppState::new(dir.clone()).unwrap();
        (state, dir)
    }

    #[test]
    fn seeds_password_managers_on_first_run() {
        let (state, dir) = temp_state();
        assert!(state.is_ignored_app("1Password.exe"));
        assert!(state.is_ignored_app("bitwarden.exe"), "matching must be case-insensitive");
        assert!(!state.is_ignored_app("notepad.exe"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn settings_persist_across_reload() {
        let (state, dir) = temp_state();
        state.update_settings(|s| s.accent = "#123456".into()).unwrap();
        drop(state);

        let reopened = AppState::new(dir.clone()).unwrap();
        assert_eq!(reopened.settings().accent, "#123456");
        drop(reopened);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn vault_opens_on_a_fresh_install() {
        // With no passphrase configured the vault is machine-keyed, so secret
        // capture works from the very first copy.
        let (state, dir) = temp_state();
        assert!(state.is_vault_unlocked());

        let key = state.vault_key().unwrap();
        let blob = crate::infra::crypto::encrypt_str(&key, "token").unwrap();
        assert_eq!(state.decrypt(&blob).unwrap(), "token");

        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn decrypting_garbage_fails_without_panicking() {
        let (state, dir) = temp_state();
        assert!(state.decrypt(b"not a real ciphertext").is_err());
        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn locking_makes_secrets_unreadable() {
        let (state, dir) = temp_state();
        let key = state.vault_key().unwrap();
        let blob = crate::infra::crypto::encrypt_str(&key, "token").unwrap();

        state.set_vault_key(None);
        assert!(matches!(state.decrypt(&blob), Err(Error::VaultLocked)));

        drop(state);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn focus_is_remembered() {
        let (state, dir) = temp_state();
        state.remember_focus(4242);
        assert_eq!(state.last_focus(), 4242);
        std::fs::remove_dir_all(dir).ok();
    }
}
