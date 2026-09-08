// SPDX-License-Identifier: MPL-2.0

//! Recognizing a logo image and working out where it sits.

use crate::style::Logo;

/// The image formats a logo may be in. They are exactly the ones `resvg` can
/// draw, which matters because the preview and the exports both go through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    WebP,
    Svg,
}

impl ImageFormat {
    /// The MIME type, as it appears in a `data:` URL.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
            Self::Svg => "image/svg+xml",
        }
    }

    /// Identifies an image by its leading bytes.
    ///
    /// File names lie and are often absent, so nothing here looks at one.
    pub fn detect(data: &[u8]) -> Option<Self> {
        if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(Self::Png);
        }
        if data.starts_with(b"\xff\xd8\xff") {
            return Some(Self::Jpeg);
        }
        if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            return Some(Self::Gif);
        }
        if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
            return Some(Self::WebP);
        }
        if looks_like_svg(data) {
            return Some(Self::Svg);
        }
        None
    }
}

/// Whether the bytes open like an SVG document: optionally a byte order mark,
/// then whitespace, then a declaration, a comment, a doctype or the root tag.
fn looks_like_svg(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let head = data
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |start| &data[start..]);

    [&b"<?xml"[..], b"<!--", b"<!DOCTYPE", b"<svg"]
        .iter()
        .any(|prefix| head.starts_with(prefix))
}

/// Where a logo sits, in module coordinates that do not include the quiet zone.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    /// Center of the code, which is also the center of the logo.
    center: f32,
    /// Half the width of the image itself.
    image_half: f32,
    /// Half the width of the area cleared of modules: the image plus its
    /// padding.
    clear_half: f32,
}

impl Placement {
    /// Places `logo` in the middle of a code `modules` wide.
    pub fn new(logo: &Logo, modules: u32) -> Self {
        let modules = modules as f32;
        let image_half = logo.size * modules / 2.0;

        Self {
            center: modules / 2.0,
            image_half,
            clear_half: image_half + logo.padding,
        }
    }

    /// The image's own box, as `(x, y, side)`.
    pub fn image_box(&self) -> (f32, f32, f32) {
        (
            self.center - self.image_half,
            self.center - self.image_half,
            self.image_half * 2.0,
        )
    }

    /// Distance from the cleared area to the nearest edge of the code, in
    /// modules. The finder patterns and their separators occupy the outermost
    /// eight, so this is what keeps the logo off them.
    pub fn margin(&self) -> f32 {
        self.center - self.clear_half
    }

    /// Share of the code's area that the cleared box takes up.
    pub fn area_fraction(&self) -> f32 {
        let side = self.clear_half * 2.0;
        let modules = self.center * 2.0;
        (side * side) / (modules * modules)
    }

    /// Whether the module at (`x`, `y`) is inside the cleared square, and so
    /// should not be drawn.
    ///
    /// The module's cell is a unit square, and the cleared area holds the
    /// center, so the cell meets it exactly when the point of the cell nearest
    /// the center does.
    pub fn covers(&self, x: u32, y: u32) -> bool {
        let nearest = |along: u32| self.center.clamp(along as f32, along as f32 + 1.0);
        let dx = (nearest(x) - self.center).abs();
        let dy = (nearest(y) - self.center).abs();

        dx <= self.clear_half && dy <= self.clear_half
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_told_apart_by_their_magic_bytes() {
        assert_eq!(
            ImageFormat::detect(b"\x89PNG\r\n\x1a\n..."),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            ImageFormat::detect(b"\xff\xd8\xff\xe0..."),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(ImageFormat::detect(b"GIF89a..."), Some(ImageFormat::Gif));
        assert_eq!(
            ImageFormat::detect(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(ImageFormat::WebP)
        );
        assert_eq!(
            ImageFormat::detect(b"  \n<svg xmlns=''/>"),
            Some(ImageFormat::Svg)
        );
        assert_eq!(
            ImageFormat::detect(b"<?xml version='1.0'?><svg/>"),
            Some(ImageFormat::Svg)
        );

        assert_eq!(ImageFormat::detect(b""), None);
        assert_eq!(ImageFormat::detect(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(ImageFormat::detect(b"not an image at all"), None);
    }
}
