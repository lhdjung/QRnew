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
//!
//! # The shape of the window
//!
//! **Three columns, and that is the whole layout argument.** Two fixed-width
//! rails of controls, then a stage on the right that the code sits on. Nothing
//! in a rail can move the code, which is what the original single-column stack
//! got wrong — opening the colour picker there pushed the preview under the
//! bottom of the window, so the one thing somebody was adjusting the colour of
//! was the one thing they could no longer see.
//!
//! The second rail is what stops the first one scrolling. One column of
//! controls only fitted a window this tall with the picker collapsed, so the
//! picker had to be something you opened — and the moment it was, half the
//! form was below the fold. Split in two, everything is on screen at once, the
//! picker is simply *there*, and the wells above it choose which of the two
//! colours it is editing rather than whether it exists.
//!
//! Both rails are now full: Content, Error correction and Margin down the
//! first, Colors and Inset down the second. Height is the scarce thing here
//! and the picker is what holds most of it, which is why adding the Inset card
//! took thirty pixels off the saturation square — the arithmetic is in
//! `ui.css`, and `no_control_is_below_the_fold` checks it at the size a
//! maximized window actually gets on a laptop screen rather than at the size
//! the window falls back to.
//!
//! The three export buttons are drawn from the first frame rather than
//! appearing with the first character, dimmed until there is something to
//! export, so the stage never rearranges itself while it is being looked at.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use dioxus::prelude::*;
use qrnew_core::{ErrorCorrection, ImageFormat, Logo, Qr, QrStyle, ReadError, Rgb};

use crate::fl;

/// Resolution of saved and copied images, in pixels per module.
const EXPORT_SCALE: u32 = 10;

/// The narrowest border the app is prepared to vouch for, in modules.
///
/// Two modules of white is enough for a phone camera to find the edge of the
/// code; below that a scan starts to depend on what the code is printed on and
/// on how steady the hand holding the camera is. The claim is checked rather
/// than assumed — `a_narrow_margin_still_scans` in `qrnew-core` decodes a
/// two-module border at both export sizes.
///
/// It is one constant and not two because the default and the warning are the
/// same idea from either side: the app opens at the narrowest border it will
/// stand behind, and says so as soon as somebody goes under it.
pub const SAFE_MARGIN: u32 = 2;

/// Width of the blank border the app starts with, in modules.
///
/// The QR standard asks for four, which `qrnew_core::DEFAULT_QUIET_ZONE` is,
/// and four is visibly generous on screen: a third of a small code's width is
/// border. [`SAFE_MARGIN`] is what the app opens at instead, and the control
/// is right there for anybody printing something that has to survive a bad
/// photocopier.
pub const DEFAULT_MARGIN: u32 = SAFE_MARGIN;

/// As wide a border as the stepper will go to.
///
/// Past this the code is a stamp in the middle of an empty page, and the
/// preview stops being a useful picture of what gets saved.
pub const MAX_MARGIN: u32 = 16;

/// How long a button says `Copied.` before it goes back to its own name.
///
/// A confirmation is only worth anything while it is still about the click
/// that caused it. Left up, it stops being news and becomes the button's name:
/// somebody who comes back to the window a minute later reads `Copied.` as a
/// label rather than as an answer, and has no way to tell whether the thing on
/// the clipboard is still the thing on screen.
///
/// Three seconds is long enough to be read by somebody whose eyes were on the
/// code rather than on the button, and short enough that it is gone before the
/// next thing anybody does.
pub const CONFIRM_FOR: Duration = Duration::from_secs(3);

/// The middle of the margin field, measured from inside its left border.
///
/// Half of the width `ui.css` gives `.count`, less the one-pixel border: the
/// point the number is centred on. It is here rather than in the stylesheet
/// because **Blitz ignores `text-align` inside an `<input>`** — the field's
/// text belongs to a `parley` editor that is handed a font and nothing else —
/// so the centring is arithmetic the app does with `padding-left` and the
/// field's own `ch`. The long version of the story is above `.count` in
/// `ui.css`, and `the_margin_number_is_centered_in_its_field` is what stops
/// this number and that width drifting apart.
const COUNT_MIDDLE: f32 = 30.0;

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

/// The picture in the middle of the code, as it was picked.
///
/// The bytes are kept rather than the path, because the file is read once and
/// then belongs to the app: a code drawn from a picture that has since been
/// moved or edited on disk would be a code the person never asked for. The
/// format comes off those same bytes — `ImageFormat::detect` looks at what is
/// in the file, not at what it is called — and it is what the thumbnail's
/// `data:` URL declares itself to be.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Inset {
    name: String,
    format: ImageFormat,
    bytes: Vec<u8>,
}

impl Inset {
    /// Reads a file, if it turns out to be a picture.
    ///
    /// `None` covers both ways this can go wrong — the file would not open,
    /// and it opened but is not an image — because the card has one thing to
    /// say about either: that file cannot be the picture in the middle of a
    /// code. Which of the two it was is not something the person can act on
    /// differently.
    fn read(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        // What the file *is*, not what it is called: a dialog filter is a
        // convenience and an extension is a claim, so the bytes decide.
        let format = ImageFormat::detect(&bytes)?;
        Some(Self {
            name: path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            format,
            bytes,
        })
    }
}

/// A button's `Copied.`, which raises itself and then lets go on its own.
///
/// Two signals rather than one flag, because a second copy while the first is
/// still confirmed has to be able to tell the two countdowns apart: `serial`
/// numbers the copies, and a countdown that finds a newer number than its own
/// has been overtaken and leaves the confirmation alone. Without it the first
/// click's timer would cut the second click's confirmation short.
#[derive(Debug, Clone, Copy)]
struct Confirmation {
    shown: Signal<bool>,
    serial: Signal<u64>,
}

fn use_confirmation() -> Confirmation {
    Confirmation {
        shown: use_signal(|| false),
        serial: use_signal(|| 0u64),
    }
}

impl Confirmation {
    /// Whether the button should be saying so. Subscribes, like any read.
    fn showing(&self) -> bool {
        *self.shown.read()
    }

    /// Says so, and takes it back after [`CONFIRM_FOR`].
    fn raise(&mut self) {
        *self.serial.write() += 1;
        let serial = *self.serial.peek();
        self.shown.set(true);

        let mut shown = self.shown;
        let latest = self.serial;
        spawn(async move {
            after(CONFIRM_FOR).await;
            // `peek`, not a read: a task that subscribed to these would be
            // woken by its own writes.
            if *latest.peek() == serial {
                shown.set(false);
            }
        });
    }

    /// Takes it back now, because what was copied is no longer what is here.
    ///
    /// A no-op when nothing is being claimed, so that an effect may call it
    /// without writing on every pass.
    fn lower(&mut self) {
        if *self.shown.peek() {
            self.shown.set(false);
        }
    }
}

/// A picture to open with, provided as a root context by `main.rs`.
///
/// [`Fill`]'s sibling, and it is here for both of the same reasons. The stated
/// one is measurement: a code carrying an inset is a second image decoded and
/// composited on every redraw, so a renderer measured on a bare code has not
/// been measured on the heaviest thing the app draws. The quieter one is that
/// **a native file dialog is the one control neither a test nor a scripted run
/// can touch**, and without a way in, everything the interface does once a
/// picture is in place — the thumbnail, error correction locked at 30%, what
/// the too-long message says — would be reachable only by hand.
///
/// It holds a path rather than bytes: `main.rs` is given one on the command
/// line, and a file that will not open is reported by the same silence as a
/// file that is not a picture. See [`Inset::read`].
#[derive(Clone)]
pub struct Inlay(pub String);

/// Which of the two colours the picker is pointed at.
///
/// Not an `Option`: the picker is always on screen, and the wells choose what
/// it edits. It used to be one, back when opening the picker was a thing you
/// did and closing it again was how you got the rest of the form back.
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
    let mut ec = use_signal(|| ErrorCorrection::Medium);
    let mut dark = use_signal(|| Rgb::BLACK);
    let mut light = use_signal(|| Rgb::WHITE);
    let mut margin = use_signal(|| DEFAULT_MARGIN);
    // The stepper's field keeps its own text for the same reason the hex field
    // does: half-typed input is not a number, and a field rewritten from the
    // value on every keystroke cannot be emptied to type a new one into.
    let mut margin_draft = use_signal(|| DEFAULT_MARGIN.to_string());
    // What was decoded out of an image, and whether it has been copied since.
    //
    // It is shown under the button rather than dropped into the field: reading
    // a code and writing one are two different errands, and somebody checking
    // what a printed code says did not ask for the code they were looking at
    // to become the code the app is drawing.
    let mut read_text = use_signal(|| None::<String>);
    let mut copied = use_confirmation();
    let mut copied_image = use_confirmation();
    let mut read_error = use_signal(|| None::<ReadError>);
    // The picture in the middle of the code, and whether the last file offered
    // for the job turned out not to be one.
    let mut inset = use_signal(|| {
        dioxus_core::try_consume_context::<Inlay>()
            .and_then(|Inlay(path)| Inset::read(std::path::Path::new(&path)))
    });
    let mut inset_error = use_signal(|| false);
    let mut editing = use_signal(|| Well::Dark);
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
            quiet_zone: margin(),
            // The default placement, always: a sixth of the code's width with
            // half a module of air, which `the_default_logo_fits_even_the_
            // smallest_code` in `qrnew-core` shows clears the finder patterns
            // on a 21-module code. That is what lets the app offer an inset
            // with no size control and no way to be turned down — the two
            // rules `Qr::new` enforces are ones this placement cannot break.
            logo: inset
                .read()
                .as_ref()
                .map(|chosen| Logo::new(chosen.bytes.clone())),
            ..QrStyle::default()
        };
        // `Err` is an input past what the densest code can hold. The libcosmic
        // build showed the placeholder for that and said nothing, and so does
        // this.
        Qr::new(&text, ec(), &style).ok()
    });

    // Kept apart from `code` so that a re-render that changes neither the text
    // nor the colours does not re-encode the SVG into base64.
    let preview = use_memo(move || {
        code()
            .as_ref()
            .map(|qr| data_url("image/svg+xml", qr.svg().as_bytes()))
    });

    // The chosen picture, as something an `<img>` can point at. A memo for the
    // same reason: the bytes are base64'd once per choice rather than once per
    // keystroke.
    let thumbnail = use_memo(move || {
        inset
            .read()
            .as_ref()
            .map(|chosen| data_url(chosen.format.mime(), &chosen.bytes))
    });

    let read_file = move |_| {
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
                .pick_file()
                .await
            else {
                return;
            };
            let outcome = match std::fs::read(handle.path()) {
                Ok(bytes) => qrnew_core::read(&bytes),
                Err(error) => Err(ReadError::Damaged(error.to_string())),
            };
            match outcome {
                Ok(text) => {
                    read_error.set(None);
                    copied.lower();
                    read_text.set(Some(text));
                }
                Err(error) => {
                    read_text.set(None);
                    read_error.set(Some(error));
                }
            }
        });
    };

    // The file dialog offers the same formats the reader does, because they
    // are the same list for the same reason: they are what `resvg` can draw,
    // and the preview and both exports go through `resvg`.
    let choose_inset = move |_| {
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
                .pick_file()
                .await
            else {
                return;
            };
            match Inset::read(handle.path()) {
                Some(chosen) => {
                    inset_error.set(false);
                    inset.set(Some(chosen));
                }
                None => inset_error.set(true),
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
            let copy = clipboard.set_image(arboard::ImageData {
                width: raster.width as usize,
                height: raster.height as usize,
                bytes: raster.pixels.into(),
            });
            if copy.is_ok() {
                copied_image.raise();
            }
        }
    };

    let copy_text = move |_| {
        let Some(text) = read_text() else { return };
        if let Ok(mut clipboard) = arboard::Clipboard::new()
            && clipboard.set_text(text).is_ok()
        {
            copied.raise();
        }
    };

    // The confirmation belongs to the image that was copied, so any edit that
    // redraws the code takes it back: what is on the clipboard is no longer
    // what is on the stage, and a button still saying "Copied." would be
    // claiming otherwise.
    use_effect(move || {
        code.read();
        // `Confirmation::lower` peeks rather than reads, so this effect does
        // not subscribe to the flag it clears — subscribing would make it run
        // on the write that raises it and put it straight back down.
        copied_image.lower();
    });

    // Both stepper buttons and the field itself go through here, so the number
    // and the text under it cannot disagree about what the margin is.
    let mut set_margin = move |next: u32| {
        let next = next.min(MAX_MARGIN);
        margin.set(next);
        margin_draft.set(next.to_string());
    };

    // Whether there is anything to save, copy or look at. It decides both the
    // stage's content and how the three export buttons are drawn.
    let ready = preview().is_some();

    // Whether an inset is in place. It is asked three times below — the code
    // is drawn with one, error correction is held at 30% by one, and the
    // too-long message says different things with and without one — so it is
    // read once here rather than borrowed three times in the middle of the
    // markup.
    let has_inset = inset.read().is_some();
    let shown_ec = if has_inset { ErrorCorrection::High } else { ec() };

    // The line under the field. The prompt while the field is empty, nothing
    // at all while a code is being drawn, and — the one failure the app could
    // not report before — text past what a code can hold.
    //
    // It used to count the characters typed once there was a code. That count
    // was a running total of nothing: there is no length anybody is working
    // towards here, no limit worth watching approach, and the one number that
    // *would* matter arrives as a sentence when the text stops fitting. What
    // is left is a line that is often silent, which `min-height` in `ui.css`
    // is what makes safe.
    let (note, note_class) = if input().is_empty() {
        (fl!("input-placeholder"), "note")
    } else if ready {
        (String::new(), "note")
    } else if has_inset {
        // An inset raises error correction to 30%, which costs capacity — so
        // text that would have fitted a moment ago may not fit now, and the
        // way out is a choice between the two rather than only the text.
        (fl!("input-too-long-inset"), "note bad")
    } else {
        (fl!("input-too-long"), "note bad")
    };

    // How far the number in the margin field has to be pushed to sit in the
    // middle of it. Half the field, less half the text: `1ch` is the width of
    // a digit in the field's own font, so this is exact rather than tuned.
    let count_pad = margin_draft.read().chars().count() as f32 * 0.5;

    // Cloned out rather than borrowed through the markup: holding a `Ref` on
    // the signal while the tree is built is a lock held over a great deal of
    // other people's code, and a file name is a few dozen bytes.
    let inset_name = inset.read().as_ref().map(|chosen| chosen.name.clone());
    let export_ink = if ready { Ink::Plain } else { Ink::Faint };
    let export_class = if ready { "btn" } else { "btn off" };

    rsx! {
        style { {include_str!("ui.css")} }

        div { class: "app",

            header { class: "topbar",
                div { class: "brand",
                    {glyph(Glyph::Code, Ink::Accent, "glyph-brand")}
                    span { {fl!("app-title")} }
                }
                div { class: "spacer" }
                button {
                    class: "about-open",
                    onclick: move |_| about.toggle(),
                    {glyph(Glyph::Info, Ink::Faint, "glyph")}
                    span { {fl!("about")} }
                }
            }

            div { class: "body",

                section { class: "rail rail-main",

                    div { class: "card",
                        div { class: "card-head",
                            {glyph(Glyph::Type, Ink::Accent, "glyph")}
                            span { {fl!("section-content")} }
                        }
                        input {
                            class: "field",
                            r#type: "text",
                            // The field is the app, so it holds the keyboard
                            // from the moment the window opens.
                            autofocus: true,
                            value: "{input}",
                            oninput: move |event| {
                                read_error.set(None);
                                input.set(event.value());
                            },
                        }
                        // **Blitz has no `placeholder`.** There is no such
                        // attribute in `blitz-dom` at all, so setting one — as
                        // this app did until now — left the field simply
                        // blank. The prompt is a sibling instead, and it is a
                        // sibling rather than an overlay because
                        // `pointer-events: none` is also missing, so anything
                        // laid over the field would eat the click that is
                        // supposed to focus it.
                        //
                        // The line keeps its height when it has nothing to
                        // say, so the button below it never moves.
                        p { class: "{note_class}", "{note}" }
                        button {
                            class: "btn wide",
                            "data-read": "true",
                            onclick: read_file,
                            {glyph(Glyph::Image, Ink::Plain, "glyph")}
                            span { {fl!("read-file")} }
                        }
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
                        // What was in the image, in the same place the failure
                        // message appears — so the one button has one place it
                        // reports to, whichever way it went.
                        if let Some(text) = read_text() {
                            div { class: "readout",
                                span { class: "readout-head", {fl!("read-result")} }
                                p { class: "readout-text", "data-readout": "true", "{text}" }
                                button {
                                    class: "btn wide",
                                    "data-copy-text": "true",
                                    onclick: copy_text,
                                    {
                                        glyph(
                                            if copied.showing() { Glyph::Check } else { Glyph::Copy },
                                            if copied.showing() { Ink::Accent } else { Ink::Plain },
                                            "glyph",
                                        )
                                    }
                                    span {
                                        {
                                            if copied.showing() {
                                                fl!("read-copied")
                                            } else {
                                                fl!("read-copy")
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "card",
                        div { class: "card-head",
                            {glyph(Glyph::Shield, Ink::Accent, "glyph")}
                            span { {fl!("section-correction")} }
                        }
                        // An inset takes this control away: `Qr::new` raises
                        // error correction to `High` whenever there is a logo,
                        // whatever it was asked for. The row goes on saying
                        // what the code is actually drawn at — dimmed, and
                        // inert, because a button that looks live and moves
                        // nothing is worse than one that admits it is held.
                        // The choice underneath is remembered, and comes back
                        // when the inset goes.
                        div { class: "segments",
                            // `data-ec` is the level's own name rather than its
                            // label, because the label is a translation and a
                            // test that selected on it would pass in English
                            // and nowhere else.
                            for (level , name , label) in [
                                (ErrorCorrection::Low, "low", fl!("ec-low")),
                                (ErrorCorrection::Medium, "medium", fl!("ec-medium")),
                                (ErrorCorrection::Quartile, "quartile", fl!("ec-quartile")),
                                (ErrorCorrection::High, "high", fl!("ec-high")),
                            ] {
                                button {
                                    key: "{name}",
                                    class: chip_class(shown_ec == level, has_inset),
                                    "data-ec": "{name}",
                                    aria_pressed: if shown_ec == level { "true" } else { "false" },
                                    onclick: move |_| {
                                        if !has_inset {
                                            ec.set(level);
                                        }
                                    },
                                    {label.clone()}
                                }
                            }
                        }
                        // What the previous build hid behind a hover tooltip.
                        // There is room for it here, and a sentence somebody
                        // can read is worth more than one they have to find.
                        p { class: "hint",
                            {if has_inset { fl!("ec-locked") } else { fl!("ec-hint") }}
                        }
                    }

                    div { class: "card",
                        div { class: "card-head",
                            {glyph(Glyph::Frame, Ink::Accent, "glyph")}
                            span { {fl!("section-margin")} }
                        }
                        // A number with a button on either side of it. The
                        // buttons are for the adjustment somebody is making by
                        // eye, watching the preview; the field is for the one
                        // they already know the answer to.
                        div { class: "stepper",
                            button {
                                class: "step",
                                "data-margin-less": "true",
                                aria_label: fl!("margin-less"),
                                onclick: move |_| set_margin(margin().saturating_sub(1)),
                                {glyph(Glyph::Minus, Ink::Plain, "glyph")}
                            }
                            input {
                                class: "count",
                                r#type: "text",
                                "data-margin": "true",
                                // **This is what centres the number.** Blitz
                                // paints an input's text at the left edge of
                                // its content box and never looks at
                                // `text-align`, so the padding is the only
                                // handle there is: half the field, less half
                                // the text, in the field's own digit width.
                                style: "padding-left: calc({COUNT_MIDDLE}px - {count_pad}ch)",
                                value: "{margin_draft}",
                                oninput: move |event| {
                                    let text = event.value();
                                    match text.trim().parse::<u32>() {
                                        // Out of range is clamped rather than
                                        // refused, so a stray digit corrects
                                        // itself instead of stopping the field.
                                        Ok(value) if value > MAX_MARGIN => set_margin(MAX_MARGIN),
                                        Ok(value) => {
                                            margin.set(value);
                                            margin_draft.set(text);
                                        }
                                        // Not a number yet — most often an
                                        // empty field on the way to one.
                                        Err(_) => margin_draft.set(text),
                                    }
                                },
                                // Half-typed input is allowed to sit in the
                                // field while it is being typed, but it cannot
                                // outlive the keyboard: the code is still drawn
                                // at the last number that parsed, so leaving an
                                // empty field behind would leave the app
                                // showing one margin and the field claiming
                                // another. Whatever is applied is what comes
                                // back.
                                onblur: move |_| margin_draft.set(margin().to_string()),
                            }
                            button {
                                class: "step",
                                "data-margin-more": "true",
                                aria_label: fl!("margin-more"),
                                onclick: move |_| set_margin(margin() + 1),
                                {glyph(Glyph::Plus, Ink::Plain, "glyph")}
                            }
                        }
                        // Only once somebody has actually gone below two: a
                        // caveat printed permanently is read once and then
                        // stops being read, and this one arrives at the moment
                        // it applies. It is a banner under the control rather
                        // than a remark at the end of its row, and it is set
                        // larger than the sentence below it, because the one
                        // line here that says a code might not scan should not
                        // be the smallest thing in the card.
                        if margin() < SAFE_MARGIN {
                            p { class: "warn", "data-margin-warning": "true",
                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                span { {fl!("margin-warning")} }
                            }
                        }
                        p { class: "hint", {fl!("margin-hint")} }
                    }
                }

                section { class: "rail rail-colors",

                    div { class: "card",
                        div { class: "card-head",
                            {glyph(Glyph::Drop, Ink::Accent, "glyph")}
                            span { {fl!("section-colors")} }
                        }
                        div { class: "wells",
                            ColorWell {
                                which: Well::Dark,
                                name: fl!("color-dark-label"),
                                color: dark(),
                                editing: editing() == Well::Dark,
                                onpick: move |_| editing.set(Well::Dark),
                            }
                            ColorWell {
                                which: Well::Light,
                                name: fl!("color-light-label"),
                                color: light(),
                                editing: editing() == Well::Light,
                                onpick: move |_| editing.set(Well::Light),
                            }
                        }
                        // Two branches rather than one call with a signal
                        // chosen inside it, because switching wells has to
                        // *rebuild* the picker: its draft hex and its hue are
                        // its own state, and a picker handed a different
                        // colour would carry the first one's over. A `key`
                        // does not do it — Dioxus diffs a lone child by
                        // position — but two arms of an `if` are two different
                        // nodes, and swapping them mounts a fresh one.
                        if editing() == Well::Dark {
                            Picker { color: dark }
                        } else {
                            Picker { color: light }
                        }
                        button {
                            class: "btn wide",
                            "data-reset": "true",
                            onclick: move |_| {
                                dark.set(Rgb::BLACK);
                                light.set(Rgb::WHITE);
                            },
                            {glyph(Glyph::Undo, Ink::Plain, "glyph")}
                            span { {fl!("color-reset")} }
                        }
                    }

                    // Under the colours, which is where it belongs: an inset
                    // is the last thing anybody adds and the first thing they
                    // take away again, and it is the only control in the
                    // window that changes what another one is allowed to say.
                    div { class: "card",
                        div { class: "card-head",
                            {glyph(Glyph::Inset, Ink::Accent, "glyph")}
                            span { {fl!("section-inset")} }
                        }
                        if let Some(name) = inset_name {
                            div { class: "inset",
                                img {
                                    class: "inset-thumb",
                                    "data-inset-thumb": "true",
                                    // On the code's own background, because
                                    // that is what the picture will be sitting
                                    // on: a dark mark on a transparent ground
                                    // reads here and disappears there.
                                    style: "background: {light().to_hex()}",
                                    src: thumbnail().unwrap_or_default(),
                                    alt: "{name}",
                                }
                                span { class: "inset-name", "data-inset-name": "true", "{name}" }
                            }
                            div { class: "inset-actions",
                                button {
                                    class: "btn",
                                    "data-inset-choose": "true",
                                    onclick: choose_inset,
                                    {glyph(Glyph::Image, Ink::Plain, "glyph")}
                                    span { {fl!("inset-replace")} }
                                }
                                button {
                                    class: "btn",
                                    "data-inset-remove": "true",
                                    onclick: move |_| {
                                        inset.set(None);
                                        inset_error.set(false);
                                    },
                                    {glyph(Glyph::Close, Ink::Plain, "glyph")}
                                    span { {fl!("inset-remove")} }
                                }
                            }
                        } else {
                            button {
                                class: "btn wide",
                                "data-inset-choose": "true",
                                onclick: choose_inset,
                                {glyph(Glyph::Image, Ink::Plain, "glyph")}
                                span { {fl!("inset-choose")} }
                            }
                        }
                        if inset_error() {
                            p { class: "error", {fl!("inset-error")} }
                        }
                        // The sentence is what an empty card is for: it says
                        // what an inset is and what taking one costs. Once
                        // there is a picture the thumbnail says the first
                        // half, and the error-correction card next door is
                        // already saying the second in its own hint — so
                        // keeping it here would be the same fact twice, in the
                        // one column that has no room to spare.
                        if !has_inset {
                            p { class: "hint", {fl!("inset-hint")} }
                        }
                    }
                }

                section { class: "stage",
                    if let Some(src) = preview() {
                        // The mat is painted in the code's own background
                        // colour, so the rounded corners belong to the mat and
                        // the image never has to be clipped to them.
                        div { class: "preview", style: "background: {light().to_hex()}",
                            img { src: "{src}", alt: fl!("app-title") }
                        }
                    } else {
                        div { class: "placeholder",
                            {glyph(Glyph::Code, Ink::Faint, "glyph-empty")}
                            span { {fl!("qr-placeholder")} }
                        }
                    }
                    div { class: "exports",
                        button { class: "{export_class}", onclick: save_png,
                            {glyph(Glyph::Download, export_ink, "glyph")}
                            span { {fl!("save-png")} }
                        }
                        button { class: "{export_class}", onclick: save_svg,
                            {glyph(Glyph::Download, export_ink, "glyph")}
                            span { {fl!("save-svg")} }
                        }
                        button {
                            class: "{export_class}",
                            "data-copy-image": "true",
                            onclick: copy,
                            {
                                glyph(
                                    if copied_image.showing() { Glyph::Check } else { Glyph::Copy },
                                    if copied_image.showing() { Ink::Accent } else { export_ink },
                                    "glyph",
                                )
                            }
                            span {
                                {
                                    if copied_image.showing() {
                                        fl!("copy-copied")
                                    } else {
                                        fl!("copy")
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if about() {
            div { class: "scrim", onclick: move |_| about.set(false),
                div {
                    class: "about",
                    // The scrim closes on a click; the panel is not the scrim.
                    onclick: move |event| event.stop_propagation(),
                    h2 {
                        {glyph(Glyph::Code, Ink::Accent, "glyph-brand")}
                        span { {fl!("app-title")} }
                    }
                    p { {fl!("app-description")} }
                    p { class: "version", {format!("Version {}", env!("CARGO_PKG_VERSION"))} }
                    div { class: "about-actions",
                        button {
                            class: "btn",
                            onclick: move |_| {
                                let _ = open::that(env!("CARGO_PKG_REPOSITORY"));
                            },
                            {glyph(Glyph::External, Ink::Plain, "glyph")}
                            span { {fl!("repository")} }
                        }
                        button {
                            class: "btn about-close",
                            onclick: move |_| about.set(false),
                            {glyph(Glyph::Close, Ink::Plain, "glyph")}
                            span { {fl!("close")} }
                        }
                    }
                }
            }
        }
    }
}

/// One of the two colour tiles: the swatch, what it paints, and its hex.
///
/// A tile rather than the bare circle the libcosmic build drew, because a
/// circle of colour beside the word "Foreground" is two things to look at for
/// one fact. This is one thing to look at, and it is also the click target
/// that points the picker below at this colour.
#[component]
fn ColorWell(
    which: Well,
    name: String,
    color: Rgb,
    editing: bool,
    onpick: EventHandler<MouseEvent>,
) -> Element {
    let slug = match which {
        Well::Dark => "dark",
        Well::Light => "light",
    };
    rsx! {
        button {
            class: if editing { "well on" } else { "well" },
            "data-well": "{slug}",
            aria_label: "{name}",
            aria_pressed: if editing { "true" } else { "false" },
            onclick: move |event| onpick.call(event),
            span { class: "chipdot", style: "background: {color.to_hex()}" }
            span { class: "well-text",
                span { class: "well-name", "{name}" }
                span { class: "well-hex", "{color.to_hex()}" }
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
/// **The markers are background layers rather than child elements**, which is
/// the one non-obvious decision in this file. A child sitting on top of the
/// square is what the pointer hits, and Blitz measures element coordinates
/// once against the hit node — so `element_coordinates()` would come back
/// relative to the marker instead of the square, and `pointer-events: none` —
/// the usual answer — is not implemented. A background layer has no hit box at
/// all, so the square stays its own target no matter where the marker is.
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

    // The last colour this picker itself wrote, so that a colour arriving from
    // anywhere else can be told apart from one of its own. "Reset to black &
    // white" is the one place another colour comes from, and before this the
    // square, the strip and the hex field all carried on showing whatever had
    // been picked before it — the code went black and the picker did not.
    //
    // A round trip through HSV cannot stand in for this comparison: it is lossy
    // for exactly the colours a picker is used on, so a half-typed hex would be
    // mistaken for an outside change and overwritten mid-keystroke.
    let mut written = use_signal(|| *color.peek());

    let mut apply = move |next: Hsv| {
        hsv.set(next);
        let rgb = from_hsv(next);
        color.set(rgb);
        written.set(rgb);
        draft.set(rgb.to_hex());
        valid.set(true);
    };

    use_effect(move || {
        let outside = color();
        // `peek` rather than a read: this effect must not subscribe to what it
        // writes, or setting `written` below would schedule it to run again.
        if outside != *written.peek() {
            written.set(outside);
            hsv.set(to_hsv(outside));
            draft.set(outside.to_hex());
            valid.set(true);
        }
    });

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
    let picked = color();
    let square_mark = marker(
        f64::from(saturation) * SQUARE_W,
        f64::from(1.0 - value) * SQUARE_H,
        SQUARE_W,
        SQUARE_H,
        picked,
    );
    let strip_mark = marker(
        f64::from(hue) / 360.0 * SQUARE_W,
        STRIP_H / 2.0,
        SQUARE_W,
        STRIP_H,
        from_hsv(Hsv { hue, saturation: 1.0, value: 1.0 }),
    );

    rsx! {
        div { class: "picker",
            div {
                class: "sv",
                "data-square": "true",
                // Four layers: the marker's three, then the white-to-hue wash,
                // the black-to-nothing wash, and the hue itself as a gradient
                // from one colour to the same colour — a flat fill, written
                // this way so that every layer is an image and the three lists
                // below line up entry for entry.
                style: "background-image: {square_mark.image}, \
                    linear-gradient(to top, #000000, rgba(0,0,0,0)), \
                    linear-gradient(to right, #FFFFFF, rgba(255,255,255,0)), \
                    linear-gradient({pure}, {pure}); \
                    background-position: {square_mark.position}, 0px 0px, 0px 0px, 0px 0px; \
                    background-size: {square_mark.size}, auto, auto, auto; \
                    background-repeat: no-repeat",
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
                style: "background-image: {strip_mark.image}, \
                    linear-gradient(to right, #FF0000, #FFFF00, #00FF00, #00FFFF, #0000FF, #FF00FF, #FF0000); \
                    background-position: {strip_mark.position}, 0px 0px; \
                    background-size: {strip_mark.size}, auto; \
                    background-repeat: no-repeat",
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
            div { class: "hexrow",
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
                                written.set(parsed);
                            }
                            None => valid.set(false),
                        }
                        draft.set(text);
                    },
                }
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
///
/// These are border-box sizes, which is what a pointer's element coordinates
/// are measured against; `ui.css` sets `background-origin: border-box` so that
/// the layers agree.
const SQUARE_W: f64 = 310.0;
const SQUARE_H: f64 = 170.0;
const STRIP_H: f64 = 22.0;

/// A colour as the picker holds it: hue in degrees, the rest in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hsv {
    hue: f32,
    saturation: f32,
    value: f32,
}

/// The ink an icon is drawn in.
///
/// It is a presentation attribute on the `<svg>` rather than a CSS colour,
/// because **CSS does not reach inside an SVG in Blitz** — the element is
/// handed to `usvg` as a document of its own. Which is also why the interface
/// has one theme: see the note at the top of `ui.css`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ink {
    /// Section marks and the brand, in the accent.
    Accent,
    /// Buttons somebody can press.
    Plain,
    /// Buttons that are not doing anything yet, and quiet furniture.
    Faint,
    /// The one caution the app has: a margin too narrow to vouch for.
    Warn,
}

impl Ink {
    const fn stroke(self) -> &'static str {
        match self {
            Ink::Accent => "#4ECB8F",
            Ink::Plain => "#D2D8E0",
            Ink::Faint => "#8C949E",
            // `--warn` in `ui.css`, spelled out again because an icon's ink
            // is a presentation attribute and cannot read a custom property.
            Ink::Warn => "#E9C07C",
        }
    }
}

/// The icons, as the paths that draw them on a 24×24 grid.
///
/// Hand-drawn rather than pulled from an icon font, for the reason the whole
/// app exists: a font is a file to ship and a licence to honour, and seventeen
/// icons is less of both. They are stroked, round-capped and unfilled, which
/// is the one decision that keeps them looking like a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    /// Three finder squares and a scatter of modules: QRnew's own mark, and
    /// the thing that stands in for the code before there is one.
    Code,
    Info,
    Type,
    Image,
    Shield,
    Drop,
    Undo,
    Download,
    Copy,
    /// The copy that has already happened.
    Check,
    /// A border around a smaller square: the margin, drawn as what it is.
    Frame,
    /// A square with something round in the middle of it: the inset, drawn as
    /// what it is, and deliberately not [`Frame`](Glyph::Frame) with the inner
    /// shape filled — the two sit in the same column and have to be told apart
    /// at a glance.
    Inset,
    /// A triangle with a bang in it, for the one warning in the window.
    Alert,
    Minus,
    Plus,
    Close,
    External,
}

impl Glyph {
    const fn paths(self) -> &'static [&'static str] {
        match self {
            Glyph::Code => &[
                "M4 4 H9.5 V9.5 H4 Z",
                "M14.5 4 H20 V9.5 H14.5 Z",
                "M4 14.5 H9.5 V20 H4 Z",
                "M14.6 15 h2.4",
                "M19.6 15 h0.4",
                "M14.6 20 h0.4",
                "M17.4 20 h2.6",
                "M17.4 17.5 h2.6",
            ],
            Glyph::Info => &[
                "M21 12 A9 9 0 1 1 3 12 A9 9 0 1 1 21 12",
                "M12 11.2 V16.4",
                "M12 7.7 h0.4",
            ],
            Glyph::Type => &["M4.6 7.2 V5 H19.4 V7.2", "M12 5 V19", "M8.8 19 H15.2"],
            Glyph::Image => &[
                "M4.6 4.8 H19.4 V19.2 H4.6 Z",
                "M4.6 15.6 L9.4 10.8 L13.6 15 L16.2 12.4 L19.4 15.6",
                "M15.4 8.6 h0.4",
            ],
            Glyph::Shield => &[
                "M12 3.2 L19.8 6 V11.6 C19.8 16 16.6 19.7 12 20.8 C7.4 19.7 4.2 16 4.2 11.6 V6 Z",
                "M9.2 12 L11.3 14.1 L15.2 10.2",
            ],
            Glyph::Drop => &[
                "M12 3.4 C12 3.4 5.6 9.9 5.6 14.1 A6.4 6.4 0 0 0 18.4 14.1 C18.4 9.9 12 3.4 12 3.4 Z",
                "M9.3 14.7 A2.7 2.7 0 0 0 12 17.4",
            ],
            Glyph::Undo => &["M3.5 12 A8.5 8.5 0 1 0 6.4 5.7 L3.5 8.7", "M3.5 3.7 V8.7 H8.5"],
            Glyph::Download => &[
                "M12 3.6 V15.4",
                "M7.4 10.9 L12 15.5 L16.6 10.9",
                "M4.6 19.6 H19.4",
            ],
            Glyph::Copy => &["M9 8.6 H19.4 V19.4 H9 Z", "M15.4 8.6 V4.6 H4.6 V15.4 H9"],
            Glyph::Check => &["M5.2 12.6 L10 17.4 L18.8 7.2"],
            Glyph::Frame => &["M4.4 4.4 H19.6 V19.6 H4.4 Z", "M9.2 9.2 H14.8 V14.8 H9.2 Z"],
            Glyph::Inset => &[
                "M4.4 4.4 H19.6 V19.6 H4.4 Z",
                "M15.2 12 A3.2 3.2 0 1 1 8.8 12 A3.2 3.2 0 1 1 15.2 12",
            ],
            Glyph::Alert => &["M12 3.9 L21.2 19.9 H2.8 Z", "M12 10 V14.3", "M12 17 h0.4"],
            Glyph::Minus => &["M6.2 12 H17.8"],
            Glyph::Plus => &["M6.2 12 H17.8", "M12 6.2 V17.8"],
            Glyph::Close => &["M6.4 6.4 L17.6 17.6", "M17.6 6.4 L6.4 17.6"],
            Glyph::External => &[
                "M14.2 4.6 H19.4 V9.8",
                "M19.4 4.6 L11.2 12.8",
                "M17 13.8 V19.4 H4.6 V7 H10.2",
            ],
        }
    }
}

/// The classes one error-correction segment is drawn with.
///
/// Four states out of two questions — is this the level the code is drawn at,
/// and is an inset holding the row where it is — and they are spelled out
/// rather than assembled, because a class list built by pushing strings
/// together is a class list nothing can grep for.
const fn chip_class(selected: bool, locked: bool) -> &'static str {
    match (selected, locked) {
        (true, false) => "chip on",
        (true, true) => "chip on off",
        (false, false) => "chip",
        (false, true) => "chip off",
    }
}

/// One icon, as an inline `<svg>` sized by `class`.
///
/// Blitz serializes the element back to markup and parses it with `usvg`, the
/// same route the preview takes — so an icon here is a real document, not a
/// glyph in a font and not a rasterized image.
fn glyph(kind: Glyph, ink: Ink, class: &'static str) -> Element {
    rsx! {
        svg {
            class: "{class}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: ink.stroke(),
            stroke_width: "1.7",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for (index , outline) in kind.paths().iter().enumerate() {
                path { key: "{index}", d: "{outline}" }
            }
        }
    }
}

/// The three `background` lists that draw one position marker.
///
/// Kept together because they are only correct together: the images, their
/// sizes and their positions are three parallel lists and a layer is the same
/// index in each of them.
struct Marker {
    image: String,
    position: String,
    size: String,
}

/// A marker on the square or the strip, as three stacked background layers.
///
/// It is a dark outline, a white ring inside it and the picked colour in the
/// middle — three filled squares, largest at the bottom, so that it reads on a
/// white corner and a black one alike and says what has been picked while it
/// is at it.
///
/// **Not a `radial-gradient`, which is what drew it before.** Blitz resolves a
/// radial gradient's centre in CSS pixels and then adds it to a rectangle it
/// has already measured in device pixels, so on a 2× display the ring landed
/// at half the offset it was given: the colour under the pointer was right and
/// the mark was somewhere else entirely. `background-position` and
/// `background-size` are both multiplied by the scale before they are used,
/// and `linear-gradient(c, c)` is a flat fill of `c`, so a marker built out of
/// those is drawn where it was put at any scale.
///
/// The centre is held half a marker inside the box. A background layer is
/// clipped to its element — unlike the child element a browser would use,
/// which is free to overhang — so an unclamped marker on a fully black or
/// fully saturated colour would be a sliver against the edge, which is exactly
/// when somebody is looking for it.
fn marker(x: f64, y: f64, width: f64, height: f64, fill: Rgb) -> Marker {
    /// Half the outermost square, which is how far in the centre is held.
    const REACH: f64 = 10.0;

    let x = x.clamp(REACH, width - REACH);
    let y = y.clamp(REACH, height - REACH);
    let fill = fill.to_hex();
    let corner = |inset: f64| format!("{:.1}px {:.1}px", x - inset, y - inset);

    Marker {
        image: format!(
            "linear-gradient({fill}, {fill}), \
             linear-gradient(#FFFFFF, #FFFFFF), \
             linear-gradient(rgba(0,0,0,0.55), rgba(0,0,0,0.55))"
        ),
        position: format!("{}, {}, {}", corner(5.0), corner(8.0), corner(REACH)),
        size: "10px 10px, 16px 16px, 20px 20px".to_string(),
    }
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

/// Bytes, as something an `<img>` can point at.
///
/// Two callers: the generated SVG, rebuilt on every keystroke, and whatever
/// picture has been chosen as an inset, encoded once when it is chosen.
///
/// Base64 rather than percent-encoding because a QR code's document is mostly
/// path data and the characters that would have to be escaped are common in
/// it: base64 costs a third more, escaping everything costs three times more,
/// and this is rebuilt on every keystroke.
fn data_url(mime: &str, bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(mime.len() + 13 + bytes.len().div_ceil(3) * 4);
    out.push_str("data:");
    out.push_str(mime);
    out.push_str(";base64,");

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

/// A future that completes once `delay` has passed.
///
/// **There is no timer in the dependency list to borrow one from**, and that
/// is the point of the dependency list: `tokio` is in the tree, pulled in by
/// something else, but only as `rt` — no time driver, and nothing running one.
/// The alternative to twenty lines here is a crate whose whole job is to spawn
/// the thread below, in an app whose privacy claim is that somebody can read
/// its dependencies in one sitting.
///
/// The thread is started on the first poll rather than on construction, so a
/// countdown that is dropped before anyone waits on it costs nothing. The
/// waker is stored on every poll rather than only the first: a task that is
/// polled from somewhere else afterwards has a new waker, and the old one
/// would wake nobody.
fn after(delay: Duration) -> After {
    After { delay, alarm: None }
}

struct After {
    delay: Duration,
    alarm: Option<Arc<Mutex<Countdown>>>,
}

#[derive(Default)]
struct Countdown {
    done: bool,
    waker: Option<Waker>,
}

impl Future for After {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        let delay = me.delay;
        let alarm = me.alarm.get_or_insert_with(|| {
            let alarm = Arc::new(Mutex::new(Countdown::default()));
            let ring = Arc::clone(&alarm);
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let waker = {
                    let mut countdown = ring.lock().unwrap();
                    countdown.done = true;
                    countdown.waker.take()
                };
                // Woken outside the lock: the waker runs the app's own code on
                // the way through, and it has no business doing that while
                // holding something this thread will not be back to release.
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
            alarm
        });

        let mut countdown = alarm.lock().unwrap();
        if countdown.done {
            Poll::Ready(())
        } else {
            countdown.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
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
        let url = |text: &str| data_url("image/svg+xml", text.as_bytes());

        // The three padding cases, which is the whole of what can go wrong.
        assert!(url("any carnal pleasure.").ends_with("YW55IGNhcm5hbCBwbGVhc3VyZS4="));
        assert!(url("any carnal pleasure").ends_with("YW55IGNhcm5hbCBwbGVhc3VyZQ=="));
        assert!(url("any carnal pleasur").ends_with("YW55IGNhcm5hbCBwbGVhc3Vy"));
    }

    #[test]
    fn a_data_url_declares_the_type_it_was_given() {
        assert!(data_url("image/png", b"\x89PNG").starts_with("data:image/png;base64,"));
    }

    /// **The countdown behind every `Copied.` in the window.**
    ///
    /// It is tested here rather than through the interface because raising a
    /// confirmation means putting something on the clipboard first, and the
    /// tests run on a Linux CI machine with no display to have a clipboard on.
    /// So this is the part that can be checked anywhere: it does not finish
    /// early, it does finish, and it wakes whoever was waiting rather than
    /// relying on being polled again by chance.
    #[test]
    fn a_countdown_finishes_when_it_says_it_will() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Counts the wake-ups, so that the test can tell being woken from
        /// being polled again for some other reason.
        struct Counter(AtomicUsize);

        impl std::task::Wake for Counter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Release);
            }
        }

        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&counter));
        let mut cx = Context::from_waker(&waker);

        let delay = Duration::from_millis(120);
        let mut timer = std::pin::pin!(after(delay));
        let started = std::time::Instant::now();

        assert_eq!(timer.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(
            counter.0.load(Ordering::Acquire),
            0,
            "nothing has been woken yet"
        );

        // Slack in one direction only: a thread that sleeps is allowed to
        // oversleep, and this asserts it did not *under*sleep.
        std::thread::sleep(delay * 3);
        assert!(started.elapsed() >= delay);
        assert!(
            counter.0.load(Ordering::Acquire) >= 1,
            "the waiter was woken rather than left to poll again on its own"
        );
        assert_eq!(timer.as_mut().poll(&mut cx), Poll::Ready(()));
    }
}
