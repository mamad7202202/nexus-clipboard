//! Windows-specific integration.
//!
//! Three jobs live here:
//! 1. Knowing which application owns the foreground window, so captures can be
//!    attributed and password managers can be ignored.
//! 2. Restoring focus to that window and synthesising Ctrl+V, which is what
//!    turns "pick an item" into an actual paste.
//! 3. Reading clipboard formats `arboard` does not expose (HTML, CF_HDROP).
//!
//! Everything here is `unsafe` by necessity; each block documents the invariant
//! that makes it sound.

#![cfg(windows)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow,
};

use crate::error::{Error, Result};

/// Identity of the window that was focused when a copy happened.
#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    /// Executable file name, e.g. `Code.exe`.
    pub app: Option<String>,
    /// Window title at the moment of capture.
    pub title: Option<String>,
    /// Raw handle, kept so focus can be restored before pasting.
    pub hwnd: isize,
}

/// Inspect the current foreground window.
pub fn foreground_window() -> WindowInfo {
    // SAFETY: GetForegroundWindow takes no arguments and returns a possibly-null
    // handle, which we check before use.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return WindowInfo::default();
    }

    WindowInfo {
        app: process_name(hwnd),
        title: window_title(hwnd),
        hwnd: hwnd.0 as isize,
    }
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 512];
    // SAFETY: `buf` is a valid, writable slice of the length we pass.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return None;
    }
    let title = OsString::from_wide(&buf[..len as usize])
        .to_string_lossy()
        .trim()
        .to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn process_name(hwnd: HWND) -> Option<String> {
    let mut pid: u32 = 0;
    // SAFETY: `pid` is a valid out-pointer for the lifetime of the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }

    // SAFETY: opening with the most limited rights that still allow reading the
    // image name; the handle is closed on every path below.
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;

    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    // SAFETY: `buf`/`len` describe a valid writable buffer.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: `handle` came from a successful OpenProcess and is not used again.
    unsafe { let _ = CloseHandle(handle); };

    if ok.is_err() || len == 0 {
        return None;
    }

    let full = OsString::from_wide(&buf[..len as usize]).to_string_lossy().to_string();
    full.rsplit('\\').next().map(|s| s.to_string())
}

/// Bring `hwnd` back to the foreground and send Ctrl+V.
///
/// Windows restricts `SetForegroundWindow` to the process that currently owns
/// focus. Because the launcher window *is* that process at the moment the user
/// picks an item, the call succeeds — but only if we hide our own window first,
/// which the caller does.
pub fn paste_into(hwnd: isize) -> Result<()> {
    if hwnd != 0 {
        let target = HWND(hwnd as *mut std::ffi::c_void);
        // SAFETY: handle originated from GetForegroundWindow. If the window has
        // since closed the call simply fails, which we tolerate: the user may
        // have switched apps, and pasting into whatever is focused now is the
        // expected behaviour.
        unsafe { let _ = SetForegroundWindow(target); };
        // Focus changes are asynchronous; without a beat the keystroke can land
        // in the window that is on its way out.
        std::thread::sleep(std::time::Duration::from_millis(60));
    }

    send_ctrl_v()
}

fn send_ctrl_v() -> Result<()> {
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];

    // SAFETY: `inputs` is a valid array of correctly-sized INPUT structs.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(Error::platform("failed to synthesise the paste keystroke"));
    }
    Ok(())
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

// ---------------------------------------------------------------------------
// Extra clipboard formats
// ---------------------------------------------------------------------------

/// RAII guard around `OpenClipboard`/`CloseClipboard`.
///
/// The clipboard is a single global resource; failing to close it wedges every
/// other application on the desktop, so ownership is tied to a guard rather
/// than to careful early returns.
struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self> {
        // The clipboard is frequently held for a few milliseconds by whichever
        // app just wrote to it; retrying is normal, not exceptional.
        for attempt in 0..10 {
            // SAFETY: passing a null owner window is valid and associates the
            // clipboard with the current task.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(std::time::Duration::from_millis(10 * (attempt + 1)));
        }
        Err(Error::Clipboard("clipboard is busy".into()))
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: only reachable when OpenClipboard succeeded.
        unsafe { let _ = CloseClipboard(); };
    }
}

/// Read a registered clipboard format as raw bytes.
fn read_format(format: u32) -> Option<Vec<u8>> {
    // SAFETY: format availability is a simple query with no side effects.
    if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return None;
    }

    let _guard = ClipboardGuard::open().ok()?;

    // SAFETY: guarded by an open clipboard; the returned handle is owned by the
    // clipboard and must not be freed by us.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    if handle.0.is_null() {
        return None;
    }

    let hglobal = windows::Win32::Foundation::HGLOBAL(handle.0);
    // SAFETY: handle came from GetClipboardData for a GlobalAlloc'd format.
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the lock above is released on every path below.
    let size = unsafe { GlobalSize(hglobal) };
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
    // SAFETY: pairs with the GlobalLock above.
    unsafe { let _ = GlobalUnlock(hglobal); };

    Some(bytes)
}

/// The HTML flavour of the clipboard, if the source app published one.
///
/// `CF_HTML` is a registered format wrapping the fragment in a header of byte
/// offsets; we return just the fragment.
pub fn read_html() -> Option<String> {
    // SAFETY: the format name is a valid NUL-terminated wide string.
    let format = unsafe {
        let name: Vec<u16> = "HTML Format\0".encode_utf16().collect();
        RegisterClipboardFormatW(PCWSTR(name.as_ptr()))
    };
    if format == 0 {
        return None;
    }

    let bytes = read_format(format)?;
    let text = String::from_utf8_lossy(&bytes);
    Some(extract_html_fragment(&text))
}

/// Pull the `<!--StartFragment-->…<!--EndFragment-->` span out of a CF_HTML
/// payload, falling back to the whole body when the markers are absent.
fn extract_html_fragment(raw: &str) -> String {
    const START: &str = "<!--StartFragment-->";
    const END: &str = "<!--EndFragment-->";

    if let (Some(s), Some(e)) = (raw.find(START), raw.find(END)) {
        if e > s {
            return raw[s + START.len()..e].trim().to_string();
        }
    }

    // No fragment markers: strip the offset header, which always ends at the
    // first '<'.
    match raw.find('<') {
        Some(idx) => raw[idx..].trim().to_string(),
        None => raw.trim().to_string(),
    }
}

/// File paths copied from Explorer (`CF_HDROP`).
pub fn read_files() -> Option<Vec<String>> {
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    const CF_HDROP: u32 = 15;
    // SAFETY: availability query only.
    if unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_err() {
        return None;
    }

    let _guard = ClipboardGuard::open().ok()?;

    // SAFETY: guarded by an open clipboard.
    let handle = unsafe { GetClipboardData(CF_HDROP) }.ok()?;
    if handle.0.is_null() {
        return None;
    }

    let hdrop = HDROP(handle.0);
    // SAFETY: passing u32::MAX asks for the file count rather than a path.
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
    if count == 0 {
        return None;
    }

    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        let mut buf = [0u16; 32_768]; // MAX_PATH is not enough for long paths.
        // SAFETY: `buf` is a valid writable buffer of the declared length.
        let len = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
        if len > 0 {
            paths.push(OsString::from_wide(&buf[..len as usize]).to_string_lossy().to_string());
        }
    }

    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// A monotonically increasing counter Windows bumps on every clipboard change.
///
/// Comparing it is far cheaper than reading the clipboard, so the watcher uses
/// it to ignore the echo of its own writes.
pub fn sequence_number() -> u32 {
    // SAFETY: no arguments, no side effects.
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

// Re-exported for the watcher, which needs the message-loop types.
pub(crate) use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, CW_USEDEFAULT, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WNDCLASSW, WM_CLIPBOARDUPDATE, WM_DESTROY,
};
pub(crate) use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
pub(crate) use windows::Win32::System::LibraryLoader::GetModuleHandleW;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_html_fragment() {
        let raw = "Version:0.9\r\nStartHTML:0000000105\r\n<html><body>\
                   <!--StartFragment--><b>hi</b><!--EndFragment--></body></html>";
        assert_eq!(extract_html_fragment(raw), "<b>hi</b>");
    }

    #[test]
    fn falls_back_when_markers_are_missing() {
        let raw = "Version:0.9\r\n<p>plain</p>";
        assert_eq!(extract_html_fragment(raw), "<p>plain</p>");
    }

    #[test]
    fn handles_payload_without_any_markup() {
        assert_eq!(extract_html_fragment("nothing here"), "nothing here");
    }

    #[test]
    fn sequence_number_is_readable() {
        // Should not panic and should return something on a real desktop.
        let _ = sequence_number();
    }
}
