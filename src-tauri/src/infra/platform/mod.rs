//! Platform facade.
//!
//! The rest of the codebase calls these functions and never `#[cfg]`s on the
//! target OS. Windows has a full implementation, Linux has a native X11 /
//! Wayland implementation, and any other target gets honest no-ops so the
//! project still builds and tests run everywhere.

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use self::windows::{foreground_window, paste_into, read_files, read_html, sequence_number, WindowInfo};

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use self::linux::{foreground_window, paste_into, read_files, read_html, sequence_number, WindowInfo};

#[cfg(not(any(windows, target_os = "linux")))]
mod fallback {
    use crate::error::{Error, Result};

    #[derive(Debug, Clone, Default)]
    pub struct WindowInfo {
        pub app: Option<String>,
        pub title: Option<String>,
        pub hwnd: isize,
    }

    pub fn foreground_window() -> WindowInfo {
        WindowInfo::default()
    }

    pub fn paste_into(_hwnd: isize) -> Result<()> {
        Err(Error::platform("paste-back is only implemented on Windows and Linux"))
    }

    pub fn read_html() -> Option<String> {
        None
    }

    pub fn read_files() -> Option<Vec<String>> {
        None
    }

    pub fn sequence_number() -> u32 {
        0
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub use fallback::*;
