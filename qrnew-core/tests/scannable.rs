// SPDX-License-Identifier: MPL-2.0

//! Every style has to survive a real decoder.
//!
//! The unit tests check that each shape covers the middle of its own module,
//! which is the property a scanner relies on. These tests check the conclusion
//! rather than the reasoning: they rasterize the code and read it back with
//! `rqrr`, an independent implementation that knows nothing about how it was
//! drawn.

use qrnew_core::{
    Clearing, ErrorCorrection, Finder, FinderShape, Logo, MAX_LOGO_AREA, ModuleShape, Qr, QrStyle,
    Rgb,
};

const DATA: &str = "https://github.com/lhdjung/QRnew";

/// Pixels per module. Enough that a decoder is reading the shapes rather than
/// fighting the resolution.
const SCALE: u32 = 10;

/// A rounded blue square with a white ring, standing in for a real logo.
const LOGO: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">"##,
    r##"<rect width="100" height="100" rx="24" fill="#1b6ef3"/>"##,
    r##"<circle cx="50" cy="50" r="26" fill="none" stroke="#fff" stroke-width="12"/></svg>"##,
);

fn logo() -> Vec<u8> {
    LOGO.as_bytes().to_vec()
}

/// Rasterizes a code and reads it back. Returns what the decoder made of it.
fn scan(qr: &Qr) -> Result<String, String> {
    let raster = qr.to_rgba(SCALE).unwrap();
    let luma: Vec<u8> = raster
        .pixels
        .chunks_exact(4)
        .map(|px| {
            let [r, g, b] = [px[0] as u32, px[1] as u32, px[2] as u32];
            ((r * 299 + g * 587 + b * 114) / 1000) as u8
        })
        .collect();

    let mut image = rqrr::PreparedImage::prepare_from_greyscale(
        raster.width as usize,
        raster.height as usize,
        |x, y| luma[y * raster.width as usize + x],
    );

    let grids = image.detect_grids();
    if grids.is_empty() {
        return Err("no code found".to_owned());
    }

    grids[0]
        .decode()
        .map(|(_, content)| content)
        .map_err(|error| error.to_string())
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

        assert_eq!(scan(&qr).as_deref(), Ok(DATA), "{module:?} + {finder:?}");
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

            assert_eq!(
                scan(&qr).as_deref(),
                Ok(DATA),
                "{module:?} + {finder:?} + {clearing:?}",
            );
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

    assert_eq!(scan(&qr).as_deref(), Ok(data.as_str()));
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

    assert_eq!(scan(&qr).as_deref(), Ok(DATA));
}
