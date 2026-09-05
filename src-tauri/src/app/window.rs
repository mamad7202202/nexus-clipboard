//! Window management.
//!
//! Two surfaces exist:
//! - **main** — the full application: history, collections, settings.
//! - **launcher** — a frameless, always-on-top quick-picker summoned by the
//!   global hotkey. It is created once and then shown/hidden, because creating
//!   a webview takes ~100 ms and the launcher must feel instant.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::app::state::AppState;
use crate::error::{Error, Result};

pub const MAIN: &str = "main";
pub const LAUNCHER: &str = "launcher";

/// Launcher dimensions. Wide enough for a preview pane, short enough to feel
/// like an overlay rather than a window.
const LAUNCHER_W: f64 = 780.0;
const LAUNCHER_H: f64 = 520.0;

/// Create the launcher window up front, hidden.
pub fn create_launcher(app: &AppHandle) -> Result<()> {
    if app.get_webview_window(LAUNCHER).is_some() {
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, LAUNCHER, WebviewUrl::App("index.html?view=launcher".into()))
        .title("Nexus")
        .inner_size(LAUNCHER_W, LAUNCHER_H)
        .min_inner_size(560.0, 360.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .visible(false)
        .shadow(true)
        .build()?;

    // Closing the launcher with Esc or the window chrome should hide it, not
    // destroy it — rebuilding costs the responsiveness we are protecting.
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            hide_launcher(&handle);
        }
    });

    Ok(())
}

/// Show the launcher, remembering which window to paste back into.
pub fn show_launcher(app: &AppHandle) -> Result<()> {
    // Capture the target *before* our window takes focus.
    if let Some(state) = app.try_state::<std::sync::Arc<AppState>>() {
        let info = crate::infra::platform::foreground_window();
        state.remember_focus(info.hwnd);
    }

    create_launcher(app)?;
    let window = app
        .get_webview_window(LAUNCHER)
        .ok_or_else(|| Error::other("launcher window is missing"))?;

    position_launcher(app, &window)?;
    window.show()?;
    window.set_focus()?;
    // Tell the UI to reset its search box and selection.
    let _ = tauri::Emitter::emit(app, crate::ipc::events::HOTKEY, "launcher");

    Ok(())
}

/// Toggle: a second press of the hotkey dismisses the launcher.
pub fn toggle_launcher(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window(LAUNCHER) {
        if window.is_visible().unwrap_or(false) {
            hide_launcher(app);
            return Ok(());
        }
    }
    show_launcher(app)
}

pub fn hide_launcher(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LAUNCHER) {
        let _ = window.hide();
    }
}

fn position_launcher(app: &AppHandle, window: &tauri::WebviewWindow) -> Result<()> {
    let follow_cursor = app
        .try_state::<std::sync::Arc<AppState>>()
        .map(|s| s.settings().launcher_follows_cursor)
        .unwrap_or(false);

    if !follow_cursor {
        // Centre on the monitor that currently has the cursor, not always the
        // primary one — multi-monitor users notice this immediately.
        if let Ok(Some(monitor)) = window.current_monitor() {
            let size = monitor.size();
            let scale = monitor.scale_factor();
            let position = monitor.position();
            let x = position.x as f64 + (size.width as f64 / scale - LAUNCHER_W) / 2.0;
            // Slightly above centre reads better than dead centre.
            let y = position.y as f64 + (size.height as f64 / scale - LAUNCHER_H) / 2.5;
            window.set_position(tauri::LogicalPosition::new(x, y))?;
        } else {
            window.center()?;
        }
        return Ok(());
    }

    let Ok(cursor) = window.cursor_position() else {
        window.center()?;
        return Ok(());
    };

    let (mut x, mut y) = (cursor.x, cursor.y + 24.0);

    // Keep the whole window on screen even when summoned near an edge.
    if let Ok(Some(monitor)) = window.current_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let origin = monitor.position();
        let max_x = origin.x as f64 + size.width as f64 / scale - LAUNCHER_W - 16.0;
        let max_y = origin.y as f64 + size.height as f64 / scale - LAUNCHER_H - 16.0;
        x = x.min(max_x).max(origin.x as f64 + 16.0);
        y = y.min(max_y).max(origin.y as f64 + 16.0);
    }

    window.set_position(tauri::LogicalPosition::new(x, y))?;
    Ok(())
}

/// Bring the main window up, creating it if the user closed it earlier.
pub fn show_main(app: &AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window(MAIN) {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Nexus Clipboard")
        .inner_size(1180.0, 760.0)
        .min_inner_size(880.0, 560.0)
        .center()
        .decorations(true)
        .build()?;

    Ok(())
}

/// Open the main window on a particular route (used by the tray menu).
pub fn show_main_at(app: &AppHandle, route: &str) -> Result<()> {
    show_main(app)?;
    let _ = tauri::Emitter::emit(app, "navigate", route);
    Ok(())
}
