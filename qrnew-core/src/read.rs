// SPDX-License-Identifier: MPL-2.0

//! Reading a QR code back out of an image file.
//!
//! This is the pipeline in [`crate::draw`] run backwards, and it reuses the
//! same machinery: the file is wrapped in a `data:` URL and handed to `resvg`,
//! exactly as a logo inset is, which means every format a logo may be in is
//! also a format a code may be read from. The decoding itself is `rqrr`'s.

use std::fmt;

use resvg::tiny_skia;

use crate::logo::ImageFormat;
use crate::raster;

/// Smallest and largest the image is scaled to before decoding, as the length
/// of its longer side in pixels.
///
/// A code saved at one pixel per module carries too little for a decoder to
/// lock onto, and a photograph from a modern camera carries far more than it
/// needs; both ends cost only time.
const MIN_SIDE: f32 = 512.0;
const MAX_SIDE: f32 = 2048.0;

/// Why an image could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The file is not a PNG, JPEG, GIF, WebP or SVG image.
    NotAnImage,
    /// The file says it is an image but could not be opened.
    Damaged(String),
    /// The image opened, but holds no QR code that could be located.
    NoCode,
    /// A code was located but could not be decoded, usually because the image
    /// is too blurred, too skewed or too small.
    Unreadable(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnImage => write!(f, "this is not a PNG, JPEG, GIF, WebP or SVG image"),
            Self::Damaged(reason) => write!(f, "this image cannot be opened: {reason}"),
            Self::NoCode => write!(f, "no QR code was found in this image"),
            Self::Unreadable(reason) => write!(f, "the QR code could not be read: {reason}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Reads the first QR code found in an encoded image.
///
/// The image may be a PNG, JPEG, GIF, WebP or SVG, including an SVG this crate
/// wrote. If it holds more than one code, the first one located wins.
///
/// A word on expectations: this reads a rendering, not a photograph. Clean
/// images — a saved file, a screenshot, an export from some other tool — read
/// reliably. A picture taken at an angle, in poor light or out of focus is a
/// harder problem than this does, and a phone camera will often succeed where
/// this returns [`ReadError::NoCode`].
pub fn read(image: &[u8]) -> Result<String, ReadError> {
    let pixmap = rasterize(image)?;
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);

    // `rqrr` works on brightness alone, which is also what a scanner sees.
    let luma: Vec<u8> = pixmap
        .pixels()
        .iter()
        .map(|pixel| {
            let color = pixel.demultiply();
            let [r, g, b] = [
                color.red() as u32,
                color.green() as u32,
                color.blue() as u32,
            ];
            ((r * 299 + g * 587 + b * 114) / 1000) as u8
        })
        .collect();

    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * width + x]);

    let grids = prepared.detect_grids();
    if grids.is_empty() {
        return Err(ReadError::NoCode);
    }

    // Any one of the located grids may be the readable one.
    let mut last = None;
    for grid in &grids {
        match grid.decode() {
            Ok((_, content)) => return Ok(content),
            Err(error) => last = Some(error),
        }
    }

    Err(ReadError::Unreadable(last.map_or_else(
        || "unknown reason".to_owned(),
        |error| error.to_string(),
    )))
}

/// Draws an encoded image into a pixmap, scaled into the range a decoder is
/// comfortable with and laid over white so that transparency reads as
/// background rather than as ink.
fn rasterize(image: &[u8]) -> Result<tiny_skia::Pixmap, ReadError> {
    let format = ImageFormat::detect(image).ok_or(ReadError::NotAnImage)?;
    let href = raster::href(image, format);

    // How big the image is is not something this crate parses out of a file
    // header, so it is asked before anything is drawn. `usvg` drops an
    // `<image>` whose data it cannot make sense of, which is what an image
    // that is not the one it claims to be looks like from here.
    let (width, height) = raster::natural_size(&href)
        .filter(|(width, height)| *width > 0.0 && *height > 0.0)
        .ok_or_else(|| ReadError::Damaged("the image data could not be decoded".to_owned()))?;

    let scale = fit(width.max(height));
    let size = (raster::whole(width * scale), raster::whole(height * scale));

    raster::draw(&href, size, Some(tiny_skia::Color::WHITE))
        .ok_or_else(|| ReadError::Damaged("the image has no area".to_owned()))
}

/// The scale that brings an image's longer side into the range a decoder is
/// comfortable with.
fn fit(longest: f32) -> f32 {
    if longest < MIN_SIDE {
        MIN_SIDE / longest
    } else if longest > MAX_SIDE {
        MAX_SIDE / longest
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorCorrection, Qr, QrStyle};
    use resvg::usvg;

    const DATA: &str = "https://github.com/lhdjung/QRnew";

    fn code() -> Qr {
        Qr::new(DATA, ErrorCorrection::Medium, &QrStyle::default()).unwrap()
    }

    /// A code saved at one pixel per module has no detail to spare, and is
    /// what the lower end of the scaling range exists for.
    #[test]
    fn a_code_saved_at_one_pixel_per_module_still_reads() {
        let png = code().to_png(1).unwrap();

        assert_eq!(read(&png).as_deref(), Ok(DATA));
    }

    /// The other end: an image far larger than a decoder needs is scaled down
    /// rather than worked through at full size, and not squashed on the way.
    #[test]
    fn an_oversized_image_is_scaled_down_in_proportion() {
        let mut huge = tiny_skia::Pixmap::new(2400, 1600).unwrap();
        huge.fill(tiny_skia::Color::WHITE);

        let scaled = rasterize(&huge.encode_png().unwrap()).unwrap();

        assert_eq!(scaled.width(), MAX_SIDE as u32);
        assert_eq!(scaled.height(), (MAX_SIDE * 2.0 / 3.0).round() as u32);
    }

    /// And a code that goes through that scaling still reads afterwards.
    #[test]
    fn a_code_read_through_the_downscaling_survives_it() {
        let qr = code();
        // Just past the limit, which is the cheapest way to land beyond it.
        let scale = MAX_SIDE as u32 / qr.size_in_modules() + 1;
        assert!(qr.size_in_modules() * scale > MAX_SIDE as u32);

        assert_eq!(read(&qr.to_png(scale).unwrap()).as_deref(), Ok(DATA));
    }

    #[test]
    fn a_file_that_is_not_an_image_is_refused() {
        assert_eq!(read(b"this is not an image"), Err(ReadError::NotAnImage));
    }

    #[test]
    fn an_image_that_lies_about_being_a_png_is_refused() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(b"and then nothing that belongs in one");

        assert!(matches!(read(&bytes), Err(ReadError::Damaged(_))));
    }

    #[test]
    fn an_image_with_no_code_in_it_is_reported_as_empty() {
        let mut blank = tiny_skia::Pixmap::new(600, 400).unwrap();
        blank.fill(tiny_skia::Color::WHITE);

        assert_eq!(read(&blank.encode_png().unwrap()), Err(ReadError::NoCode));
    }

    /// Transparency has to read as background. A code saved with a transparent
    /// quiet zone would otherwise arrive as dark-on-dark.
    #[test]
    fn a_transparent_background_reads_as_light() {
        let side = 400;
        let mut pixmap = tiny_skia::Pixmap::new(side, side).unwrap();
        let tree = usvg::Tree::from_str(code().svg(), &usvg::Options::default()).unwrap();
        let factor = side as f32 / tree.size().width();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(factor, factor),
            &mut pixmap.as_mut(),
        );

        // Knock the light modules out, leaving only the dark ones on nothing.
        for pixel in pixmap.pixels_mut() {
            if pixel.red() > 128 {
                *pixel = tiny_skia::PremultipliedColorU8::TRANSPARENT;
            }
        }

        assert_eq!(read(&pixmap.encode_png().unwrap()).as_deref(), Ok(DATA));
    }
}
