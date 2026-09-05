// SPDX-License-Identifier: MPL-2.0

//! Turning a module matrix into an SVG document.
//!
//! Coordinates here are module units throughout: the document's `viewBox`
//! carries the whole grid, so nothing below has to know about pixels. Matrix
//! positions run from 0 to `modules`, and the quiet zone is added on the way
//! into the document.

use std::fmt::Write as _;

use qrcode::Color;

use crate::logo::{self, ImageFormat, Placement};
use crate::style::{FinderShape, Logo, ModuleShape, QrStyle, Rgb};

/// Nominal size of one module in the `width`/`height` attributes of a
/// generated SVG. The geometry itself lives in the `viewBox`; this only decides
/// how big the file looks to a viewer that ignores it.
const SVG_MODULE_PX: u32 = 8;

/// Width of a finder pattern, in modules.
pub const FINDER_SIZE: u32 = 7;

/// Corner radius of a rounded module, in modules. At half a module a lone
/// module comes out as a circle and a run comes out as a capsule.
const MODULE_RADIUS: f32 = 0.5;

/// Corner radius of a rounded finder pattern's outer ring, in modules.
///
/// This one is not a matter of taste. A scanner finds a code by sweeping lines
/// across it and looking for the 1:1:3:1:1 run a finder pattern produces;
/// rounding the outer corners shortens the runs near the top and bottom of the
/// ring and eventually breaks that signature.
///
/// How much is too much depends on the resolution the code is read at, which is
/// why the round-trip tests sweep both. At 1.25 modules a code fails to decode
/// however large it is drawn. At 1.0 it decodes from ten pixels per module up
/// and is unreliable below — a trap, since it looks fine at whatever single size
/// you happen to test. At 0.8 and below that fragility is gone. The crate claims
/// ten either way, and the margin is what absorbs the printing and camera angle
/// the tests do not model, so this sits at 0.75.
///
/// The hole and the center are unconstrained; the center is a full circle.
const FINDER_RADIUS: f32 = 0.75;

/// Writes the matrix as an SVG document.
///
/// The second half of the return is where the logo's `<image>` begins, when
/// there is one and it could be drawn. It is the last element in the document,
/// so that offset is also the length of the same document without it — which
/// is what [`Qr::svg_without_inset`](crate::Qr::svg_without_inset) hands to a
/// caller drawing the picture as a layer of its own.
pub fn draw(colors: &[Color], modules: u32, size: u32, style: &QrStyle) -> (String, Option<usize>) {
    let grid = Grid {
        colors,
        modules,
        logo: style
            .logo
            .as_ref()
            .map(|logo| Placement::new(logo, modules)),
    };
    let quiet = style.quiet_zone as f32;
    let mut svg = String::with_capacity(colors.len() * 8);

    open_document(&mut svg, size, style);
    modules_path(&mut svg, &grid, quiet, style);
    finder_paths(&mut svg, modules, quiet, style);
    // Measured rather than assumed: `logo_image` writes nothing for a style
    // whose logo has no placement or no readable format, and an offset naming
    // an element that is not there would truncate the document at `</svg>`
    // and call the result "without the inset".
    let logo_at = match &style.logo {
        Some(logo) => {
            let at = svg.len();
            logo_image(&mut svg, logo, &grid, quiet);
            (svg.len() > at).then_some(at)
        }
        None => None,
    };
    svg.push_str("</svg>");

    (svg, logo_at)
}

/// The module matrix, with the parts that are drawn some other way — or not at
/// all — already taken out.
struct Grid<'a> {
    colors: &'a [Color],
    modules: u32,
    logo: Option<Placement>,
}

impl Grid<'_> {
    /// Whether the module at (`x`, `y`) belongs to the matrix path: dark, on
    /// the grid, outside the finder patterns, and not cleared for a logo.
    ///
    /// Off-grid positions count as light, which lets the shape code ask about
    /// neighbors without special-casing the border.
    fn drawn(&self, x: i64, y: i64) -> bool {
        let modules = self.modules as i64;
        if x < 0 || y < 0 || x >= modules || y >= modules {
            return false;
        }

        let (x, y) = (x as u32, y as u32);
        if is_finder(x, y, self.modules) {
            return false;
        }
        if self.logo.is_some_and(|logo| logo.covers(x, y)) {
            return false;
        }

        self.colors[(y * self.modules + x) as usize] == Color::Dark
    }
}

/// Whether (`x`, `y`) falls inside one of the three finder patterns.
fn is_finder(x: u32, y: u32, modules: u32) -> bool {
    finder_origins(modules)
        .into_iter()
        .any(|(fx, fy)| x >= fx && x < fx + FINDER_SIZE && y >= fy && y < fy + FINDER_SIZE)
}

/// Top left corners of the three finder patterns, in matrix coordinates.
fn finder_origins(modules: u32) -> [(u32, u32); 3] {
    let far = modules - FINDER_SIZE;
    [(0, 0), (far, 0), (0, far)]
}

fn open_document(svg: &mut String, size: u32, style: &QrStyle) {
    // Snapping edges to the pixel grid only helps while every edge is straight;
    // on a curve it trades a soft outline for a jagged one.
    let rendering =
        if style.module == ModuleShape::Square && style.finder.shape == FinderShape::Square {
            r#" shape-rendering="crispEdges""#
        } else {
            ""
        };

    write!(
        svg,
        concat!(
            r#"<?xml version="1.0" standalone="yes"?>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1""#,
            r#" width="{px}" height="{px}" viewBox="0 0 {size} {size}"{rendering}>"#,
            r#"<rect width="{size}" height="{size}" fill="{light}"/>"#,
        ),
        px = size * SVG_MODULE_PX,
        size = size,
        rendering = rendering,
        light = style.light.to_hex(),
    )
    .expect("writing to a String cannot fail");
}

/// Writes every module outside the finder patterns as a single path.
fn modules_path(svg: &mut String, grid: &Grid<'_>, quiet: f32, style: &QrStyle) {
    let mut path = String::new();

    match style.module {
        ModuleShape::Square => square_modules(&mut path, grid, quiet),
        ModuleShape::Rounded => rounded_modules(&mut path, grid, quiet),
        ModuleShape::Dot => dot_modules(&mut path, grid, quiet),
    }

    fill_path(svg, &path, style.dark);
}

/// Merges horizontal runs of modules into one rectangle each, which keeps the
/// document small for a dense code.
fn square_modules(path: &mut String, grid: &Grid<'_>, quiet: f32) {
    for y in 0..grid.modules {
        let mut x = 0;
        while x < grid.modules {
            if !grid.drawn(x as i64, y as i64) {
                x += 1;
                continue;
            }

            let start = x;
            while x < grid.modules && grid.drawn(x as i64, y as i64) {
                x += 1;
            }

            let run = x - start;
            let left = start as f32 + quiet;
            let top = y as f32 + quiet;
            write!(path, "M{left} {top}h{run}v1h-{run}z").expect("writing to a String cannot fail");
        }
    }
}

/// Rounds off each corner that no neighbor fills in, so that runs and blocks of
/// modules merge into smooth shapes while lone modules come out round.
fn rounded_modules(path: &mut String, grid: &Grid<'_>, quiet: f32) {
    for y in 0..grid.modules {
        for x in 0..grid.modules {
            let (mx, my) = (x as i64, y as i64);
            if !grid.drawn(mx, my) {
                continue;
            }

            // A corner survives only where both edges meeting at it are free.
            let free = |dx: i64, dy: i64| {
                if grid.drawn(mx + dx, my) || grid.drawn(mx, my + dy) {
                    0.0
                } else {
                    MODULE_RADIUS
                }
            };
            let corners = [free(-1, -1), free(1, -1), free(1, 1), free(-1, 1)];

            rounded_rect(path, x as f32 + quiet, y as f32 + quiet, 1.0, corners);
        }
    }
}

/// Draws each module as a circle filling its cell.
fn dot_modules(path: &mut String, grid: &Grid<'_>, quiet: f32) {
    for y in 0..grid.modules {
        for x in 0..grid.modules {
            if grid.drawn(x as i64, y as i64) {
                let center = 0.5 + quiet;
                circle(path, x as f32 + center, y as f32 + center, 0.5, true);
            }
        }
    }
}

/// Writes the three finder patterns: a ring in one path, the centers in another
/// so that they can carry their own color.
fn finder_paths(svg: &mut String, modules: u32, quiet: f32, style: &QrStyle) {
    let (mut rings, mut centers) = (String::new(), String::new());

    for (fx, fy) in finder_origins(modules) {
        let (x, y) = (fx as f32 + quiet, fy as f32 + quiet);

        match style.finder.shape {
            FinderShape::Square => {
                rounded_rect(&mut rings, x, y, 7.0, [0.0; 4]);
                rounded_rect_reversed(&mut rings, x + 1.0, y + 1.0, 5.0, 0.0);
                rounded_rect(&mut centers, x + 2.0, y + 2.0, 3.0, [0.0; 4]);
            }
            FinderShape::Rounded => {
                rounded_rect(&mut rings, x, y, 7.0, [FINDER_RADIUS; 4]);
                rounded_rect_reversed(&mut rings, x + 1.0, y + 1.0, 5.0, FINDER_RADIUS);
                circle(&mut centers, x + 3.5, y + 3.5, 1.5, true);
            }
        }
    }

    fill_path(svg, &rings, style.finder.ring_color(style.dark));
    fill_path(svg, &centers, style.finder.center_color(style.dark));
}

/// Embeds the logo as a `data:` URL, sized to fit its box without distortion.
fn logo_image(svg: &mut String, logo: &Logo, grid: &Grid<'_>, quiet: f32) {
    let Some(placement) = grid.logo else {
        return;
    };
    let Some(format) = ImageFormat::detect(&logo.image) else {
        return;
    };
    let (x, y, side) = placement.image_box();

    write!(
        svg,
        concat!(
            r#"<image x="{x}" y="{y}" width="{side}" height="{side}""#,
            r#" preserveAspectRatio="xMidYMid meet""#,
            r#" href="data:{mime};base64,{data}"/>"#,
        ),
        x = x + quiet,
        y = y + quiet,
        side = side,
        mime = format.mime(),
        data = logo::base64(&logo.image),
    )
    .expect("writing to a String cannot fail");
}

/// Wraps path data in a filled `<path>`, or writes nothing if there is none.
fn fill_path(svg: &mut String, path: &str, fill: Rgb) {
    if path.is_empty() {
        return;
    }

    write!(svg, r#"<path fill="{}" d="{path}"/>"#, fill.to_hex())
        .expect("writing to a String cannot fail");
}

/// Appends a square with independently rounded corners, wound clockwise.
///
/// Corners are given as radii, starting at the top left and going clockwise. A
/// radius of zero leaves the corner sharp. Winding decides what happens where
/// two shapes in one path overlap: clockwise fills, and
/// [`rounded_rect_reversed`] punches a hole.
fn rounded_rect(path: &mut String, x: f32, y: f32, side: f32, corners: [f32; 4]) {
    let [top_left, top_right, bottom_right, bottom_left] = corners;
    let (right, bottom) = (x + side, y + side);

    write!(path, "M{} {y}", x + top_left).expect("writing to a String cannot fail");
    edge(path, 'H', right - top_right, top_right, top_right);
    edge(
        path,
        'V',
        bottom - bottom_right,
        -bottom_right,
        bottom_right,
    );
    edge(path, 'H', x + bottom_left, -bottom_left, -bottom_left);
    edge(path, 'V', y + top_left, top_left, -top_left);
    path.push('z');

    /// One straight edge and the arc turning off it, given as the offset from
    /// the end of the edge to the end of the arc.
    fn edge(path: &mut String, axis: char, to: f32, dx: f32, dy: f32) {
        write!(path, "{axis}{to}").expect("writing to a String cannot fail");

        let radius = dx.abs().max(dy.abs());
        if radius > 0.0 {
            write!(path, "a{radius} {radius} 0 0 1 {dx} {dy}")
                .expect("writing to a String cannot fail");
        }
    }
}

/// Appends a square with uniformly rounded corners, wound counter-clockwise so
/// that it cuts a hole in whatever it sits inside.
fn rounded_rect_reversed(path: &mut String, x: f32, y: f32, side: f32, radius: f32) {
    let (right, bottom) = (x + side, y + side);

    write!(path, "M{x} {}", y + radius).expect("writing to a String cannot fail");
    edge(path, 'V', bottom - radius, radius, radius);
    edge(path, 'H', right - radius, radius, -radius);
    edge(path, 'V', y + radius, -radius, -radius);
    edge(path, 'H', x + radius, -radius, radius);
    path.push('z');

    fn edge(path: &mut String, axis: char, to: f32, dx: f32, dy: f32) {
        write!(path, "{axis}{to}").expect("writing to a String cannot fail");

        let radius = dx.abs().max(dy.abs());
        if radius > 0.0 {
            write!(path, "a{radius} {radius} 0 0 0 {dx} {dy}")
                .expect("writing to a String cannot fail");
        }
    }
}

/// Appends a circle, as two half-circle arcs.
fn circle(path: &mut String, cx: f32, cy: f32, radius: f32, clockwise: bool) {
    let sweep = u8::from(clockwise);
    let diameter = radius * 2.0;

    write!(
        path,
        "M{} {cy}a{radius} {radius} 0 1 {sweep} {diameter} 0a{radius} {radius} 0 1 {sweep} -{diameter} 0z",
        cx - radius,
    )
    .expect("writing to a String cannot fail");
}
