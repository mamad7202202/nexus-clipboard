//! Event-driven clipboard watcher.
//!
//! On Windows we create a message-only window and register it with
//! `AddClipboardFormatListener`. The OS then posts `WM_CLIPBOARDUPDATE`
//! whenever the clipboard changes — no polling, so the process uses no CPU at
//! all while the user is not copying anything.
//!
//! On Linux we monitor both standard CLIPBOARD and PRIMARY selection (mouse
//! selection, matching CopyQ behavior) via native XFixes events on X11,
//! with adaptive, push-driven Wayland monitoring.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// What the watcher reports. The payload is read on the consumer side so the
/// message loop stays responsive.
#[derive(Debug, Clone, Copy)]
pub struct ClipboardChanged {
    /// Sequence number at the time of the event.
    pub sequence: u32,
}

/// Handle used to control a running watcher from other threads.
#[derive(Clone)]
pub struct WatcherHandle {
    running: Arc<AtomicBool>,
    /// Sequence numbers we produced ourselves, so our own writes are ignored.
    self_write: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
}

impl WatcherHandle {
    /// Suspend capture without tearing down the listener.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Called immediately before the app writes to the clipboard itself.
    ///
    /// Re-copying an item from the history must not create a second entry, and
    /// the cheapest way to tell "us" from "them" is the sequence number the OS
    /// assigns to the write we are about to make.
    pub fn expect_self_write(&self) {
        self.self_write
            .store(crate::infra::platform::sequence_number().wrapping_add(1), Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

pub struct Watcher {
    pub handle: WatcherHandle,
    pub events: Receiver<ClipboardChanged>,
}

impl Watcher {
    /// Start watching. The returned receiver yields one event per real change.
    pub fn start() -> crate::error::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let handle = WatcherHandle {
            running: Arc::new(AtomicBool::new(true)),
            self_write: Arc::new(AtomicU32::new(0)),
            paused: Arc::new(AtomicBool::new(false)),
        };

        spawn_loop(tx, handle.clone())?;

        Ok(Self { handle, events: rx })
    }
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn spawn_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) -> crate::error::Result<()> {
    use std::cell::RefCell;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

    use crate::infra::platform::windows::{
        AddClipboardFormatListener, CreateWindowExW, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GetMessageW, GetModuleHandleW, PostQuitMessage,
        RegisterClassW, RemoveClipboardFormatListener, TranslateMessage, CW_USEDEFAULT, HWND_MESSAGE,
        MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW, WM_CLIPBOARDUPDATE, WM_DESTROY,
    };

    thread_local! {
        static CONTEXT: RefCell<Option<(Sender<ClipboardChanged>, WatcherHandle)>> =
            const { RefCell::new(None) };
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CLIPBOARDUPDATE => {
                CONTEXT.with(|ctx| {
                    if let Some((tx, handle)) = ctx.borrow().as_ref() {
                        if handle.is_paused() {
                            return;
                        }
                        let sequence = crate::infra::platform::sequence_number();
                        let expected = handle.self_write.load(Ordering::SeqCst);

                        // Ignore the echo of a write we made ourselves.
                        if expected != 0 && sequence <= expected {
                            handle.self_write.store(0, Ordering::SeqCst);
                            return;
                        }
                        let _ = tx.send(ClipboardChanged { sequence });
                    }
                });
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    let running = handle.running.clone();

    std::thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(move || {
            CONTEXT.with(|ctx| *ctx.borrow_mut() = Some((tx, handle)));

            unsafe {
                let instance = match GetModuleHandleW(PCWSTR::null()) {
                    Ok(i) => i,
                    Err(e) => {
                        tracing::error!(?e, "GetModuleHandle failed; clipboard watching disabled");
                        return;
                    }
                };

                let class_name: Vec<u16> = "NexusClipboardWatcher\0".encode_utf16().collect();
                let class = WNDCLASSW {
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: instance.into(),
                    lpszClassName: PCWSTR(class_name.as_ptr()),
                    ..Default::default()
                };
                RegisterClassW(&class);

                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    PCWSTR(class_name.as_ptr()),
                    PCWSTR(class_name.as_ptr()),
                    WINDOW_STYLE(0),
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(instance.into()),
                    None,
                );

                let hwnd = match hwnd {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::error!(?e, "could not create the watcher window");
                        return;
                    }
                };

                if AddClipboardFormatListener(hwnd).is_err() {
                    tracing::error!("AddClipboardFormatListener failed");
                    let _ = DestroyWindow(hwnd);
                    return;
                }

                tracing::info!("clipboard watcher started");

                let mut msg = MSG::default();
                while running.load(Ordering::SeqCst) {
                    let result = GetMessageW(&mut msg, None, 0, 0);
                    if result.0 <= 0 {
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let _ = RemoveClipboardFormatListener(hwnd);
                let _ = DestroyWindow(hwnd);
                tracing::info!("clipboard watcher stopped");
            }
        })
        .map_err(|e| crate::error::Error::platform(format!("cannot spawn watcher thread: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Linux implementation: Robust Dual-Protocol (X11 XFixes & Wayland)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn spawn_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) -> crate::error::Result<()> {
    use crate::infra::platform::linux::{detect_session_type, SessionType};

    std::thread::Builder::new()
        .name("clipboard-watcher-linux".into())
        .spawn(move || {
            let session = detect_session_type();
            tracing::info!(?session, "starting Linux clipboard listener");

            if session == SessionType::X11 {
                if let Err(e) = run_x11_xfixes_loop(tx.clone(), handle.clone()) {
                    tracing::warn!(?e, "X11 XFixes monitor failed; falling back to Wayland/Polling monitor");
                    run_wayland_loop(tx, handle);
                }
            } else {
                run_wayland_loop(tx, handle);
            }
        })
        .map_err(|e| crate::error::Error::platform(format!("cannot spawn watcher thread: {e}")))?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn run_x11_xfixes_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) -> Result<(), Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = conn.setup().roots.get(screen_num).ok_or("no screen")?;
    let root = screen.root;

    // Initialize XFixes extension
    conn.xfixes_query_version(5, 0)?.reply()?;

    let clip_atom = conn.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
    let primary_atom = AtomEnum::PRIMARY.into();

    // Listen for both standard CLIPBOARD and PRIMARY selection (like CopyQ)
    conn.xfixes_select_selection_input(
        root,
        clip_atom,
        SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE,
    )?;

    conn.xfixes_select_selection_input(
        root,
        primary_atom,
        SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE,
    )?;

    conn.flush()?;
    tracing::info!("native X11 XFixes clipboard & primary listener active");

    let mut last_event_time = std::time::Instant::now();

    while handle.is_running() {
        if let Ok(Some(event)) = conn.poll_for_event() {
            if let x11rb::protocol::Event::XfixesSelectionNotify(notify) = event {
                if handle.is_paused() {
                    continue;
                }

                // Debounce high frequency mouse selection events
                if notify.selection == primary_atom {
                    if last_event_time.elapsed() < Duration::from_millis(150) {
                        continue;
                    }
                    last_event_time = std::time::Instant::now();
                }

                let seq = crate::infra::platform::linux::bump_sequence_number();
                let expected = handle.self_write.load(Ordering::SeqCst);

                // Echo suppression
                if expected != 0 && seq <= expected {
                    handle.self_write.store(0, Ordering::SeqCst);
                    continue;
                }

                if tx.send(ClipboardChanged { sequence: seq }).is_err() {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn run_wayland_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) {
    use std::time::Duration;
    const POLL_INTERVAL: Duration = Duration::from_millis(200);

    let mut last_hash = String::new();
    tracing::info!("Wayland adaptive clipboard monitor started");

    while handle.is_running() {
        std::thread::sleep(POLL_INTERVAL);
        if handle.is_paused() {
            continue;
        }

        // Read through wl-paste or arboard
        let current_content = if let Ok(output) = std::process::Command::new("wl-paste")
            .args(["-n"])
            .output()
        {
            if output.status.success() && !output.stdout.is_empty() {
                crate::util::hash_bytes(&output.stdout)
            } else {
                continue;
            }
        } else if let Ok(mut clip) = arboard::Clipboard::new() {
            if let Ok(text) = clip.get_text() {
                if text.is_empty() {
                    continue;
                }
                crate::util::hash_bytes(text.as_bytes())
            } else if let Ok(img) = clip.get_image() {
                let mut h = Vec::with_capacity(8 + img.bytes.len().min(1024));
                h.extend_from_slice(&(img.width as u32).to_le_bytes());
                h.extend_from_slice(&(img.height as u32).to_le_bytes());
                h.extend_from_slice(&img.bytes[..img.bytes.len().min(1024)]);
                crate::util::hash_bytes(&h)
            } else {
                continue;
            }
        } else {
            continue;
        };

        if current_content == last_hash {
            continue;
        }

        last_hash = current_content;
        let seq = crate::infra::platform::linux::bump_sequence_number();
        let expected = handle.self_write.load(Ordering::SeqCst);

        if expected != 0 && seq <= expected {
            handle.self_write.store(0, Ordering::SeqCst);
            continue;
        }

        if tx.send(ClipboardChanged { sequence: seq }).is_err() {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Portable fallback: polling
// ---------------------------------------------------------------------------

#[cfg(not(any(windows, target_os = "linux")))]
fn spawn_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) -> crate::error::Result<()> {
    use std::time::Duration;
    const POLL: Duration = Duration::from_millis(400);

    std::thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(move || {
            let mut last = String::new();
            let mut sequence = 0u32;

            while handle.is_running() {
                std::thread::sleep(POLL);
                if handle.is_paused() {
                    continue;
                }
                let Ok(mut clipboard) = arboard::Clipboard::new() else { continue };
                let Ok(text) = clipboard.get_text() else { continue };
                if text == last {
                    continue;
                }
                last = text;
                sequence = sequence.wrapping_add(1);
                if tx.send(ClipboardChanged { sequence }).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| crate::error::Error::platform(format!("cannot spawn watcher thread: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_tracks_pause_state() {
        let handle = WatcherHandle {
            running: Arc::new(AtomicBool::new(true)),
            self_write: Arc::new(AtomicU32::new(0)),
            paused: Arc::new(AtomicBool::new(false)),
        };
        assert!(!handle.is_paused());
        handle.set_paused(true);
        assert!(handle.is_paused());
        handle.stop();
        assert!(!handle.is_running());
    }
}
