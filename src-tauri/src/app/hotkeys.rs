//! Global shortcut registration.
//!
//! Registrations are best-effort: another application may already own a
//! combination, and that must not stop the app from starting. Failures are
//! logged and surfaced in settings rather than thrown.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::app::{state::AppState, window};
use crate::config::Settings;
use crate::error::Result;

/// Register every configured shortcut. Safe to call repeatedly.
pub fn register(app: &AppHandle, settings: &Settings) -> Result<()> {
    let bindings: [(&str, Action); 3] = [
        (settings.hotkey_launcher.as_str(), Action::Launcher),
        (settings.hotkey_palette.as_str(), Action::Palette),
        (settings.hotkey_quick_paste.as_str(), Action::QuickPaste),
    ];

    for (accelerator, action) in bindings {
        if accelerator.trim().is_empty() {
            continue;
        }
        let shortcut: Shortcut = match accelerator.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(accelerator, ?e, "unparseable shortcut; skipping");
                continue;
            }
        };

        let handle = app.clone();
        let result = app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
            // Fire on press only; without this the action runs twice per tap.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            dispatch(&handle, action);
        });

        if let Err(e) = result {
            tracing::warn!(accelerator, ?e, "could not register the shortcut (already in use?)");
        }
    }

    Ok(())
}

/// Drop all registrations and apply the current settings again.
pub fn reregister(app: &AppHandle, settings: &Settings) -> Result<()> {
    if let Err(e) = app.global_shortcut().unregister_all() {
        tracing::warn!(?e, "could not clear existing shortcuts");
    }
    register(app, settings)
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Launcher,
    Palette,
    QuickPaste,
}

fn dispatch(app: &AppHandle, action: Action) {
    match action {
        Action::Launcher => {
            if let Err(e) = window::toggle_launcher(app) {
                tracing::warn!(?e, "could not toggle the launcher");
            }
        }
        Action::Palette => {
            if let Err(e) = window::show_launcher(app) {
                tracing::warn!(?e, "could not open the launcher");
                return;
            }
            let _ = app.emit(crate::ipc::events::HOTKEY, "palette");
        }
        Action::QuickPaste => quick_paste(app),
    }
}

/// Paste the second-most-recent item without opening any UI.
///
/// The most recent item is what a plain Ctrl+V already gives you, so the
/// genuinely useful shortcut is the one *before* it — that is what makes
/// alternating between two values fast.
fn quick_paste(app: &AppHandle) {
    let Some(state) = app.try_state::<Arc<AppState>>() else { return };

    let target = {
        let Ok(conn) = state.conn() else { return };
        let query = crate::domain::Query { limit: 2, ..Default::default() };
        match crate::infra::db::items::list(&conn, &query) {
            Ok(page) => page.items.get(1).map(|i| i.id),
            Err(e) => {
                tracing::warn!(?e, "quick paste could not read the history");
                None
            }
        }
    };

    let Some(id) = target else { return };

    // Focus is still on the user's target window; record it before pasting.
    let info = crate::infra::platform::foreground_window();
    state.remember_focus(info.hwnd);

    if let Err(e) = crate::features::paste::paste_item(
        &state,
        id,
        crate::features::paste::PasteMode::Paste,
    ) {
        tracing::warn!(?e, "quick paste failed");
    }
}
