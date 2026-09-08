//! Event-driven clipboard watcher.
//!
//! On Windows we create a message-only window and register it with
//! `AddClipboardFormatListener`. The OS then posts `WM_CLIPBOARDUPDATE`
//! whenever the clipboard changes — no polling, so the process uses no CPU at
//! all while the user is not copying anything. That is the single biggest
//! difference between this and the poll-every-500ms approach most clipboard
//! managers take.
//!
//! The watcher owns a dedicated thread because a Win32 message loop must run on
//! the thread that created the window.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// What the watcher reports. The payload is read on the consumer side so the
/// message loop stays responsive.
#[derive(Debug, Clone, Copy)]
pub struct ClipboardChanged {
    /// Windows' clipboard sequence number at the time of the event.
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
        // The write has not happened yet; the next sequence number will be this
        // one plus one. Recording the current value and comparing with `<=` in
        // the loop covers both the pre- and post-write case.
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

    // The window procedure is a plain `extern "system"` fn and cannot capture,
    // so the channel lives in thread-local storage. This is sound because the
    // window, the message loop and the procedure all run on the same thread.
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
                        // A closed receiver means the app is shutting down.
                        let _ = tx.send(ClipboardChanged { sequence });
                    }
                });
                LRESULT(0)
            }
            WM_DESTROY => {
                // SAFETY: called on the message-loop thread during teardown.
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            // SAFETY: forwarding to the default handler with the original args.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    let running = handle.running.clone();

    std::thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(move || {
            CONTEXT.with(|ctx| *ctx.borrow_mut() = Some((tx, handle)));

            // SAFETY: every call below follows the documented Win32 contract;
            // the window is destroyed and the listener removed before return.
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
                // A duplicate class registration is fine on a restart.
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
                    // A message-only window: never shown, never in the taskbar.
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
                    // GetMessageW blocks until something arrives, which is why
                    // this thread costs nothing while idle.
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
// Linux implementation: event-driven via XFixes (no polling)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn spawn_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) -> crate::error::Result<()> {
    use clipboard_master::{CallbackResult, ClipboardHandler, Master};

    struct LinuxHandler {
        tx: Sender<ClipboardChanged>,
        handle: WatcherHandle,
    }

    impl ClipboardHandler for LinuxHandler {
        fn on_clipboard_change(&mut self) -> CallbackResult {
            if !self.handle.is_running() {
                return CallbackResult::Stop;
            }
            if self.handle.is_paused() {
                return CallbackResult::Next;
            }

            let seq = crate::infra::platform::linux::bump_sequence_number();
            let expected = self.handle.self_write.load(Ordering::SeqCst);

            // Echo suppression: ignore the event our own paste just produced.
            if expected != 0 && seq <= expected {
                self.handle.self_write.store(0, Ordering::SeqCst);
                return CallbackResult::Next;
            }

            // A closed receiver means the app is shutting down.
            if self.tx.send(ClipboardChanged { sequence: seq }).is_err() {
                return CallbackResult::Stop;
            }
            CallbackResult::Next
        }

        fn on_clipboard_error(&mut self, error: std::io::Error) -> CallbackResult {
            tracing::warn!(?error, "Linux clipboard event error");
            CallbackResult::Next
        }
    }

    std::thread::Builder::new()
        .name("clipboard-watcher-linux".into())
        .spawn(move || {
            let handler = LinuxHandler { tx, handle };
            // Master::new opens the X11 connection; on a pure-Wayland session
            // without XWayland this fails, and capture stays disabled rather
            // than crashing or falling back to a polling loop.
            let mut master = match Master::new(handler) {
                Ok(master) => master,
                Err(e) => {
                    tracing::error!(?e, "cannot initialize the Linux clipboard monitor");
                    return;
                }
            };
            // run() blocks on XFixes selection events, so this thread costs
            // nothing while idle. It returns only on Stop or fatal error.
            if let Err(e) = master.run() {
                tracing::error!(?e, "Linux clipboard monitor terminated unexpectedly");
            }
        })
        .map_err(|e| crate::error::Error::platform(format!("cannot spawn watcher thread: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Portable fallback: polling (platforms with no native listener)
// ---------------------------------------------------------------------------

#[cfg(not(any(windows, target_os = "linux")))]
fn spawn_loop(tx: Sender<ClipboardChanged>, handle: WatcherHandle) -> crate::error::Result<()> {
    use std::time::Duration;

    // Without a native change notification the only option is polling. 400 ms
    // is a compromise between responsiveness and idle cost.
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
