//! Events pushed from the backend to the UI.
//!
//! Names are exported as constants so the TypeScript side can import the same
//! strings (see `src/lib/events.ts`) and a rename cannot drift between layers.

use tauri::{AppHandle, Emitter};

/// The history changed in some way; the UI should refetch the current page.
pub const HISTORY_CHANGED: &str = "history:changed";

/// A new item was captured. Payload: the item id.
pub const ITEM_CAPTURED: &str = "history:captured";

/// Capture was paused or resumed. Payload: `bool` (true = capturing).
pub const CAPTURE_STATE: &str = "capture:state";

/// The vault was locked or unlocked. Payload: `bool` (true = unlocked).
pub const VAULT_STATE: &str = "vault:state";

/// Settings were saved elsewhere (tray menu, another window).
pub const SETTINGS_CHANGED: &str = "settings:changed";

/// A global hotkey fired and the UI should open a specific surface.
/// Payload: `"launcher" | "palette"`.
pub const HOTKEY: &str = "hotkey:fired";

/// Background maintenance finished. Payload: the report.
pub const MAINTENANCE_DONE: &str = "maintenance:done";

pub fn history_changed(app: &AppHandle) {
    // A failed emit means every window is gone, i.e. we are shutting down.
    let _ = app.emit(HISTORY_CHANGED, ());
}

pub fn item_captured(app: &AppHandle, id: i64) {
    let _ = app.emit(ITEM_CAPTURED, id);
    let _ = app.emit(HISTORY_CHANGED, ());
}
