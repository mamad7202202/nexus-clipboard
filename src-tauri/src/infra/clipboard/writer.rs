//! Writing items back to the system clipboard.
//!
//! Handles format conversions and Linux persistence (keeping clipboard data
//! alive even after the originating window/app closes).

use arboard::Clipboard;

use crate::error::{Error, Result};

fn open() -> Result<Clipboard> {
    Clipboard::new().map_err(|e| Error::Clipboard(format!("cannot open clipboard: {e}")))
}

/// Put plain text on the clipboard.
pub fn write_text(text: &str) -> Result<()> {
    let mut clipboard = open()?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| Error::Clipboard(format!("cannot write text: {e}")))?;

    #[cfg(target_os = "linux")]
    crate::infra::platform::linux::persist_clipboard_data(
        crate::infra::platform::linux::ClipboardPayload::Text(text.to_string()),
    );

    Ok(())
}

/// Put both a plain and an HTML flavour on the clipboard, so pasting into a
/// rich editor keeps formatting while a code editor still gets clean text.
pub fn write_html(html: &str, plain: &str) -> Result<()> {
    let mut clipboard = open()?;
    clipboard
        .set_html(html, Some(plain))
        .map_err(|e| Error::Clipboard(format!("cannot write html: {e}")))?;

    #[cfg(target_os = "linux")]
    crate::infra::platform::linux::persist_clipboard_data(
        crate::infra::platform::linux::ClipboardPayload::Html {
            text: plain.to_string(),
            html: html.to_string(),
        },
    );

    Ok(())
}

/// Decode a stored PNG and place it on the clipboard as a bitmap.
pub fn write_image(png: &[u8]) -> Result<()> {
    let img = image::load_from_memory(png)?.to_rgba8();
    let (width, height) = img.dimensions();

    let data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(img.into_raw()),
    };

    let mut clipboard = open()?;
    clipboard
        .set_image(data)
        .map_err(|e| Error::Clipboard(format!("cannot write image: {e}")))?;

    #[cfg(target_os = "linux")]
    crate::infra::platform::linux::persist_clipboard_data(
        crate::infra::platform::linux::ClipboardPayload::Image(png.to_vec()),
    );

    Ok(())
}

/// Put a file list on the clipboard.
pub fn write_files(paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Err(Error::invalid("no paths to write"));
    }

    #[cfg(target_os = "linux")]
    {
        crate::infra::platform::linux::persist_clipboard_data(
            crate::infra::platform::linux::ClipboardPayload::Files(paths.to_vec()),
        );
        write_text(&paths.join("\n"))
    }

    #[cfg(not(target_os = "linux"))]
    write_text(&paths.join("\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_list_is_rejected() {
        assert!(write_files(&[]).is_err());
    }

    #[test]
    fn invalid_png_is_rejected() {
        assert!(write_image(b"definitely not a png").is_err());
    }
}
