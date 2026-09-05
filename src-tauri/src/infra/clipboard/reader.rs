//! Reading the current clipboard into a domain [`Snapshot`].
//!
//! Formats are probed in order of specificity: files, then images, then text.
//! That order matters — copying a PNG from Explorer offers both CF_HDROP and a
//! bitmap, and the file path is the more useful of the two.

use arboard::Clipboard;

use crate::domain::Snapshot;
use crate::error::{Error, Result};
use crate::infra::platform;

/// Images above this size are downscaled before storage. 8K screenshots are
/// rarely worth 40 MB of history.
const MAX_IMAGE_PIXELS: u32 = 4096;

/// Take a snapshot of whatever is on the clipboard right now.
///
/// Returns `Ok(None)` when the clipboard holds nothing we handle (or is empty),
/// which is a normal outcome, not an error.
pub fn read_snapshot() -> Result<Option<Snapshot>> {
    // 1. File paths (Explorer, most file managers).
    if let Some(files) = platform::read_files() {
        if !files.is_empty() {
            return Ok(Some(Snapshot::Files(files)));
        }
    }

    let mut clipboard =
        Clipboard::new().map_err(|e| Error::Clipboard(format!("cannot open clipboard: {e}")))?;

    // 2. Text (with its HTML flavour when one exists).
    //    Checked before images because many apps publish a text fallback
    //    alongside rich content, and text is what the user usually wants.
    match clipboard.get_text() {
        Ok(text) if !text.trim().is_empty() => {
            let html = platform::read_html().filter(|h| !h.trim().is_empty());
            return Ok(Some(Snapshot::Text { text, html }));
        }
        _ => {}
    }

    // 3. Raster image (screenshots, copied pictures).
    if let Ok(image) = clipboard.get_image() {
        let width = image.width as u32;
        let height = image.height as u32;
        if width == 0 || height == 0 {
            return Ok(None);
        }
        let png = encode_png(&image.bytes, width, height)?;
        let (png, width, height) = maybe_downscale(png, width, height)?;
        return Ok(Some(Snapshot::Image { png, width, height }));
    }

    Ok(None)
}

/// `arboard` hands back raw RGBA; the history stores PNG so blobs stay small
/// and the webview can render them directly.
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| Error::Clipboard("image dimensions overflow".into()))?;
    if rgba.len() < expected {
        return Err(Error::Clipboard("truncated image data".into()));
    }

    let buffer = image::RgbaImage::from_raw(width, height, rgba[..expected].to_vec())
        .ok_or_else(|| Error::Clipboard("could not build image buffer".into()))?;

    let mut out = Vec::with_capacity(expected / 4);
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok(out)
}

fn maybe_downscale(png: Vec<u8>, width: u32, height: u32) -> Result<(Vec<u8>, u32, u32)> {
    if width <= MAX_IMAGE_PIXELS && height <= MAX_IMAGE_PIXELS {
        return Ok((png, width, height));
    }

    let img = image::load_from_memory(&png)?;
    let resized = img.resize(MAX_IMAGE_PIXELS, MAX_IMAGE_PIXELS, image::imageops::FilterType::Lanczos3);
    let (w, h) = (resized.width(), resized.height());

    let mut out = Vec::new();
    resized.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok((out, w, h))
}

/// Build a small PNG thumbnail as a data URI for the list view.
///
/// Returns `None` rather than failing: a missing thumbnail degrades the UI
/// slightly, but a failed capture would lose the item entirely.
pub fn thumbnail(png: &[u8], max_edge: u32) -> Option<String> {
    use base64::Engine;

    let img = image::load_from_memory(png).ok()?;
    let thumb = img.thumbnail(max_edge, max_edge);

    let mut out = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;

    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&out)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_rgba(w: u32, h: u32) -> Vec<u8> {
        vec![0x40u8; (w * h * 4) as usize]
    }

    #[test]
    fn encodes_png_from_rgba() {
        let png = encode_png(&solid_rgba(8, 8), 8, 8).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn rejects_truncated_image_data() {
        assert!(encode_png(&[0u8; 4], 8, 8).is_err());
    }

    #[test]
    fn rejects_overflowing_dimensions() {
        assert!(encode_png(&[0u8; 4], u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn downscales_oversized_images() {
        let w = MAX_IMAGE_PIXELS + 512;
        let png = encode_png(&solid_rgba(w, 16), w, 16).unwrap();
        let (_, out_w, _) = maybe_downscale(png, w, 16).unwrap();
        assert!(out_w <= MAX_IMAGE_PIXELS);
    }

    #[test]
    fn builds_thumbnail_data_uri() {
        let png = encode_png(&solid_rgba(64, 64), 64, 64).unwrap();
        let uri = thumbnail(&png, 16).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn thumbnail_of_garbage_is_none() {
        assert!(thumbnail(b"not an image", 16).is_none());
    }
}
