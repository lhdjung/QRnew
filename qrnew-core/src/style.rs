// SPDX-License-Identifier: MPL-2.0

//! Everything about a code's appearance that the data itself does not decide.

/// Width of the blank margin around the code, in modules. Four is the minimum
/// the QR standard asks for.
pub const DEFAULT_QUIET_ZONE: u32 = 4;

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

    /// Formats the color as `#rrggbb`.
    ///
    /// Lower case, everywhere and without exception: this string is what the
    /// interface puts in front of somebody as *their* colour, and a hex code
    /// in capitals reads as machine output rather than as a value they chose.
    /// The SVG is written with the same function, so a saved file and the
    /// window agree character for character.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// How much of the code can be damaged or obscured while still scanning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
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

/// The outline given to each individual module.
///
/// Scanners look at the color at the center of a module, so any shape that
/// covers its center and stays inside its cell is safe. All three below do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ModuleShape {
    /// Full square cells, the shape the standard describes.
    #[default]
    Square,
    /// Square cells whose corners are rounded off wherever no neighbor fills
    /// them in, so runs of modules merge into smooth blobs.
    Rounded,
    /// A separate circle per module, filling the cell edge to edge.
    Dot,
}

/// The outline given to the three big squares in the corners.
///
/// These are the finder patterns, which a scanner locates before it reads
/// anything else. They are drawn separately from the rest of the matrix so a
/// playful [`ModuleShape`] never breaks them up, and so they can be recolored on
/// their own.
///
/// There is deliberately no fully circular option, tempting as it is: a scanner
/// recognizes a finder pattern by the 1:1:3:1:1 run it leaves along any line
/// through it, and a circular ring only produces that on the one line through
/// its middle. `rqrr` cannot find a code with circular finders at any
/// resolution. [`Rounded`] gets most of the way there and does scan.
///
/// [`Rounded`]: FinderShape::Rounded
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FinderShape {
    /// A 7×7 square ring around a 3×3 square, as the standard draws it.
    #[default]
    Square,
    /// Softly rounded corners on the ring, and a round dot in the middle.
    Rounded,
}

/// Shape and color of the finder patterns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Finder {
    pub shape: FinderShape,
    /// Color of the outer ring. Falls back to [`QrStyle::dark`].
    pub ring: Option<Rgb>,
    /// Color of the inner square or dot. Falls back to the ring's color.
    pub center: Option<Rgb>,
}

impl Finder {
    /// Color of the outer ring, resolved against the code's own dark color.
    pub fn ring_color(&self, dark: Rgb) -> Rgb {
        self.ring.unwrap_or(dark)
    }

    /// Color of the inner square or dot, resolved the same way.
    pub fn center_color(&self, dark: Rgb) -> Rgb {
        self.center.unwrap_or_else(|| self.ring_color(dark))
    }
}

/// Outline of the area a logo clears out of the matrix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Clearing {
    #[default]
    Square,
    Rounded,
    Circle,
}

/// An image placed in the middle of the code.
///
/// The modules underneath are left out rather than painted over, so nothing
/// shows through a logo with transparent parts. Error correction is what makes
/// the code survive the loss; see [`Qr::new`](crate::Qr::new) for the rules
/// that get enforced.
#[derive(Debug, Clone, PartialEq)]
pub struct Logo {
    /// The encoded image. PNG, JPEG, GIF, WebP and SVG are recognized, and the
    /// format is detected from the bytes rather than from a file name.
    pub image: Vec<u8>,
    /// Side of the logo, as a fraction of the code's width. The quiet zone
    /// does not count towards that width.
    pub size: f32,
    /// Blank margin kept around the logo, in modules.
    pub padding: f32,
    /// Outline of the cleared area.
    pub clearing: Clearing,
}

impl Logo {
    /// A sixth of the code's width. That covers under 3% of the modules, and
    /// stays clear of the finder patterns even on the smallest code there is.
    pub const DEFAULT_SIZE: f32 = 1.0 / 6.0;
    /// Half a module of air around the logo.
    pub const DEFAULT_PADDING: f32 = 0.5;

    /// Wraps an encoded image in the default placement.
    pub fn new(image: Vec<u8>) -> Self {
        Self {
            image,
            size: Self::DEFAULT_SIZE,
            padding: Self::DEFAULT_PADDING,
            clearing: Clearing::default(),
        }
    }
}

/// Everything about a code's appearance that the data itself does not decide.
#[derive(Debug, Clone, PartialEq)]
pub struct QrStyle {
    /// Color of the set modules.
    pub dark: Rgb,
    /// Color of the unset modules and of the quiet zone.
    pub light: Rgb,
    /// Width of the blank margin, in modules.
    pub quiet_zone: u32,
    /// Outline of each module.
    pub module: ModuleShape,
    /// Shape and color of the three finder patterns.
    pub finder: Finder,
    /// An image in the middle of the code, if any.
    pub logo: Option<Logo>,
}

impl Default for QrStyle {
    fn default() -> Self {
        Self {
            dark: Rgb::BLACK,
            light: Rgb::WHITE,
            quiet_zone: DEFAULT_QUIET_ZONE,
            module: ModuleShape::default(),
            finder: Finder::default(),
            logo: None,
        }
    }
}
