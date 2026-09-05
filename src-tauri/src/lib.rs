//! Nexus Clipboard — a fast, private, local-first clipboard manager.
//!
//! # Architecture
//!
//! ```text
//!   ipc/        Tauri commands + events        ← the only thing the UI sees
//!   app/        composition root, tray, hotkeys, windows, state
//!   features/   use cases: capture, paste, maintenance, backup, ai
//!   domain/     entities and pure rules (no IO, no OS, no SQL)
//!   infra/      adapters: SQLite, blob store, crypto, clipboard, Win32
//! ```
//!
//! Dependencies point inwards only: `infra` and `features` may use `domain`,
//! never the reverse. That is what keeps the classifier and the query model
//! unit-testable without a database or a desktop session.

pub mod app;
pub mod config;
pub mod domain;
pub mod error;
pub mod features;
pub mod infra;
pub mod ipc;
pub mod util;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use crate::app::state::AppState;

/// Build and run the application.
pub fn run() {
    init_tracing();

    let mut builder = tauri::Builder::default();

    // A clipboard manager must be a singleton: two watchers would double every
    // capture. A second launch raises the existing instance instead.
    builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Err(e) = crate::app::window::show_main(app) {
            tracing::warn!(?e, "second instance could not raise the main window");
        }
    }));

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // `--minimized` tells the UI to stay in the tray on a login launch.
            Some(vec!["--minimized"]),
        ))
        .setup(setup)
        .on_window_event(handle_window_event)
        .invoke_handler(ipc::commands::handler())
        .run(tauri::generate_context!())
        .expect("failed to start Nexus Clipboard");
}

fn setup(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // 1. State. Everything else depends on this, so a failure here is fatal and
    //    worth reporting clearly rather than panicking deep in a command.
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve the application data directory: {e}"))?;
    // Constructing the state also opens the vault (machine-keyed on first run,
    // left locked when the user has set a passphrase).
    let state = AppState::new(data_dir)?;
    app.manage(state.clone());

    // 2. Clipboard watcher.
    match infra::clipboard::Watcher::start() {
        Ok(watcher) => {
            state.set_watcher(watcher.handle.clone());
            watcher.handle.set_paused(!state.settings().capture_enabled);
            spawn_capture_loop(handle.clone(), state.clone(), watcher.events);
        }
        Err(e) => tracing::error!(?e, "clipboard watching is unavailable"),
    }

    // 3. OS integration.
    let settings = state.settings();
    if settings.show_tray_icon {
        if let Err(e) = app::tray::build(&handle) {
            tracing::warn!(?e, "could not create the tray icon");
        }
    }
    if let Err(e) = app::hotkeys::register(&handle, &settings) {
        tracing::warn!(?e, "could not register global shortcuts");
    }
    if let Err(e) = app::window::create_launcher(&handle) {
        tracing::warn!(?e, "could not pre-create the launcher window");
    }

    // 4. Housekeeping.
    features::maintenance::spawn(state.clone());

    // 5. Honour "start minimized" — but only when launched by the OS at login.
    let launched_by_system = std::env::args().any(|a| a == "--minimized");
    if settings.start_minimized && launched_by_system {
        if let Some(main) = handle.get_webview_window(app::window::MAIN) {
            let _ = main.hide();
        }
    }

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Nexus Clipboard is ready");
    Ok(())
}

/// Consume clipboard events and run the capture pipeline.
///
/// This runs on a blocking thread rather than the async runtime: the pipeline
/// is entirely synchronous IO, and giving it a dedicated thread means a slow
/// capture (a 20 MB image) never delays a command the user is waiting on.
fn spawn_capture_loop(
    app: tauri::AppHandle,
    state: Arc<AppState>,
    events: std::sync::mpsc::Receiver<infra::clipboard::watcher::ClipboardChanged>,
) {
    std::thread::Builder::new()
        .name("capture-pipeline".into())
        .spawn(move || {
            // Rapid-fire events (some apps write the clipboard several times per
            // copy) are collapsed: wait briefly, then drain anything queued.
            const SETTLE: std::time::Duration = std::time::Duration::from_millis(40);

            while events.recv().is_ok() {
                std::thread::sleep(SETTLE);
                while events.try_recv().is_ok() {}

                match features::capture::capture(&state) {
                    Ok(features::capture::Outcome::Stored(id)) => {
                        ipc::events::item_captured(&app, id);
                    }
                    Ok(features::capture::Outcome::Duplicate(_)) => {
                        ipc::events::history_changed(&app);
                    }
                    Ok(features::capture::Outcome::Skipped(reason)) => {
                        tracing::trace!(reason = reason.as_str(), "capture skipped");
                    }
                    Err(e) => tracing::warn!(?e, "capture failed"),
                }
            }

            tracing::info!("capture pipeline stopped");
        })
        .expect("could not spawn the capture pipeline thread");
}

/// Closing the main window hides it instead of quitting: the app lives in the
/// tray, and quitting on close would silently stop clipboard history.
fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == app::window::MAIN {
            let keep_running = window
                .app_handle()
                .try_state::<Arc<AppState>>()
                .map(|s| s.settings().show_tray_icon)
                .unwrap_or(false);

            if keep_running {
                api.prevent_close();
                let _ = window.hide();
            }
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_env("NEXUS_LOG")
        .unwrap_or_else(|_| EnvFilter::new(if cfg!(debug_assertions) { "info" } else { "warn" }));

    // A failure here means a subscriber is already installed, which is fine.
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(cfg!(debug_assertions))
        .try_init();
}
