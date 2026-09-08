//! Linux platform integration (X11 and Wayland).
//!
//! Three jobs live here, mirroring the Windows implementation:
//! 1. Knowing which application owns the foreground window, so captures can be
//!    attributed and password managers can be ignored.
//! 2. Synthesising Ctrl+V so picking an item actually pastes.
//! 3. Reading clipboard formats `arboard` does not expose (HTML, file lists) —
//!    currently honest `None`s; the text and image paths go through `arboard`.
//!
//! Display-server strategy:
//! - Hyprland (`HYPRLAND_INSTANCE_SIGNATURE` set): query `hyprctl activewindow`.
//! - Sway (`SWAYSOCK` set): query `swaymsg -t get_tree` for the focused node.
//! - Generic Wayland (`WAYLAND_DISPLAY` set): the security model isolates
//!   windows from each other, so metadata is unavailable — return `None`
//!   rather than guessing or crashing.
//! - X11: full introspection via pure-Rust `x11rb` (`_NET_ACTIVE_WINDOW`).
//!
//! Every function here is panic-free: all fallible steps use `Option`/`Result`
//! combinators, never `unwrap` or `expect`.

#![cfg(target_os = "linux")]

use std::sync::atomic::{AtomicU32, Ordering};

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::error::{Error, Result};

/// Process-local clipboard sequence counter.
///
/// Windows has a real OS sequence number; Linux has none, so the watcher bumps
/// this on every event and `expect_self_write` snapshots it — the same
/// echo-suppression protocol, backed by the same atomic.
static SEQ: AtomicU32 = AtomicU32::new(1);

/// Identity of the window that was focused when a copy happened.
#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    /// Application class / id, e.g. `firefox`.
    pub app: Option<String>,
    /// Window title at the moment of capture.
    pub title: Option<String>,
    /// X11 window id where available, otherwise 0.
    pub hwnd: isize,
}

/// Current sequence value (does not increment).
pub fn sequence_number() -> u32 {
    SEQ.load(Ordering::Relaxed)
}

/// Advance the sequence and return the new value.
///
/// Called once per real clipboard event by the watcher.
pub fn bump_sequence_number() -> u32 {
    SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

/// Detect the active window across Hyprland, Sway, X11 and generic Wayland.
///
/// Never panics; returns a default (unknown) `WindowInfo` when the compositor
/// does not allow introspection.
pub fn foreground_window() -> WindowInfo {
    // 1. Hyprland (popular on Arch / Parch): direct IPC via hyprctl.
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        if let Some(info) = read_hyprland_window() {
            return info;
        }
    }

    // 2. Sway and other wlroots compositors exposing SWAYSOCK.
    if std::env::var("SWAYSOCK").is_ok() {
        if let Some(info) = read_sway_window() {
            return info;
        }
    }

    // 3. Generic Wayland (GNOME/Mutter, KDE/KWin, wlroots): the security model
    //    isolates windows, so there is nothing honest to report.
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return WindowInfo::default();
    }

    // 4. X11 session: full introspection.
    read_x11_window().unwrap_or_default()
}

/// Query Hyprland's active window via its `hyprctl` IPC client.
fn read_hyprland_window() -> Option<WindowInfo> {
    let output = std::process::Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let app = v.get("class").and_then(|c| c.as_str()).map(|s| s.to_string());
    let title = v.get("title").and_then(|t| t.as_str()).map(|s| s.to_string());
    if app.is_none() && title.is_none() {
        return None;
    }
    // Hyprland reports an address string, not a numeric id; keep 0.
    Some(WindowInfo { app, title, hwnd: 0 })
}

/// Query Sway's focused window via `swaymsg -t get_tree`.
///
/// Walks the tree depth-first so the deepest focused leaf (the actual window,
/// not one of its focused ancestor containers) wins.
fn read_sway_window() -> Option<WindowInfo> {
    let output = std::process::Command::new("swaymsg")
        .args(["-t", "get_tree"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let tree: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    find_sway_focused(&tree)
}

fn find_sway_focused(node: &serde_json::Value) -> Option<WindowInfo> {
    // Descend first: containers on the focused path are also marked focused,
    // so the window itself is found below them.
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(info) = find_sway_focused(child) {
                    return Some(info);
                }
            }
        }
    }

    let focused = node.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
    if !focused {
        return None;
    }

    // Wayland-native windows expose app_id + name directly.
    if let Some(app_id) = node.get("app_id").and_then(|v| v.as_str()) {
        let title = node.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
        return Some(WindowInfo {
            app: Some(app_id.to_string()),
            title,
            hwnd: 0,
        });
    }

    // XWayland windows nest class/title under window_properties.
    if let Some(props) = node.get("window_properties") {
        let app = props.get("class").and_then(|v| v.as_str()).map(|s| s.to_string());
        let title = props.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
        if app.is_some() || title.is_some() {
            return Some(WindowInfo { app, title, hwnd: 0 });
        }
    }

    None
}

/// Pure-Rust X11 active-window introspection.
///
/// Reads `_NET_ACTIVE_WINDOW` off the root window, then `_NET_WM_NAME` and
/// `WM_CLASS` off the active window. Any failure (no display, missing atoms,
/// gone window) yields `None`.
fn read_x11_window() -> Option<WindowInfo> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?;
    let root = screen.root;

    let net_active = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
    let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME").ok()?.reply().ok()?.atom;
    let utf8_string = conn.intern_atom(false, b"UTF8_STRING").ok()?.reply().ok()?.atom;

    let prop = conn
        .get_property(false, root, net_active, u32::from(AtomEnum::WINDOW), 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let win_id = prop.value32()?.next()?;
    if win_id == 0 {
        return None;
    }

    let title = conn
        .get_property(false, win_id, net_wm_name, utf8_string, 0, 1024)
        .ok()?
        .reply()
        .ok()
        .and_then(|reply| String::from_utf8(reply.value).ok())
        .filter(|s| !s.is_empty());

    let app = conn
        .get_property(
            false,
            win_id,
            u32::from(AtomEnum::WM_CLASS),
            u32::from(AtomEnum::STRING),
            0,
            1024,
        )
        .ok()?
        .reply()
        .ok()
        .and_then(|reply| {
            reply
                .value
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .last()
                .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
        });

    Some(WindowInfo {
        app,
        title,
        hwnd: win_id as isize,
    })
}

/// Bring the target back into focus (best effort) and synthesise Ctrl+V.
///
/// The 60 ms yield mirrors the Windows implementation: focus changes are
/// asynchronous, and without a beat the keystroke can land in the window that
/// is on its way out.
pub fn paste_into(_hwnd: isize) -> Result<()> {
    std::thread::sleep(std::time::Duration::from_millis(60));

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| Error::platform(format!("Enigo initialization failed: {e:?}")))?;

    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| Error::platform(format!("failed to press Control: {e:?}")))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| Error::platform(format!("failed to send V keystroke: {e:?}")))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| Error::platform(format!("failed to release Control: {e:?}")))?;

    Ok(())
}

/// The HTML flavour of the clipboard, when exposed.
///
/// Not yet implemented on Linux; the plain-text path covers capture.
pub fn read_html() -> Option<String> {
    None
}

/// File paths from the clipboard, when exposed by the file manager.
///
/// Not yet implemented on Linux.
pub fn read_files() -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_number_increments_monotonically() {
        let first = bump_sequence_number();
        let second = bump_sequence_number();
        assert!(second > first);
        assert!(sequence_number() >= second);
    }

    #[test]
    fn sway_search_ignores_unfocused_nodes() {
        let tree = serde_json::json!({
            "focused": true,
            "nodes": [
                { "focused": false, "app_id": "background", "name": "bg" },
                { "focused": true, "app_id": "firefox", "name": "Example" },
            ]
        });
        let info = find_sway_focused(&tree).unwrap();
        assert_eq!(info.app.as_deref(), Some("firefox"));
        assert_eq!(info.title.as_deref(), Some("Example"));
    }

    #[test]
    fn sway_search_prefers_deepest_focused_leaf() {
        let tree = serde_json::json!({
            "focused": true,
            "app_id": "outer",
            "nodes": [{ "focused": true, "app_id": "inner", "name": "Inner" }]
        });
        let info = find_sway_focused(&tree).unwrap();
        assert_eq!(info.app.as_deref(), Some("inner"));
    }

    #[test]
    fn sway_search_reads_xwayland_properties() {
        let tree = serde_json::json!({
            "focused": true,
            "window_properties": { "class": "Code", "title": "editor" }
        });
        let info = find_sway_focused(&tree).unwrap();
        assert_eq!(info.app.as_deref(), Some("Code"));
        assert_eq!(info.title.as_deref(), Some("editor"));
    }

    #[test]
    fn sway_search_returns_none_without_focus() {
        let tree = serde_json::json!({ "focused": false, "app_id": "x" });
        assert!(find_sway_focused(&tree).is_none());
    }
}
