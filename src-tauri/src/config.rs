//! User settings.
//!
//! Settings live in the `settings` table as one JSON blob under a single key.
//! That keeps reads to one row, makes adding fields a no-op (serde defaults
//! fill the gaps), and means a settings write is atomic.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::infra::db::{meta, Conn};

const SETTINGS_KEY: &str = "app";
const VAULT_KEY: &str = "vault";

/// How long history is kept before automatic cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    Forever,
    Days(u32),
}

impl Default for Retention {
    fn default() -> Self {
        Self::Forever
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Default for Theme {
    fn default() -> Self {
        Self::System
    }
}

/// Density of the history list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    Comfortable,
    Spacious,
}

impl Default for Density {
    fn default() -> Self {
        Self::Comfortable
    }
}

/// Which AI provider powers the smart features. `None` disables them entirely,
/// which is the default: nothing leaves the machine unless the user opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Disabled,
    Anthropic,
    Openai,
    Custom,
}

impl Default for AiProvider {
    fn default() -> Self {
        Self::Disabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: AiProvider,
    /// Stored encrypted in the vault, never in this struct when persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub model: String,
    /// Base URL for API endpoints (OpenAI-compatible, proxies, Ollama, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Generate a one-line summary for long captures automatically.
    pub auto_summarize: bool,
    /// Only summarise items longer than this many characters.
    pub summarize_threshold: usize,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: AiProvider::Disabled,
            api_key: None,
            model: "claude-sonnet-5".into(),
            base_url: None,
            auto_summarize: false,
            summarize_threshold: 800,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // --- capture ---------------------------------------------------------
    /// Master switch for clipboard monitoring.
    pub capture_enabled: bool,
    /// Capture images as well as text.
    pub capture_images: bool,
    /// Capture file lists.
    pub capture_files: bool,
    /// Skip captures larger than this (bytes). Guards against a copied 500 MB
    /// log file stalling the pipeline.
    pub max_capture_bytes: usize,
    /// Automatically encrypt anything the classifier flags as a credential.
    pub encrypt_secrets: bool,
    /// Skip captures entirely when the source app is on the ignore list.
    pub respect_ignore_list: bool,

    // --- retention -------------------------------------------------------
    pub retention: Retention,
    /// Hard cap on live items; the oldest unpinned entries are pruned first.
    pub max_items: u32,
    /// Days a trashed item survives before permanent deletion.
    pub trash_days: u32,

    // --- interface -------------------------------------------------------
    pub theme: Theme,
    pub density: Density,
    /// Accent colour as a hex string; drives the whole UI palette.
    pub accent: String,
    /// Show the launcher near the cursor rather than centred.
    pub launcher_follows_cursor: bool,
    /// Paste immediately after picking an item, instead of only copying.
    pub paste_on_select: bool,
    /// Hide the launcher after a paste.
    pub hide_after_paste: bool,
    /// Blur previews of sensitive items until explicitly revealed.
    pub blur_secrets: bool,

    // --- shortcuts -------------------------------------------------------
    /// Global hotkey that opens the launcher.
    pub hotkey_launcher: String,
    /// Global hotkey for the command palette.
    pub hotkey_palette: String,
    /// Global hotkey that pastes the previous item directly.
    pub hotkey_quick_paste: String,

    // --- system ----------------------------------------------------------
    pub start_with_system: bool,
    pub start_minimized: bool,
    pub show_tray_icon: bool,

    // --- intelligence ----------------------------------------------------
    pub ai: AiSettings,

    // --- sync ------------------------------------------------------------
    /// Folder used for file-based sync (e.g. inside OneDrive or Dropbox).
    /// Empty means sync is off. No server is ever involved.
    pub sync_folder: String,
    pub sync_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            capture_images: true,
            capture_files: true,
            max_capture_bytes: 32 * 1024 * 1024,
            encrypt_secrets: true,
            respect_ignore_list: true,

            retention: Retention::Forever,
            max_items: 500_000,
            trash_days: 30,

            theme: Theme::System,
            density: Density::Comfortable,
            accent: "#6366f1".into(),
            launcher_follows_cursor: false,
            paste_on_select: true,
            hide_after_paste: true,
            blur_secrets: true,

            // Ctrl+Shift+V is the de-facto standard for clipboard history and
            // does not collide with a plain paste.
            hotkey_launcher: "CommandOrControl+Shift+V".into(),
            hotkey_palette: "CommandOrControl+Shift+P".into(),
            hotkey_quick_paste: "CommandOrControl+Shift+B".into(),

            start_with_system: false,
            start_minimized: true,
            show_tray_icon: true,

            ai: AiSettings::default(),

            sync_folder: String::new(),
            sync_enabled: false,
        }
    }
}

impl Settings {
    /// Load from the database, falling back to defaults for a fresh install or
    /// an unreadable blob (a corrupt settings row must never block startup).
    pub fn load(conn: &Conn) -> Settings {
        match meta::get_setting_raw(conn, SETTINGS_KEY) {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|e| {
                tracing::warn!(?e, "settings were unreadable; falling back to defaults");
                Settings::default()
            }),
            _ => Settings::default(),
        }
    }

    pub fn save(&self, conn: &Conn) -> Result<()> {
        meta::set_setting_raw(conn, SETTINGS_KEY, &serde_json::to_string(self)?)
    }

    /// Cutoff timestamp implied by the retention policy, if any.
    pub fn retention_cutoff(&self, now: i64) -> Option<i64> {
        match self.retention {
            Retention::Forever => None,
            Retention::Days(d) => Some(now - (d as i64) * 86_400_000),
        }
    }

    pub fn trash_cutoff(&self, now: i64) -> i64 {
        now - (self.trash_days as i64) * 86_400_000
    }
}

/// Vault configuration is stored separately from settings so exporting settings
/// never carries key material along with it.
pub fn load_vault(conn: &Conn) -> Result<Option<crate::infra::crypto::VaultConfig>> {
    match meta::get_setting_raw(conn, VAULT_KEY)? {
        Some(json) => Ok(serde_json::from_str(&json).ok()),
        None => Ok(None),
    }
}

pub fn save_vault(conn: &Conn, config: &crate::infra::crypto::VaultConfig) -> Result<()> {
    meta::set_setting_raw(conn, VAULT_KEY, &serde_json::to_string(config)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db;

    #[test]
    fn defaults_round_trip_through_the_database() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();

        let mut settings = Settings::default();
        settings.accent = "#ff0000".into();
        settings.retention = Retention::Days(30);
        settings.save(&conn).unwrap();

        let loaded = Settings::load(&conn);
        assert_eq!(loaded.accent, "#ff0000");
        assert_eq!(loaded.retention, Retention::Days(30));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        // Simulates a settings blob written by an older build.
        meta::set_setting_raw(&conn, SETTINGS_KEY, r##"{"accent":"#00ff00"}"##).unwrap();

        let loaded = Settings::load(&conn);
        assert_eq!(loaded.accent, "#00ff00");
        assert!(loaded.capture_enabled, "new field did not take its default");
    }

    #[test]
    fn corrupt_settings_do_not_panic() {
        let pool = db::open_memory().unwrap();
        let conn = pool.get().unwrap();
        meta::set_setting_raw(&conn, SETTINGS_KEY, "not json at all").unwrap();
        let loaded = Settings::load(&conn);
        assert_eq!(loaded.accent, Settings::default().accent);
    }

    #[test]
    fn retention_cutoff_matches_policy() {
        let mut s = Settings::default();
        assert!(s.retention_cutoff(1_000_000).is_none());
        s.retention = Retention::Days(1);
        assert_eq!(s.retention_cutoff(86_400_000 * 2), Some(86_400_000));
    }
}
