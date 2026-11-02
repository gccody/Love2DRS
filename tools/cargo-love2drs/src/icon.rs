//! Prepares the app icon for each platform.

use std::path::Path;

use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};

/// Loads the icon, which has to be square because both platforms crop it.
pub fn load(path: &Path) -> Result<RgbaImage, String> {
    let icon = image::open(path)
        .map_err(|e| format!("can't read the icon {}: {e}", path.display()))?
        .to_rgba8();
    if icon.width() != icon.height() {
        return Err(format!(
            "the icon {} is {}x{}, but it has to be square",
            path.display(),
            icon.width(),
            icon.height()
        ));
    }
    Ok(icon)
}

pub fn resized(icon: &RgbaImage, size: u32) -> RgbaImage {
    imageops::resize(icon, size, size, FilterType::Lanczos3)
}

/// The icon scaled to `content` pixels in the middle of a transparent `canvas`.
pub fn centered(icon: &RgbaImage, canvas: u32, content: u32) -> RgbaImage {
    let mut out = RgbaImage::new(canvas, canvas);
    let offset = i64::from((canvas - content) / 2);
    imageops::overlay(&mut out, &resized(icon, content), offset, offset);
    out
}

/// The color of the icon's corner, on white if it's see-through.
pub fn corner_color(icon: &RgbaImage) -> [u8; 4] {
    flattened_pixel(*icon.get_pixel(0, 0)).0
}

/// The 1024x1024 icon iOS wants. The App Store rejects icons with transparency,
/// so anything see-through is put on white.
pub fn app_store_icon(icon: &RgbaImage) -> RgbaImage {
    let mut out = resized(icon, 1024);
    for pixel in out.pixels_mut() {
        *pixel = flattened_pixel(*pixel);
    }
    out
}

fn flattened_pixel(Rgba([r, g, b, a]): Rgba<u8>) -> Rgba<u8> {
    let over_white = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
    Rgba([over_white(r), over_white(g), over_white(b), 255])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparency_is_flattened_onto_white() {
        assert_eq!(
            flattened_pixel(Rgba([0, 0, 0, 0])),
            Rgba([255, 255, 255, 255])
        );
        assert_eq!(
            flattened_pixel(Rgba([10, 20, 30, 255])),
            Rgba([10, 20, 30, 255])
        );
        assert_eq!(flattened_pixel(Rgba([0, 0, 0, 128])).0[3], 255);
    }

    #[test]
    fn app_store_icons_are_opaque_and_full_size() {
        let icon = app_store_icon(&RgbaImage::new(16, 16));
        assert_eq!(icon.dimensions(), (1024, 1024));
        assert!(icon.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn centering_leaves_a_transparent_margin() {
        let opaque = RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255]));
        let out = centered(&opaque, 12, 6);
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        assert_eq!(out.get_pixel(6, 6).0[3], 255);
    }
}
