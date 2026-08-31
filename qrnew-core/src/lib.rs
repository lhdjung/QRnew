// SPDX-License-Identifier: MPL-2.0

//! The QR pipeline behind QRnew.
//!
//! Everything a QR code looks like is decided in one place, [`Qr::new`], which
//! emits an SVG. The preview displays that SVG and the exports rasterize it, so
//! what the user sees and what they save cannot drift apart. Module shapes,
//! custom finder patterns and logo insets live here for the same reason.
//!
//! No GUI toolkit is involved, which also means the rules can be tested without
//! opening a window.

mod draw;
mod logo;
mod read;
mod style;

use std::fmt;

use qrcode::QrCode;
use resvg::tiny_skia;
use resvg::usvg;

pub use crate::logo::ImageFormat;
pub use crate::read::{ReadError, read};
pub use crate::style::{
    Clearing, DEFAULT_QUIET_ZONE, ErrorCorrection, Finder, FinderShape, Logo, ModuleShape, QrStyle,
    Rgb,
};

use crate::logo::Placement;

/// The largest share of a code that a logo may cover, counting the blank
/// padding around it.
///
/// The `High` error correction level is usually quoted as surviving 30% damage,
/// which is where the tempting round number comes from. Measured instead of
/// quoted, by rendering codes and reading them back with `rqrr`, decoding
/// starts failing just past 19% — consistently, across code sizes. This sits a
/// good step below that, since a code that only just decodes from a perfect
/// rendering has nothing left over for the printing, lighting and camera angle
/// of an actual scan.
pub const MAX_LOGO_AREA: f32 = 0.15;

/// How far a logo has to stay from the edge of the code, in modules.
///
/// The finder patterns are seven modules wide with a one-module separator
/// around them. Damage there is not something error correction can undo: a
/// scanner that cannot find those three squares never gets as far as reading
/// the data.
const FINDER_CLEARANCE: f32 = draw::FINDER_SIZE as f32 + 1.0;

/// A raster image, as straight (non-premultiplied) RGBA bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub enum QrError {
    /// The data could not be encoded, usually because it is too long for the
    /// chosen error correction level.
    Encode(qrcode::types::QrError),
    /// The code was generated but could not be turned into pixels.
    Render(String),
    /// The logo would leave the code unreadable, or is not an image at all.
    Logo(LogoError),
}

/// Why a logo was turned down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogoError {
    /// The bytes are not a PNG, JPEG, GIF, WebP or SVG image.
    UnknownFormat,
    /// [`Logo::size`] is not a fraction of the code's width above zero.
    Size(f32),
    /// [`Logo::padding`] is negative.
    Padding(f32),
    /// The logo covers more of the code than error correction can rebuild.
    TooLarge { area: f32, max: f32 },
    /// The logo reaches into a finder pattern, the part of a code that has to
    /// stay intact for a scanner to recognize it at all.
    CoversFinder,
}

impl fmt::Display for LogoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat => {
                write!(f, "the logo is not a PNG, JPEG, GIF, WebP or SVG image")
            }
            Self::Size(size) => write!(f, "the logo's size must be between 0 and 1, not {size}"),
            Self::Padding(padding) => {
                write!(f, "the logo's padding cannot be negative, and {padding} is")
            }
            Self::TooLarge { area, max } => write!(
                f,
                "the logo covers {:.0}% of the code, above the {:.0}% error correction can rebuild",
                area * 100.0,
                max * 100.0,
            ),
            Self::CoversFinder => write!(
                f,
                "the logo reaches into a corner marker; shrink it, or add data to make the code bigger"
            ),
        }
    }
}

impl std::error::Error for LogoError {}

impl fmt::Display for QrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => write!(f, "cannot encode this input: {error}"),
            Self::Render(reason) => write!(f, "cannot render the QR code: {reason}"),
            Self::Logo(error) => write!(f, "cannot place the logo: {error}"),
        }
    }
}

impl std::error::Error for QrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            Self::Render(_) => None,
            Self::Logo(error) => Some(error),
        }
    }
}

impl From<qrcode::types::QrError> for QrError {
    fn from(error: qrcode::types::QrError) -> Self {
        Self::Encode(error)
    }
}

impl From<LogoError> for QrError {
    fn from(error: LogoError) -> Self {
        Self::Logo(error)
    }
}

/// A generated QR code, held as the SVG that every output format derives from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qr {
    svg: String,
    size: u32,
    ec: ErrorCorrection,
}

impl Qr {
    /// Encodes `data` and draws it in the given style.
    ///
    /// A logo brings three rules with it, because a code that does not scan is
    /// not worth the picture in the middle of it:
    ///
    /// - Error correction is raised to [`ErrorCorrection::High`], whatever `ec`
    ///   asks for. [`Qr::error_correction`] reports what was actually used.
    /// - The logo and its padding may cover at most [`MAX_LOGO_AREA`] of the
    ///   code.
    /// - The logo has to stay clear of the three finder patterns in the
    ///   corners, which no amount of error correction can replace.
    ///
    /// A logo that breaks one of the last two is refused rather than quietly
    /// shrunk, since only the caller knows whether to give up the size or the
    /// picture.
    pub fn new(data: &str, ec: ErrorCorrection, style: &QrStyle) -> Result<Self, QrError> {
        let ec = if style.logo.is_some() {
            ErrorCorrection::High
        } else {
            ec
        };

        let code = QrCode::with_error_correction_level(data.as_bytes(), ec.into())?;
        let modules = code.width() as u32;
        let size = modules + 2 * style.quiet_zone;

        if let Some(logo) = &style.logo {
            check_logo(logo, modules)?;
        }

        Ok(Self {
            svg: draw::draw(&code.into_colors(), modules, size, style),
            size,
            ec,
        })
    }

    /// The code as an SVG document.
    pub fn svg(&self) -> &str {
        &self.svg
    }

    /// Consumes the code, returning its SVG document.
    pub fn into_svg(self) -> String {
        self.svg
    }

    /// Width of the code in modules, quiet zone included.
    pub fn size_in_modules(&self) -> u32 {
        self.size
    }

    /// The error correction level the code was built at, which a logo raises
    /// past what was asked for.
    pub fn error_correction(&self) -> ErrorCorrection {
        self.ec
    }

    /// Rasterizes the code to a PNG at `scale` pixels per module.
    pub fn to_png(&self, scale: u32) -> Result<Vec<u8>, QrError> {
        self.rasterize(scale)?
            .encode_png()
            .map_err(|error| QrError::Render(error.to_string()))
    }

    /// Rasterizes the code to RGBA bytes at `scale` pixels per module.
    pub fn to_rgba(&self, scale: u32) -> Result<Raster, QrError> {
        let pixmap = self.rasterize(scale)?;
        let pixels = pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect();

        Ok(Raster {
            width: pixmap.width(),
            height: pixmap.height(),
            pixels,
        })
    }

    fn rasterize(&self, scale: u32) -> Result<tiny_skia::Pixmap, QrError> {
        if scale == 0 {
            return Err(QrError::Render("scale must be at least 1".to_owned()));
        }

        let tree = usvg::Tree::from_str(&self.svg, &usvg::Options::default())
            .map_err(|error| QrError::Render(error.to_string()))?;
        let side = self
            .size
            .checked_mul(scale)
            .ok_or_else(|| QrError::Render(format!("scale {scale} is too large")))?;
        let mut pixmap = tiny_skia::Pixmap::new(side, side)
            .ok_or_else(|| QrError::Render(format!("cannot allocate a {side}×{side} image")))?;

        let factor = side as f32 / tree.size().width();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(factor, factor),
            &mut pixmap.as_mut(),
        );

        Ok(pixmap)
    }
}

/// Checks a logo against the rules documented on [`Qr::new`].
fn check_logo(logo: &Logo, modules: u32) -> Result<(), LogoError> {
    // Written as a positive test so that a NaN, which compares false against
    // everything, falls into the error rather than out of it.
    let size_is_a_fraction = logo.size > 0.0 && logo.size < 1.0;
    if !size_is_a_fraction {
        return Err(LogoError::Size(logo.size));
    }

    let padding_is_a_length = logo.padding >= 0.0;
    if !padding_is_a_length {
        return Err(LogoError::Padding(logo.padding));
    }
    if ImageFormat::detect(&logo.image).is_none() {
        return Err(LogoError::UnknownFormat);
    }

    let placement = Placement::new(logo, modules);
    let area = placement.area_fraction();
    if area > MAX_LOGO_AREA {
        return Err(LogoError::TooLarge {
            area,
            max: MAX_LOGO_AREA,
        });
    }
    if placement.margin() < FINDER_CLEARANCE {
        return Err(LogoError::CoversFinder);
    }

    Ok(())
}

/// Generates a styled QR code as an SVG document.
pub fn render_svg(data: &str, ec: ErrorCorrection, style: &QrStyle) -> Result<String, QrError> {
    Qr::new(data, ec, style).map(Qr::into_svg)
}

/// Generates a styled QR code as a PNG at `scale` pixels per module.
pub fn render_png(
    data: &str,
    ec: ErrorCorrection,
    style: &QrStyle,
    scale: u32,
) -> Result<Vec<u8>, QrError> {
    Qr::new(data, ec, style)?.to_png(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);
    const GREEN: Rgb = Rgb::new(0, 255, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 255);

    /// A version 1 code, 21 modules wide, is the smallest there is.
    const SHORT: &str = "hello";
    /// Long enough to need a code wide enough for a large logo to fit inside
    /// the finder patterns.
    const LONG: &str =
        "https://example.org/a-path-long-enough-to-need-a-bigger-code-than-hello-does";

    fn styled() -> QrStyle {
        QrStyle {
            dark: RED,
            light: BLUE,
            ..QrStyle::default()
        }
    }

    /// A solid square of `color`, as a PNG, standing in for a real logo.
    fn logo_image(color: Rgb) -> Vec<u8> {
        let mut pixmap = tiny_skia::Pixmap::new(16, 16).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(color.r, color.g, color.b, 255));
        pixmap.encode_png().unwrap()
    }

    /// Color of the module at (`x`, `y`), sampled at its center. Coordinates
    /// include the quiet zone, matching the SVG's own grid.
    fn module_color(raster: &Raster, scale: u32, x: u32, y: u32) -> Rgb {
        let px = x * scale + scale / 2;
        let py = y * scale + scale / 2;
        let offset = ((py * raster.width + px) * 4) as usize;
        assert_eq!(
            raster.pixels[offset + 3],
            255,
            "module ({x}, {y}) is opaque"
        );
        Rgb::new(
            raster.pixels[offset],
            raster.pixels[offset + 1],
            raster.pixels[offset + 2],
        )
    }

    #[test]
    fn quiet_zone_is_part_of_the_document() {
        // "hello" fits in a version 1 code, which is 21 modules wide.
        let qr = Qr::new(SHORT, ErrorCorrection::Medium, &QrStyle::default()).unwrap();

        assert_eq!(qr.size_in_modules(), 21 + 2 * DEFAULT_QUIET_ZONE);
        assert!(qr.svg().contains(r#"viewBox="0 0 29 29""#), "{}", qr.svg());
        assert!(qr.svg().contains(r#"width="232""#), "{}", qr.svg());
    }

    #[test]
    fn style_colors_reach_the_document() {
        let svg = render_svg(SHORT, ErrorCorrection::Medium, &styled()).unwrap();

        assert!(svg.contains(r##"fill="#ff0000""##), "{svg}");
        assert!(svg.contains(r##"fill="#0000ff""##), "{svg}");
        assert!(!svg.contains("#000000"), "{svg}");
    }

    #[test]
    fn raster_follows_the_requested_scale() {
        let qr = Qr::new(SHORT, ErrorCorrection::Medium, &QrStyle::default()).unwrap();

        for scale in [1, 4, 10] {
            let raster = qr.to_rgba(scale).unwrap();
            assert_eq!(raster.width, qr.size_in_modules() * scale);
            assert_eq!(raster.height, raster.width);
            assert_eq!(raster.pixels.len() as u32, raster.width * raster.height * 4);
        }
    }

    /// The whole point of the crate: the SVG the preview shows and the pixels
    /// the export writes describe the same modules, in the same places.
    #[test]
    fn modules_land_where_the_svg_puts_them() {
        let scale = 6;
        let style = styled();
        let qr = Qr::new(SHORT, ErrorCorrection::Medium, &style).unwrap();
        let raster = qr.to_rgba(scale).unwrap();
        let quiet = style.quiet_zone;

        // The margin is blank.
        assert_eq!(module_color(&raster, scale, 0, 0), BLUE);
        assert_eq!(module_color(&raster, scale, quiet - 1, quiet - 1), BLUE);

        // The top left finder pattern: a 7×7 dark ring around a light ring
        // around a 3×3 dark core.
        assert_eq!(module_color(&raster, scale, quiet, quiet), RED);
        assert_eq!(module_color(&raster, scale, quiet + 1, quiet + 1), BLUE);
        assert_eq!(module_color(&raster, scale, quiet + 3, quiet + 3), RED);
        assert_eq!(module_color(&raster, scale, quiet + 6, quiet), RED);

        // The separator right of it, and the bottom right corner, which no
        // finder pattern reaches.
        assert_eq!(module_color(&raster, scale, quiet + 7, quiet), BLUE);
        let last = qr.size_in_modules() - quiet - 1;
        assert_eq!(module_color(&raster, scale, last, last), BLUE);
    }

    #[test]
    fn png_output_is_a_png() {
        let png = render_png(SHORT, ErrorCorrection::High, &QrStyle::default(), 10).unwrap();

        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn oversized_input_is_reported_as_an_encoding_error() {
        let too_long = "a".repeat(4000);
        let error = Qr::new(&too_long, ErrorCorrection::High, &QrStyle::default()).unwrap_err();

        assert!(matches!(error, QrError::Encode(_)), "{error}");
    }

    #[test]
    fn scale_of_zero_is_rejected() {
        let qr = Qr::new(SHORT, ErrorCorrection::Medium, &QrStyle::default()).unwrap();

        assert!(matches!(qr.to_png(0), Err(QrError::Render(_))));
    }

    /// A scanner reads the color at the middle of a module, so a shape may look
    /// however it likes as long as it covers its own cell's center and nothing
    /// else's. This is the property that keeps a styled code readable, and it
    /// is checked against the plain square rendering module by module.
    #[test]
    fn every_module_shape_keeps_the_matrix_it_was_given() {
        let scale = 9;
        let quiet = QrStyle::default().quiet_zone;
        let square = Qr::new(LONG, ErrorCorrection::Medium, &QrStyle::default())
            .unwrap()
            .to_rgba(scale)
            .unwrap();
        let modules = square.width / scale - 2 * quiet;

        for shape in [ModuleShape::Rounded, ModuleShape::Dot] {
            let style = QrStyle {
                module: shape,
                ..QrStyle::default()
            };
            let styled = Qr::new(LONG, ErrorCorrection::Medium, &style)
                .unwrap()
                .to_rgba(scale)
                .unwrap();

            for y in 0..modules {
                for x in 0..modules {
                    let (sx, sy) = (x + quiet, y + quiet);
                    assert_eq!(
                        module_color(&styled, scale, sx, sy),
                        module_color(&square, scale, sx, sy),
                        "module ({x}, {y}) differs under {shape:?}",
                    );
                }
            }
        }
    }

    /// Every finder shape has to leave a dark ring around a light ring around a
    /// dark middle, since that ratio is what a scanner scans for.
    #[test]
    fn every_finder_shape_keeps_the_ring_a_scanner_looks_for() {
        let scale = 9;
        let quiet = QrStyle::default().quiet_zone;

        for shape in [FinderShape::Square, FinderShape::Rounded] {
            let style = QrStyle {
                finder: Finder {
                    shape,
                    ..Finder::default()
                },
                ..QrStyle::default()
            };
            let qr = Qr::new(SHORT, ErrorCorrection::Medium, &style).unwrap();
            let raster = qr.to_rgba(scale).unwrap();
            let at = |x: u32, y: u32| module_color(&raster, scale, x + quiet, y + quiet);

            for (fx, fy) in [(0, 0), (14, 0), (0, 14)] {
                // Across the middle of the pattern: ring, gap, center, gap,
                // ring.
                assert_eq!(at(fx, fy + 3), Rgb::BLACK, "{shape:?} ring at {fx},{fy}");
                assert_eq!(at(fx + 1, fy + 3), Rgb::WHITE, "{shape:?} gap at {fx},{fy}");
                assert_eq!(
                    at(fx + 3, fy + 3),
                    Rgb::BLACK,
                    "{shape:?} core at {fx},{fy}"
                );
                assert_eq!(at(fx + 5, fy + 3), Rgb::WHITE, "{shape:?} gap at {fx},{fy}");
                assert_eq!(
                    at(fx + 6, fy + 3),
                    Rgb::BLACK,
                    "{shape:?} ring at {fx},{fy}"
                );
            }
        }
    }

    #[test]
    fn finder_patterns_can_carry_their_own_colors() {
        let style = QrStyle {
            finder: Finder {
                shape: FinderShape::Rounded,
                ring: Some(GREEN),
                center: Some(BLUE),
            },
            ..QrStyle::default()
        };
        let scale = 9;
        let qr = Qr::new(SHORT, ErrorCorrection::Medium, &style).unwrap();
        let raster = qr.to_rgba(scale).unwrap();
        let quiet = style.quiet_zone;

        assert_eq!(module_color(&raster, scale, quiet, quiet + 3), GREEN);
        assert_eq!(module_color(&raster, scale, quiet + 3, quiet + 3), BLUE);
        // The rest of the matrix keeps the code's own dark color.
        assert_eq!(
            module_color(&raster, scale, quiet + 6, quiet + 8),
            Rgb::BLACK
        );
    }

    /// Straight edges want to snap to the pixel grid; curves want the opposite.
    #[test]
    fn crisp_edges_are_asked_for_only_when_everything_is_straight() {
        let square = render_svg(SHORT, ErrorCorrection::Medium, &QrStyle::default()).unwrap();
        assert!(square.contains("crispEdges"), "{square}");

        for style in [
            QrStyle {
                module: ModuleShape::Dot,
                ..QrStyle::default()
            },
            QrStyle {
                finder: Finder {
                    shape: FinderShape::Rounded,
                    ..Finder::default()
                },
                ..QrStyle::default()
            },
        ] {
            let svg = render_svg(SHORT, ErrorCorrection::Medium, &style).unwrap();
            assert!(!svg.contains("crispEdges"), "{svg}");
        }
    }

    fn with_logo(logo: Logo) -> QrStyle {
        QrStyle {
            logo: Some(logo),
            ..QrStyle::default()
        }
    }

    #[test]
    fn logo_is_embedded_as_a_data_url() {
        let style = with_logo(Logo::new(logo_image(GREEN)));
        let svg = render_svg(LONG, ErrorCorrection::Medium, &style).unwrap();

        assert!(svg.contains(r#"<image "#), "{svg}");
        assert!(svg.contains("href=\"data:image/png;base64,iVBOR"), "{svg}");
    }

    /// The logo has to survive the whole round trip: embedded as base64, parsed
    /// back out by `usvg` and drawn by `resvg`, with the modules beneath it
    /// left out rather than painted over.
    #[test]
    fn logo_replaces_the_modules_it_covers() {
        let scale = 9;
        let style = with_logo(Logo {
            size: 0.3,
            ..Logo::new(logo_image(GREEN))
        });
        let qr = Qr::new(LONG, ErrorCorrection::Medium, &style).unwrap();
        let raster = qr.to_rgba(scale).unwrap();
        let middle = qr.size_in_modules() / 2;

        assert_eq!(module_color(&raster, scale, middle, middle), GREEN);

        // Just outside the logo and its padding, the matrix carries on. Some
        // module in that ring has to be dark, or nothing was drawn at all.
        let edge = middle + (0.3 * (qr.size_in_modules() - 2 * style.quiet_zone) as f32) as u32;
        assert!(
            (0..qr.size_in_modules()).any(|y| module_color(&raster, scale, edge, y) == Rgb::BLACK),
            "the column just past the logo is empty",
        );
    }

    #[test]
    fn a_logo_raises_error_correction_to_the_highest_level() {
        let style = with_logo(Logo::new(logo_image(GREEN)));
        let qr = Qr::new(LONG, ErrorCorrection::Low, &style).unwrap();

        assert_eq!(qr.error_correction(), ErrorCorrection::High);

        // Without one, the level asked for is the level used.
        let plain = Qr::new(LONG, ErrorCorrection::Low, &QrStyle::default()).unwrap();
        assert_eq!(plain.error_correction(), ErrorCorrection::Low);
    }

    #[test]
    fn the_default_logo_fits_even_the_smallest_code() {
        let style = with_logo(Logo::new(logo_image(GREEN)));
        let qr = Qr::new(SHORT, ErrorCorrection::High, &style).unwrap();

        assert_eq!(qr.size_in_modules(), 21 + 2 * DEFAULT_QUIET_ZONE);
    }

    #[test]
    fn a_logo_covering_more_than_error_correction_can_rebuild_is_refused() {
        // Wide enough that a logo this size still clears the finder patterns,
        // so the area rule is what it runs into.
        let style = with_logo(Logo {
            size: 0.45,
            padding: 0.0,
            ..Logo::new(logo_image(GREEN))
        });
        let error = Qr::new(LONG, ErrorCorrection::High, &style).unwrap_err();

        assert!(
            matches!(error, QrError::Logo(LogoError::TooLarge { .. })),
            "{error}",
        );
    }

    #[test]
    fn a_logo_reaching_into_a_finder_pattern_is_refused() {
        // Small enough to clear the area rule, so the finder rule is what it
        // runs into on a code this size.
        let style = with_logo(Logo {
            size: 0.3,
            ..Logo::new(logo_image(GREEN))
        });
        let error = Qr::new(SHORT, ErrorCorrection::High, &style).unwrap_err();
        assert!(
            matches!(error, QrError::Logo(LogoError::CoversFinder)),
            "{error}",
        );

        // The same logo on a code with room for it goes through.
        assert!(Qr::new(LONG, ErrorCorrection::High, &style).is_ok());
    }

    #[test]
    fn a_logo_that_is_not_an_image_is_refused() {
        let style = with_logo(Logo::new(b"this is not an image".to_vec()));
        let error = Qr::new(LONG, ErrorCorrection::High, &style).unwrap_err();

        assert!(
            matches!(error, QrError::Logo(LogoError::UnknownFormat)),
            "{error}",
        );
    }

    #[test]
    fn nonsense_logo_geometry_is_refused() {
        for size in [0.0, -0.5, 1.0, f32::NAN] {
            let style = with_logo(Logo {
                size,
                ..Logo::new(logo_image(GREEN))
            });
            let error = Qr::new(LONG, ErrorCorrection::High, &style).unwrap_err();
            assert!(
                matches!(error, QrError::Logo(LogoError::Size(_))),
                "{error}"
            );
        }

        let style = with_logo(Logo {
            padding: -1.0,
            ..Logo::new(logo_image(GREEN))
        });
        let error = Qr::new(LONG, ErrorCorrection::High, &style).unwrap_err();
        assert!(
            matches!(error, QrError::Logo(LogoError::Padding(_))),
            "{error}"
        );
    }
}
