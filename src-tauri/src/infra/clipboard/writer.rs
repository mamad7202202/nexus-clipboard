//! Writing items back to the system clipboard.

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
        .map_err(|e| Error::Clipboard(format!("cannot write text: {e}")))
}

/// Put both a plain and an HTML flavour on the clipboard, so pasting into a
/// rich editor keeps formatting while a code editor still gets clean text.
pub fn write_html(html: &str, plain: &str) -> Result<()> {
    let mut clipboard = open()?;
    clipboard
        .set_html(html, Some(plain))
        .map_err(|e| Error::Clipboard(format!("cannot write html: {e}")))
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
        .map_err(|e| Error::Clipboard(format!("cannot write image: {e}")))
}

/// Put a file list on the clipboard.
///
/// `arboard` has no CF_HDROP writer, so on Windows we fall back to newline-
/// separated paths — which is what most applications accept, and what the user
/// sees if they paste into a text field.
pub fn write_files(paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Err(Error::invalid("no paths to write"));
    }
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
