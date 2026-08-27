// SPDX-License-Identifier: MPL-2.0

//! Every style has to survive a real decoder.
//!
//! The unit tests check that each shape covers the middle of its own module,
//! which is the property a scanner relies on. These tests check the conclusion
//! rather than the reasoning: they export the code and read it back through
//! `qrnew_core::read`, which decodes with `rqrr` — an independent
//! implementation that knows nothing about how the code was drawn.

use qrnew_core::{
    Clearing, ErrorCorrection, Finder, FinderShape, Logo, MAX_LOGO_AREA, ModuleShape, Qr, QrStyle,
    ReadError, Rgb, read,
};

const DATA: &str = "https://github.com/lhdjung/QRnew";

/// Pixels per module to export at before reading back.
///
/// More than one, deliberately. A style can decode at one resolution and fail
/// at another — rounding a finder pattern's corners by a full module used to
/// read fine at ten pixels per module and fail below it — so a single scale
/// here proves much less than it looks like it does. Ten is the lowest this
/// crate claims, and what the app exports at; the second is there to catch
/// anything that depends on resolution in the other direction.
const SCALES: [u32; 2] = [10, 16];

/// A rounded blue square with a white ring, standing in for a real logo.
const LOGO: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">"##,
    r##"<rect width="100" height="100" rx="24" fill="#1b6ef3"/>"##,
    r##"<circle cx="50" cy="50" r="26" fill="none" stroke="#fff" stroke-width="12"/></svg>"##,
);

fn logo() -> Vec<u8> {
    LOGO.as_bytes().to_vec()
}

/// Exports a code as a PNG and reads it back, the way a user would who saved
/// the file and opened it again.
fn scan(qr: &Qr, scale: u32) -> Result<String, ReadError> {
    read(&qr.to_png(scale).unwrap())
}

fn shapes() -> impl Iterator<Item = (ModuleShape, FinderShape)> {
    [ModuleShape::Square, ModuleShape::Rounded, ModuleShape::Dot]
        .into_iter()
        .flat_map(|module| {
            [FinderShape::Square, FinderShape::Rounded]
                .into_iter()
                .map(move |finder| (module, finder))
        })
}

#[test]
fn every_combination_of_shapes_scans() {
    for (module, finder) in shapes() {
        let style = QrStyle {
            module,
            finder: Finder {
                shape: finder,
                ..Finder::default()
            },
            ..QrStyle::default()
        };
        let qr = Qr::new(DATA, ErrorCorrection::Medium, &style).unwrap();

        for scale in SCALES {
            assert_eq!(
                scan(&qr, scale).as_deref(),
                Ok(DATA),
                "{module:?} + {finder:?} at {scale} px per module",
            );
        }
    }
}

#[test]
fn every_combination_of_shapes_scans_with_a_logo_in_the_way() {
    for (module, finder) in shapes() {
        for clearing in [Clearing::Square, Clearing::Rounded, Clearing::Circle] {
            let style = QrStyle {
                module,
                finder: Finder {
                    shape: finder,
                    ring: Some(Rgb::new(27, 110, 243)),
                    ..Finder::default()
                },
                logo: Some(Logo {
                    size: 0.26,
                    padding: 0.75,
                    clearing,
                    ..Logo::new(logo())
                }),
                ..QrStyle::default()
            };
            let qr = Qr::new(DATA, ErrorCorrection::Low, &style).unwrap();

            for scale in SCALES {
                assert_eq!(
                    scan(&qr, scale).as_deref(),
                    Ok(DATA),
                    "{module:?} + {finder:?} + {clearing:?} at {scale} px per module",
                );
            }
        }
    }
}

/// The point of [`MAX_LOGO_AREA`] is that anything it lets through still
/// scans. A logo sized to sit right on the limit is the case that tests it.
#[test]
fn the_largest_logo_the_rules_allow_still_scans() {
    // A code wide enough that the area limit is what bites, not the finder
    // patterns. `size` is a fraction of the width, and the limit is on area.
    let data = DATA.repeat(4);
    let size = MAX_LOGO_AREA.sqrt() - 0.01;
    let style = QrStyle {
        logo: Some(Logo {
            size,
            padding: 0.0,
            ..Logo::new(logo())
        }),
        ..QrStyle::default()
    };
    let qr = Qr::new(&data, ErrorCorrection::Low, &style).unwrap();

    for scale in SCALES {
        assert_eq!(scan(&qr, scale).as_deref(), Ok(data.as_str()), "at {scale}");
    }
}

/// Colors have to keep enough contrast for a decoder, and the code is drawn
/// dark-on-light either way.
#[test]
fn a_recolored_code_scans() {
    let style = QrStyle {
        dark: Rgb::new(20, 30, 50),
        light: Rgb::new(245, 245, 235),
        module: ModuleShape::Rounded,
        finder: Finder {
            shape: FinderShape::Rounded,
            ring: Some(Rgb::new(27, 110, 243)),
            center: Some(Rgb::new(20, 30, 50)),
        },
        ..QrStyle::default()
    };
    let qr = Qr::new(DATA, ErrorCorrection::Quartile, &style).unwrap();

    for scale in SCALES {
        assert_eq!(scan(&qr, scale).as_deref(), Ok(DATA), "at {scale}");
    }
}

/// The app saves SVG as well as PNG, and the reader takes an SVG through the
/// same path as a raster. Saving a code and opening it again has to work in
/// either format.
#[test]
fn an_svg_export_reads_back() {
    let style = QrStyle {
        module: ModuleShape::Rounded,
        finder: Finder {
            shape: FinderShape::Rounded,
            ..Finder::default()
        },
        logo: Some(Logo::new(LOGO.as_bytes().to_vec())),
        ..QrStyle::default()
    };
    let qr = Qr::new(DATA, ErrorCorrection::Medium, &style).unwrap();

    assert_eq!(read(qr.svg().as_bytes()).as_deref(), Ok(DATA));
}
