// SPDX-License-Identifier: MPL-2.0

//! The QR pipeline behind QRnew.
//!
//! Everything a QR code looks like is decided in one place, [`Qr::new`], which
//! emits an SVG. The preview displays that SVG and the exports rasterize it, so
//! what the user sees and what they save cannot drift apart. Module shapes,
//! custom finder patterns and logo insets belong here too, for the same reason.
//!
//! No GUI toolkit is involved, which also means the rules can be tested without
//! opening a window.

use std::fmt::{self, Write as _};

use qrcode::QrCode;
use resvg::tiny_skia;
use resvg::usvg;

/// Width of the blank margin around the code, in modules. Four is the minimum
/// the QR standard asks for.
pub const DEFAULT_QUIET_ZONE: u32 = 4;

/// Nominal size of one module in the `width`/`height` attributes of a generated
/// SVG. The geometry itself is in module units, held by the `viewBox`; this only
/// decides how big the file looks to a viewer that ignores the `viewBox`.
const SVG_MODULE_PX: u32 = 8;

/// An opaque 8-bit-per-channel color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Formats the color as `#RRGGBB`.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// How much of the code can be damaged or obscured while still scanning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ErrorCorrection {
    Low,
    #[default]
    Medium,
    Quartile,
    High,
}

impl From<ErrorCorrection> for qrcode::EcLevel {
    fn from(level: ErrorCorrection) -> Self {
        match level {
            ErrorCorrection::Low => Self::L,
            ErrorCorrection::Medium => Self::M,
            ErrorCorrection::Quartile => Self::Q,
            ErrorCorrection::High => Self::H,
        }
    }
}

/// Everything about a code's appearance that the data itself does not decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QrStyle {
    /// Color of the set modules.
    pub dark: Rgb,
    /// Color of the unset modules and of the quiet zone.
    pub light: Rgb,
    /// Width of the blank margin, in modules.
    pub quiet_zone: u32,
}

impl Default for QrStyle {
    fn default() -> Self {
        Self {
            dark: Rgb::BLACK,
            light: Rgb::WHITE,
            quiet_zone: DEFAULT_QUIET_ZONE,
        }
    }
}

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
}

impl fmt::Display for QrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => write!(f, "cannot encode this input: {error}"),
            Self::Render(reason) => write!(f, "cannot render the QR code: {reason}"),
        }
    }
}

impl std::error::Error for QrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            Self::Render(_) => None,
        }
    }
}

impl From<qrcode::types::QrError> for QrError {
    fn from(error: qrcode::types::QrError) -> Self {
        Self::Encode(error)
    }
}

/// A generated QR code, held as the SVG that every output format derives from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qr {
    svg: String,
    size: u32,
}

impl Qr {
    /// Encodes `data` and draws it in the given style.
    pub fn new(data: &str, ec: ErrorCorrection, style: &QrStyle) -> Result<Self, QrError> {
        let code = QrCode::with_error_correction_level(data.as_bytes(), ec.into())?;
        let modules = code.width() as u32;
        let size = modules + 2 * style.quiet_zone;

        Ok(Self {
            svg: draw_svg(&code.into_colors(), modules, size, style),
            size,
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

/// Writes the module matrix as an SVG document, one path for all dark modules.
///
/// The `viewBox` is in module units, so the drawing below never has to know
/// about pixels. Horizontal runs of dark modules are merged into single path
/// segments, which keeps the document small for dense codes.
fn draw_svg(colors: &[qrcode::Color], modules: u32, size: u32, style: &QrStyle) -> String {
    let px = size * SVG_MODULE_PX;
    let mut svg = String::with_capacity(colors.len() * 8);

    write!(
        svg,
        concat!(
            r#"<?xml version="1.0" standalone="yes"?>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1""#,
            r#" width="{px}" height="{px}" viewBox="0 0 {size} {size}""#,
            r#" shape-rendering="crispEdges">"#,
            r#"<rect width="{size}" height="{size}" fill="{light}"/>"#,
            r#"<path fill="{dark}" d=""#,
        ),
        px = px,
        size = size,
        light = style.light.to_hex(),
        dark = style.dark.to_hex(),
    )
    .expect("writing to a String cannot fail");

    let is_dark = |x: u32, y: u32| colors[(y * modules + x) as usize] == qrcode::Color::Dark;

    for y in 0..modules {
        let mut x = 0;
        while x < modules {
            if !is_dark(x, y) {
                x += 1;
                continue;
            }

            let start = x;
            while x < modules && is_dark(x, y) {
                x += 1;
            }

            let run = x - start;
            let left = start + style.quiet_zone;
            let top = y + style.quiet_zone;
            write!(svg, "M{left} {top}h{run}v1h-{run}z").expect("writing to a String cannot fail");
        }
    }

    svg.push_str(r#""/></svg>"#);
    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 255);

    fn styled() -> QrStyle {
        QrStyle {
            dark: RED,
            light: BLUE,
            ..QrStyle::default()
        }
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
        let qr = Qr::new("hello", ErrorCorrection::Medium, &QrStyle::default()).unwrap();

        assert_eq!(qr.size_in_modules(), 21 + 2 * DEFAULT_QUIET_ZONE);
        assert!(qr.svg().contains(r#"viewBox="0 0 29 29""#), "{}", qr.svg());
        assert!(qr.svg().contains(r#"width="232""#), "{}", qr.svg());
    }

    #[test]
    fn style_colors_reach_the_document() {
        let svg = render_svg("hello", ErrorCorrection::Medium, &styled()).unwrap();

        assert!(svg.contains(r##"fill="#FF0000""##), "{svg}");
        assert!(svg.contains(r##"fill="#0000FF""##), "{svg}");
        assert!(!svg.contains("#000000"), "{svg}");
    }

    #[test]
    fn raster_follows_the_requested_scale() {
        let qr = Qr::new("hello", ErrorCorrection::Medium, &QrStyle::default()).unwrap();

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
        let qr = Qr::new("hello", ErrorCorrection::Medium, &style).unwrap();
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
        let png = render_png("hello", ErrorCorrection::High, &QrStyle::default(), 10).unwrap();

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
        let qr = Qr::new("hello", ErrorCorrection::Medium, &QrStyle::default()).unwrap();

        assert!(matches!(qr.to_png(0), Err(QrError::Render(_))));
    }
}
