//! System tray icon and menu.
//!
//! The tray is the app's real home: the main window can be closed, but as long
//! as the tray icon is there, capture keeps running.

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::app::{state::AppState, window};
use crate::error::Result;

pub const TRAY_ID: &str = "nexus-tray";

const ID_OPEN: &str = "open";
const ID_LAUNCHER: &str = "launcher";
const ID_PAUSE: &str = "pause";
const ID_CLEAR: &str = "clear";
const ID_SETTINGS: &str = "settings";
const ID_VAULT: &str = "vault";
const ID_QUIT: &str = "quit";

pub fn build(app: &AppHandle) -> Result<()> {
    let state = app.state::<Arc<AppState>>();
    let capturing = state.settings().capture_enabled;

    let open = MenuItem::with_id(app, ID_OPEN, "Open Nexus", true, Some("CmdOrCtrl+O"))?;
    let launcher = MenuItem::with_id(app, ID_LAUNCHER, "Quick Paste…", true, Some("CmdOrCtrl+Shift+V"))?;
    let pause = CheckMenuItem::with_id(app, ID_PAUSE, "Capture clipboard", true, capturing, None::<&str>)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, "Settings…", true, None::<&str>)?;
    let vault = MenuItem::with_id(app, ID_VAULT, "Lock vault", true, None::<&str>)?;
    let clear = MenuItem::with_id(app, ID_CLEAR, "Clear history…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit Nexus", true, None::<&str>)?;

    let danger = Submenu::with_items(app, "Danger zone", true, &[&clear])?;

    let menu = Menu::with_items(
        app,
        &[
            &open,
            &launcher,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &vault,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &danger,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().ok_or_else(|| {
            crate::error::Error::other("the bundle is missing its window icon")
        })?)
        .tooltip("Nexus Clipboard")
        .menu(&menu)
        // The menu should only appear on right-click; left-click opens the app.
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu)
        .on_tray_icon_event(handle_icon)
        .build(app)?;

    Ok(())
}

fn handle_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        ID_OPEN => {
            if let Err(e) = window::show_main(app) {
                tracing::warn!(?e, "could not open the main window");
            }
        }
        ID_LAUNCHER => {
            if let Err(e) = window::toggle_launcher(app) {
                tracing::warn!(?e, "could not open the launcher");
            }
        }
        ID_PAUSE => {
            let Some(state) = app.try_state::<Arc<AppState>>() else { return };
            let next = !state.settings().capture_enabled;
            if let Err(e) = state.update_settings(|s| s.capture_enabled = next) {
                tracing::warn!(?e, "could not persist the capture setting");
                return;
            }
            if let Some(watcher) = state.watcher() {
                watcher.set_paused(!next);
            }
            let _ = app.emit(crate::ipc::events::CAPTURE_STATE, next);
        }
        ID_VAULT => {
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                state.set_vault_key(None);
                let _ = app.emit(crate::ipc::events::VAULT_STATE, false);
            }
        }
        ID_SETTINGS => {
            // Confirmation and the actual work happen in the UI, which can show
            // a proper dialog; the tray only navigates.
            let _ = window::show_main_at(app, "/settings");
        }
        ID_CLEAR => {
            let _ = window::show_main_at(app, "/settings?confirm=clear");
        }
        ID_QUIT => app.exit(0),
        _ => {}
    }
}

fn handle_icon(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    // Left-click opens the launcher; that is the action users reach for most.
    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
        let app = tray.app_handle();
        if let Err(e) = window::toggle_launcher(app) {
            tracing::warn!(?e, "could not toggle the launcher from the tray");
        }
    }
}
