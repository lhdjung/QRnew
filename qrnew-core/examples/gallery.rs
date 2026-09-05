//! Writes one PNG per style combination, for looking at.

use qrnew_core::*;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: gallery <dir>");
    let logo = {
        // A rounded blue square with a white ring, as a stand-in logo.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" rx="24" fill="#1b6ef3"/><circle cx="50" cy="50" r="26" fill="none" stroke="#fff" stroke-width="12"/></svg>"##;
        svg.as_bytes().to_vec()
    };

    let data = "https://github.com/lhdjung/QRnew";
    let mut cases: Vec<(String, QrStyle)> = Vec::new();

    for module in [ModuleShape::Square, ModuleShape::Rounded, ModuleShape::Dot] {
        for finder in [FinderShape::Square, FinderShape::Rounded] {
            cases.push((
                format!("{module:?}-{finder:?}").to_lowercase(),
                QrStyle {
                    module,
                    finder: Finder {
                        shape: finder,
                        ..Finder::default()
                    },
                    ..QrStyle::default()
                },
            ));
        }
    }

    for clearing in [Clearing::Square, Clearing::Rounded, Clearing::Circle] {
        cases.push((
            format!("logo-{clearing:?}").to_lowercase(),
            QrStyle {
                module: ModuleShape::Rounded,
                finder: Finder {
                    shape: FinderShape::Rounded,
                    ring: Some(Rgb::new(27, 110, 243)),
                    center: Some(Rgb::new(20, 30, 50)),
                },
                logo: Some(Logo {
                    size: 0.26,
                    padding: 0.75,
                    clearing,
                    ..Logo::new(logo.clone())
                }),
                ..QrStyle::default()
            },
        ));
    }

    for (name, style) in cases {
        match render_png(data, ErrorCorrection::Quartile, &style, 12) {
            Ok(png) => std::fs::write(format!("{dir}/{name}.png"), png).unwrap(),
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
}
