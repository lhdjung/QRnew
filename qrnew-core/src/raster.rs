// SPDX-License-Identifier: MPL-2.0

//! Encoded images in, pixmaps out.
//!
//! Both directions of the crate's image work come through here — a picture on
//! its way into a code, and a code on its way out of a picture — and neither
//! of them parses an image format. `resvg` is already here to draw codes, and
//! it draws every format a logo may be in, so the whole of this module is one
//! trick used twice: wrap the file in an SVG document of its own, ask `usvg`
//! how big it turned out to be, and draw it at whatever size is wanted.

use resvg::tiny_skia;
use resvg::usvg;

use crate::logo::{self, ImageFormat};

/// An encoded image as a `data:` URL, which is how a document refers to one.
///
/// Worth encoding once and passing around: an image is measured before it is
/// drawn, and base64 of a photograph is not a string to build twice.
pub fn href(image: &[u8], format: ImageFormat) -> String {
    format!("data:{};base64,{}", format.mime(), logo::base64(image))
}

/// The size the image declares, which is its own rather than whatever document
/// it is sitting in.
///
/// `None` means the bytes are not the picture they claim to be.
pub fn natural_size(href: &str) -> Option<(f32, f32)> {
    fn found_in(group: &usvg::Group) -> Option<(f32, f32)> {
        group.children().iter().find_map(|node| match node {
            usvg::Node::Image(image) => Some((image.size().width(), image.size().height())),
            usvg::Node::Group(group) => found_in(group),
            _ => None,
        })
    }

    // Nothing is drawn from this document, and an `<image>` reports the size of
    // the picture rather than the size it was given, so one pixel square is as
    // good a box as any.
    let tree = usvg::Tree::from_str(&document(1, 1, href), &usvg::Options::default()).ok()?;
    let size = found_in(tree.root())?;

    decodes(href).then_some(size)
}

/// Whether the bytes really are a picture, which is not a question the size
/// answers.
///
/// `usvg` reads a size out of the file's own header and takes its word for it:
/// eight PNG magic bytes followed by nonsense arrive as an image half a billion
/// pixels wide rather than as no image at all. Nothing short of decoding tells
/// the two apart — 0.45 decoded while parsing and dropped the node, 0.48 leaves
/// it to the render — so draw the thing into a thumbnail with nothing behind
/// it. A picture leaves something in there; a failed decode leaves it clear.
///
/// A wholly transparent image is clear too, and is called damaged rather than
/// blank. It holds no code either way.
fn decodes(href: &str) -> bool {
    const PROBE: (u32, u32) = (8, 8);

    draw(href, PROBE, None)
        .is_some_and(|pixmap| pixmap.pixels().iter().any(|pixel| pixel.alpha() > 0))
}

/// The image drawn at `size`, over `background` if it is given one.
pub fn draw(
    href: &str,
    size: (u32, u32),
    background: Option<tiny_skia::Color>,
) -> Option<tiny_skia::Pixmap> {
    let tree =
        usvg::Tree::from_str(&document(size.0, size.1, href), &usvg::Options::default()).ok()?;

    let mut pixmap = tiny_skia::Pixmap::new(size.0, size.1)?;
    if let Some(color) = background {
        pixmap.fill(color);
    }
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );

    Some(pixmap)
}

/// An SVG document holding nothing but the image, at the given size.
fn document(width: u32, height: u32, href: &str) -> String {
    format!(
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">"#,
            r#"<image width="{width}" height="{height}" href="{href}"/></svg>"#,
        ),
        width = width,
        height = height,
        href = href,
    )
}

/// One pixmap drawn into another of a different size.
pub fn resample(source: &tiny_skia::Pixmap, size: (u32, u32)) -> Option<tiny_skia::Pixmap> {
    let mut pixmap = tiny_skia::Pixmap::new(size.0, size.1)?;
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    let scale = tiny_skia::Transform::from_scale(
        size.0 as f32 / source.width() as f32,
        size.1 as f32 / source.height() as f32,
    );
    pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, scale, None);

    Some(pixmap)
}

/// A measurement rounded to whole pixels, and never to none.
pub fn whole(measure: f32) -> u32 {
    (measure.round() as u32).max(1)
}
