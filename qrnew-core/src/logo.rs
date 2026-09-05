// SPDX-License-Identifier: MPL-2.0

//! Recognizing a logo image and working out where it sits.

use crate::style::{Clearing, Logo};

/// How much of the corner a [`Clearing::Rounded`] area rounds away, as a
/// fraction of its half-width.
const CLEARING_ROUNDING: f32 = 0.5;

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

/// Encodes bytes as base64 with the standard alphabet, for a `data:` URL.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let block = chunk.iter().enumerate().fold(0u32, |block, (i, &byte)| {
            block | (byte as u32) << (16 - 8 * i)
        });

        // Three bytes make four characters; a short chunk pads the rest.
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(block >> (18 - 6 * i) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }

    out
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
    clearing: Clearing,
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
            clearing: logo.clearing,
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
    ///
    /// Measured on the bounding box even for a round clearing, which errs on
    /// the side of reporting more loss than there is.
    pub fn area_fraction(&self) -> f32 {
        let side = self.clear_half * 2.0;
        let modules = self.center * 2.0;
        (side * side) / (modules * modules)
    }

    /// Whether the module at (`x`, `y`) is inside the cleared area, and so
    /// should not be drawn.
    ///
    /// The module's cell is a unit square. Since every clearing shape is
    /// convex and holds the center, the cell meets the shape exactly when the
    /// point of the cell nearest the center does.
    pub fn covers(&self, x: u32, y: u32) -> bool {
        let nearest = |along: u32| self.center.clamp(along as f32, along as f32 + 1.0);
        let dx = (nearest(x) - self.center).abs();
        let dy = (nearest(y) - self.center).abs();
        let half = self.clear_half;

        match self.clearing {
            Clearing::Square => dx <= half && dy <= half,
            Clearing::Circle => dx * dx + dy * dy <= half * half,
            Clearing::Rounded => {
                let radius = half * CLEARING_ROUNDING;
                let corner_x = (dx - (half - radius)).max(0.0);
                let corner_y = (dy - (half - radius)).max(0.0);
                dx <= half
                    && dy <= half
                    && corner_x * corner_x + corner_y * corner_y <= radius * radius
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_worked_examples_from_rfc_4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

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
