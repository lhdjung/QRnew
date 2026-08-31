// SPDX-License-Identifier: MPL-2.0

//! QRnew's interface, as HTML and CSS over `qrnew-core`.
//!
//! The core is not touched by any of this: `Qr::new` takes the text, the error
//! correction level and a [`QrStyle`], and hands back a document that the
//! preview, the SVG export, the PNG export and the clipboard all come out of.
//! That is the same single-SVG arrangement the libcosmic build had — the only
//! thing that changed is who draws it.
//!
//! The preview reaches the screen as a `data:` URL on an `<img>`, which Blitz
//! parses with `usvg` — `resvg`'s own parser, and the one `qrnew-core` already
//! uses to rasterize. So the bytes on screen are the bytes in the saved file,
//! through the same code, and the documented Blitz gap where CSS cannot reach
//! inside an SVG never applies: `draw.rs` writes every colour as a
//! presentation attribute so that an exported file stands on its own.

use dioxus::prelude::*;
use qrnew_core::{ErrorCorrection, Qr, QrStyle, ReadError, Rgb};

use crate::fl;

/// Resolution of saved and copied images, in pixels per module.
const EXPORT_SCALE: u32 = 10;

/// The colours offered in the picker.
///
/// Two rows of eight: a grey ramp, and hues dark enough to still read as the
/// dark side of a code. There is no colour here that a scanner would struggle
/// with as a foreground on white, which is a decision the picker makes for the
/// person using it rather than one it explains.
const PALETTE: [Rgb; 16] = [
    Rgb::new(0x00, 0x00, 0x00),
    Rgb::new(0x3a, 0x38, 0x36),
    Rgb::new(0x6b, 0x6a, 0x68),
    Rgb::new(0x9a, 0x97, 0x93),
    Rgb::new(0xc9, 0xc6, 0xc2),
    Rgb::new(0xe4, 0xe2, 0xdf),
    Rgb::new(0xf5, 0xf4, 0xf2),
    Rgb::new(0xff, 0xff, 0xff),
    Rgb::new(0x8b, 0x1a, 0x1a),
    Rgb::new(0xb5, 0x4a, 0x0e),
    Rgb::new(0x8a, 0x6d, 0x0a),
    Rgb::new(0x1f, 0x5c, 0x36),
    Rgb::new(0x0f, 0x5c, 0x63),
    Rgb::new(0x1b, 0x3f, 0x8f),
    Rgb::new(0x4b, 0x2a, 0x7a),
    Rgb::new(0x7a, 0x1f, 0x4f),
];

/// Text to start the field with, provided as a root context by `main.rs`.
///
/// It exists for `--fill`, which exists so that the app can be measured with a
/// code on screen without anybody typing one — a GPU renderer's cost is not
/// the same before and after there is something to draw. Nothing provides it
/// in a test, and an absent context means an empty field.
#[derive(Clone)]
pub struct Fill(pub String);

/// Which of the two colour wells has its picker open, if either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Well {
    Dark,
    Light,
}

#[component]
pub fn App() -> Element {
    let mut input = use_signal(|| {
        dioxus_core::try_consume_context::<Fill>().map_or_else(String::new, |fill| fill.0)
    });
    // Bumped when the app — rather than the person — puts text in the field,
    // which happens in exactly one place: after a code is read out of a file.
    //
    // It is the field's `key`, so a bump rebuilds the element instead of
    // updating it, and rebuilding re-runs `autofocus`. That is how the
    // keyboard comes back to the field once the file dialog has closed, which
    // is what `widget::text_input::focus` did in the libcosmic build.
    //
    // The roundabout route is the point: the direct way to ask for the focus
    // is `MountedData::set_focus`, and it takes `doc_mut()` from inside a
    // borrow an event handler is already holding, so it panics with `RefCell
    // already borrowed` from a stack that names neither.
    let mut revision = use_signal(|| 0u32);
    let mut ec = use_signal(|| ErrorCorrection::Medium);
    let mut dark = use_signal(|| Rgb::BLACK);
    let mut light = use_signal(|| Rgb::WHITE);
    let mut read_error = use_signal(|| None::<ReadError>);
    let mut open_well = use_signal(|| None::<Well>);
    let mut about = use_signal(|| false);

    // The one generated code, which everything downstream is a view of. A memo
    // rather than a signal written from four handlers: the inputs say what the
    // code is, so nothing can set them and forget to redraw.
    let code = use_memo(move || {
        let text = input();
        if text.is_empty() {
            return None;
        }
        let style = QrStyle {
            dark: dark(),
            light: light(),
            ..QrStyle::default()
        };
        // `Err` is an input past what the densest code can hold. The libcosmic
        // build showed the placeholder for that and said nothing, and so does
        // this.
        Qr::new(&text, ec(), &style).ok()
    });

    // Kept apart from `code` so that a re-render that changes neither the text
    // nor the colours does not re-encode the SVG into base64.
    let preview = use_memo(move || code().as_ref().map(|qr| data_url(qr.svg())));

    let read_file = move |_| {
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
                .pick_file()
                .await
            else {
                return;
            };
            match std::fs::read(handle.path()) {
                Ok(bytes) => match qrnew_core::read(&bytes) {
                    Ok(text) => {
                        read_error.set(None);
                        input.set(text);
                        *revision.write() += 1;
                    }
                    Err(error) => read_error.set(Some(error)),
                },
                Err(error) => read_error.set(Some(ReadError::Damaged(error.to_string()))),
            }
        });
    };

    let save_png = move |_| {
        let Some(qr) = code() else { return };
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("PNG Image", &["png"])
                .set_file_name("qrcode.png")
                .save_file()
                .await
            else {
                return;
            };
            if let Ok(png) = qr.to_png(EXPORT_SCALE) {
                let _ = std::fs::write(handle.path(), png);
            }
        });
    };

    let save_svg = move |_| {
        let Some(qr) = code() else { return };
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("SVG Image", &["svg"])
                .set_file_name("qrcode.svg")
                .save_file()
                .await
            else {
                return;
            };
            let _ = std::fs::write(handle.path(), qr.into_svg());
        });
    };

    let copy = move |_| {
        let Some(qr) = code() else { return };
        let Ok(raster) = qr.to_rgba(EXPORT_SCALE) else {
            return;
        };
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_image(arboard::ImageData {
                width: raster.width as usize,
                height: raster.height as usize,
                bytes: raster.pixels.into(),
            });
        }
    };

    rsx! {
        style { {include_str!("ui.css")} }

        button {
            class: "about-open",
            aria_label: fl!("about"),
            onclick: move |_| about.toggle(),
            "?"
        }

        main { class: "shell",
            h1 { class: "title", {fl!("app-title")} }

            input {
                key: "{revision}",
                class: "field",
                r#type: "text",
                // The field is the app, so it holds the keyboard from the
                // moment the window opens.
                autofocus: true,
                placeholder: fl!("input-placeholder"),
                value: "{input}",
                oninput: move |event| {
                    read_error.set(None);
                    input.set(event.value());
                },
            }

            div { class: "stack",
                button { class: "btn", "data-read": "true", onclick: read_file, {fl!("read-file")} }
                if let Some(error) = read_error() {
                    p { class: "error",
                        {
                            match error {
                                ReadError::NotAnImage => fl!("read-error-not-an-image"),
                                ReadError::Damaged(_) => fl!("read-error-damaged"),
                                ReadError::NoCode => fl!("read-error-no-code"),
                                ReadError::Unreadable(_) => fl!("read-error-unreadable"),
                            }
                        }
                    }
                }
            }

            div { class: "row",
                span { class: "tip",
                    {fl!("ec-label")}
                    span { class: "tip-body", {fl!("ec-tooltip")} }
                }
                // `data-ec` is the level's own name rather than its label,
                // because the label is a translation and a test that selected
                // on it would pass in English and nowhere else.
                for (level , name , label) in [
                    (ErrorCorrection::Low, "low", fl!("ec-low")),
                    (ErrorCorrection::Medium, "medium", fl!("ec-medium")),
                    (ErrorCorrection::Quartile, "quartile", fl!("ec-quartile")),
                    (ErrorCorrection::High, "high", fl!("ec-high")),
                ] {
                    button {
                        key: "{name}",
                        class: if ec() == level { "chip on" } else { "chip" },
                        "data-ec": "{name}",
                        aria_pressed: if ec() == level { "true" } else { "false" },
                        onclick: move |_| ec.set(level),
                        {label.clone()}
                    }
                }
            }

            div { class: "row",
                span { class: "label", {fl!("color-dark-label")} }
                button {
                    class: if open_well() == Some(Well::Dark) { "well on" } else { "well" },
                    "data-well": "dark",
                    aria_label: fl!("color-dark-label"),
                    style: "background: {dark().to_hex()}",
                    onclick: move |_| open_well.set(toggled(open_well(), Well::Dark)),
                }
                span { class: "label", {fl!("color-light-label")} }
                button {
                    class: if open_well() == Some(Well::Light) { "well on" } else { "well" },
                    "data-well": "light",
                    aria_label: fl!("color-light-label"),
                    style: "background: {light().to_hex()}",
                    onclick: move |_| open_well.set(toggled(open_well(), Well::Light)),
                }
                button {
                    class: "btn",
                    "data-reset": "true",
                    onclick: move |_| {
                        dark.set(Rgb::BLACK);
                        light.set(Rgb::WHITE);
                    },
                    {fl!("color-reset")}
                }
            }

            if let Some(well) = open_well() {
                Picker {
                    // Keyed on the well, so that switching from one to the
                    // other builds a fresh picker rather than reusing the
                    // first one's half-typed hex.
                    key: "{well:?}",
                    color: if well == Well::Dark { dark } else { light },
                }
            }

            if let Some(src) = preview() {
                div { class: "row",
                    button { class: "btn", onclick: save_png, {fl!("save-png")} }
                    button { class: "btn", onclick: save_svg, {fl!("save-svg")} }
                    button { class: "btn", onclick: copy, {fl!("copy")} }
                }
                div { class: "preview",
                    img { src: "{src}", alt: fl!("app-title") }
                }
            } else {
                div { class: "placeholder", {fl!("qr-placeholder")} }
            }
        }

        if about() {
            div { class: "scrim", onclick: move |_| about.set(false),
                div {
                    class: "about",
                    // The scrim closes on a click; the panel is not the scrim.
                    onclick: move |event| event.stop_propagation(),
                    h2 { {fl!("app-title")} }
                    p { {fl!("app-description")} }
                    p { {format!("Version {}", env!("CARGO_PKG_VERSION"))} }
                    button {
                        class: "link",
                        onclick: move |_| {
                            let _ = open::that(env!("CARGO_PKG_REPOSITORY"));
                        },
                        {fl!("repository")}
                    }
                    button {
                        class: "btn about-close",
                        onclick: move |_| about.set(false),
                        {fl!("close")}
                    }
                }
            }
        }
    }
}

/// The colour picker: a saturation/value square, a hue strip, a palette and a
/// hex field, all writing the same signal.
///
/// This is the one part of the interface that libcosmic gave QRnew for free.
/// There is no `<input type="color">` in Blitz — it has an accessibility role
/// for one and no widget behind it — so every piece here is an ordinary
/// element, and the square is drawn the way a browser would draw it: four
/// stacked CSS background layers over a `div`.
///
/// **The thumbs are background layers rather than child elements**, which is
/// the one non-obvious decision in this file. A child sitting on top of the
/// square is what the pointer hits, so `element_coordinates()` would come back
/// relative to the thumb instead of the square, and `pointer-events: none` —
/// the usual answer — is not implemented in Blitz. A hard-stopped
/// `radial-gradient` has no hit box at all, so the square stays its own target
/// no matter where the thumb is.
#[component]
fn Picker(color: Signal<Rgb>) -> Element {
    // The hex field keeps its own text, because half-typed hex is not a
    // colour: "#2f6" is three characters short of one, and a field that
    // rewrote itself from `color` on every keystroke could not be typed into.
    let mut draft = use_signal(|| color().to_hex());
    let mut valid = use_signal(|| true);

    // Hue, saturation and value are held here rather than derived from the
    // colour on every render, because the conversion back is lossy in exactly
    // the places a picker is used: black has no hue and grey has no hue, so a
    // square dragged into its bottom edge would snap the strip back to red and
    // strand whoever was dragging it.
    let mut hsv = use_signal(|| to_hsv(color()));
    let mut dragging = use_signal(|| false);

    let mut apply = move |next: Hsv| {
        hsv.set(next);
        let rgb = from_hsv(next);
        color.set(rgb);
        draft.set(rgb.to_hex());
        valid.set(true);
    };

    let mut pick_in_square = move |event: Event<PointerData>| {
        let (x, y) = event.element_coordinates().to_tuple();
        let Hsv { hue, .. } = hsv();
        apply(Hsv {
            hue,
            saturation: (x / SQUARE_W).clamp(0.0, 1.0) as f32,
            value: 1.0 - (y / SQUARE_H).clamp(0.0, 1.0) as f32,
        });
    };

    let mut pick_in_strip = move |event: Event<PointerData>| {
        let (x, _) = event.element_coordinates().to_tuple();
        let Hsv {
            saturation, value, ..
        } = hsv();
        apply(Hsv {
            hue: (x / SQUARE_W).clamp(0.0, 1.0) as f32 * 360.0,
            saturation,
            value,
        });
    };

    let Hsv {
        hue,
        saturation,
        value,
    } = hsv();
    let pure = from_hsv(Hsv {
        hue,
        saturation: 1.0,
        value: 1.0,
    })
    .to_hex();
    let square_thumb = thumb(
        f64::from(saturation) * SQUARE_W,
        f64::from(1.0 - value) * SQUARE_H,
        SQUARE_W,
        SQUARE_H,
    );
    let strip_thumb = thumb(
        f64::from(hue) / 360.0 * SQUARE_W,
        STRIP_H / 2.0,
        SQUARE_W,
        STRIP_H,
    );

    rsx! {
        div { class: "picker",
            div {
                class: "sv",
                "data-square": "true",
                style: "background: {square_thumb}, linear-gradient(to top, #000000, rgba(0,0,0,0)), linear-gradient(to right, #FFFFFF, rgba(255,255,255,0)), {pure}",
                onpointerdown: move |event| {
                    dragging.set(true);
                    pick_in_square(event);
                },
                onpointermove: move |event| {
                    if dragging() {
                        pick_in_square(event);
                    }
                },
                onpointerup: move |_| dragging.set(false),
                onpointerleave: move |_| dragging.set(false),
            }
            div {
                class: "strip",
                "data-strip": "true",
                style: "background: {strip_thumb}, linear-gradient(to right, #FF0000, #FFFF00, #00FF00, #00FFFF, #0000FF, #FF00FF, #FF0000)",
                onpointerdown: move |event| {
                    dragging.set(true);
                    pick_in_strip(event);
                },
                onpointermove: move |event| {
                    if dragging() {
                        pick_in_strip(event);
                    }
                },
                onpointerup: move |_| dragging.set(false),
                onpointerleave: move |_| dragging.set(false),
            }
            div { class: "palette",
                for swatch in PALETTE {
                    button {
                        key: "{swatch.to_hex()}",
                        class: if color() == swatch { "swatch on" } else { "swatch" },
                        "data-swatch": "{swatch.to_hex()}",
                        aria_label: "{swatch.to_hex()}",
                        style: "background: {swatch.to_hex()}",
                        onclick: move |_| apply(to_hsv(swatch)),
                    }
                }
            }
            input {
                class: if valid() { "hex" } else { "hex bad" },
                r#type: "text",
                "data-hex": "true",
                value: "{draft}",
                oninput: move |event| {
                    let text = event.value();
                    match parse_hex(&text) {
                        Some(parsed) => {
                            valid.set(true);
                            hsv.set(to_hsv(parsed));
                            color.set(parsed);
                        }
                        None => valid.set(false),
                    }
                    draft.set(text);
                },
            }
        }
    }
}

/// Width of the saturation/value square and of the hue strip, in CSS pixels.
///
/// A number rather than a measurement because the measurement is not
/// available: `get_client_rect` takes the document while an event handler is
/// already holding it, and panics. The stylesheet is told the same number, so
/// the two cannot drift without `the_square_is_the_size_the_maths_assumes`
/// saying so.
const SQUARE_W: f64 = 234.0;
const SQUARE_H: f64 = 140.0;
const STRIP_H: f64 = 22.0;

/// A colour as the picker holds it: hue in degrees, the rest in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hsv {
    hue: f32,
    saturation: f32,
    value: f32,
}

/// One thumb, as a `radial-gradient` background layer.
///
/// Hard stops rather than a fade, so it reads as a ring: transparent inside,
/// two pixels of white, transparent outside, with a dark hairline either side
/// so that it stays visible on white and on black alike.
///
/// The centre is held a ring's width inside the box. A background layer is
/// clipped to its element — unlike the child element a browser would use,
/// which is free to overhang — so an unclamped thumb on a fully black or fully
/// red colour would be a sliver against the edge, which is exactly when
/// somebody is looking for it.
fn thumb(x: f64, y: f64, width: f64, height: f64) -> String {
    const RING: f64 = 9.0;
    let x = x.clamp(RING, width - RING);
    let y = y.clamp(RING, height - RING);
    format!(
        "radial-gradient(circle at {x:.1}px {y:.1}px, \
         rgba(0,0,0,0) 5px, rgba(0,0,0,0.45) 5px, rgba(0,0,0,0.45) 6px, \
         #FFFFFF 6px, #FFFFFF 8px, rgba(0,0,0,0.45) 8px, rgba(0,0,0,0.45) 9px, \
         rgba(0,0,0,0) 9px)"
    )
}

fn to_hsv(rgb: Rgb) -> Hsv {
    let (r, g, b) = (
        f32::from(rgb.r) / 255.0,
        f32::from(rgb.g) / 255.0,
        f32::from(rgb.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let span = max - min;

    let hue = if span == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / span) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / span + 2.0)
    } else {
        60.0 * ((r - g) / span + 4.0)
    };

    Hsv {
        hue: if hue < 0.0 { hue + 360.0 } else { hue },
        saturation: if max == 0.0 { 0.0 } else { span / max },
        value: max,
    }
}

fn from_hsv(hsv: Hsv) -> Rgb {
    let Hsv {
        hue,
        saturation,
        value,
    } = hsv;
    let sector = (hue.rem_euclid(360.0)) / 60.0;
    let span = value * saturation;
    let middle = span * (1.0 - (sector % 2.0 - 1.0).abs());
    let base = value - span;

    let (r, g, b) = match sector as u32 {
        0 => (span, middle, 0.0),
        1 => (middle, span, 0.0),
        2 => (0.0, span, middle),
        3 => (0.0, middle, span),
        4 => (middle, 0.0, span),
        _ => (span, 0.0, middle),
    };

    let channel = |value: f32| ((value + base) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb::new(channel(r), channel(g), channel(b))
}

/// Clicking the well that is already open closes it.
fn toggled(current: Option<Well>, clicked: Well) -> Option<Well> {
    if current == Some(clicked) {
        None
    } else {
        Some(clicked)
    }
}

/// `#rrggbb`, `rrggbb`, `#rgb` or `rgb`, in either case.
fn parse_hex(text: &str) -> Option<Rgb> {
    let text = text.trim().trim_start_matches('#');
    let digit = |at: usize| u8::from_str_radix(&text[at..at + 1], 16).ok();
    let pair = |at: usize| u8::from_str_radix(&text[at..at + 2], 16).ok();

    match text.len() {
        3 => Some(Rgb::new(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
        6 => Some(Rgb::new(pair(0)?, pair(2)?, pair(4)?)),
        _ => None,
    }
}

/// The generated SVG, as something an `<img>` can point at.
///
/// Base64 rather than percent-encoding because a QR code's document is mostly
/// path data and the characters that would have to be escaped are common in
/// it: base64 costs a third more, escaping everything costs three times more,
/// and this is rebuilt on every keystroke.
fn data_url(svg: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(24 + bytes.len().div_ceil(3) * 4);
    out.push_str("data:image/svg+xml;base64,");

    for chunk in bytes.chunks(3) {
        let block = (u32::from(chunk[0]) << 16)
            | (chunk.get(1).map_or(0, |&byte| u32::from(byte)) << 8)
            | chunk.get(2).map_or(0, |&byte| u32::from(byte));

        for slot in 0..4 {
            if slot <= chunk.len() {
                let index = (block >> (18 - 6 * slot)) & 0x3f;
                out.push(ALPHABET[index as usize] as char);
            } else {
                out.push('=');
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_both_lengths_and_either_case() {
        assert_eq!(parse_hex("#2F6F4F"), Some(Rgb::new(0x2f, 0x6f, 0x4f)));
        assert_eq!(parse_hex("2f6f4f"), Some(Rgb::new(0x2f, 0x6f, 0x4f)));
        assert_eq!(parse_hex("  #fff "), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_hex("#f0a"), Some(Rgb::new(255, 0, 0xaa)));
        assert_eq!(parse_hex("#2f6f4"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn base64_matches_a_known_encoding() {
        // The three padding cases, which is the whole of what can go wrong.
        assert!(data_url("any carnal pleasure.").ends_with("YW55IGNhcm5hbCBwbGVhc3VyZS4="));
        assert!(data_url("any carnal pleasure").ends_with("YW55IGNhcm5hbCBwbGVhc3VyZQ=="));
        assert!(data_url("any carnal pleasur").ends_with("YW55IGNhcm5hbCBwbGVhc3Vy"));
    }
}
