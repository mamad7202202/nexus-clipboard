//! Launch-at-login integration.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

use crate::error::Result;

/// Enable or disable starting with the operating system.
///
/// Errors are logged rather than propagated to the caller's UI flow: a user
/// toggling this switch should not see a save fail because the registry was
/// briefly unavailable.
pub fn apply(app: &AppHandle, enabled: bool) -> Result<()> {
    let manager = app.autolaunch();

    let currently = manager.is_enabled().unwrap_or(false);
    if currently == enabled {
        return Ok(());
    }

    let result = if enabled { manager.enable() } else { manager.disable() };

    if let Err(e) = result {
        tracing::warn!(?e, enabled, "could not change the autostart setting");
    }

    Ok(())
}

/// Whether the OS currently has us registered to start at login.
pub fn is_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}
