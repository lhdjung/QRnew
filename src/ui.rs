// SPDX-License-Identifier: MPL-2.0

//! QRnew's interface, as HTML and CSS over `qrnew-core`.
//!
//! `Qr::new` takes the text, the error correction level and a [`QrStyle`], and
//! hands back one document that the preview, both exports and the clipboard all
//! come out of. The core is untouched by any of this.
//!
//! # The preview is the file, in two layers
//!
//! The preview reaches the screen as **the document's own markup** dropped into
//! the stage, which Blitz parses into an `<svg>` and hands to `usvg` — the same
//! parser `qrnew-core` rasterizes with, so the bytes on screen are the bytes in
//! the saved file. `draw.rs` writes every colour as a presentation attribute,
//! which is what makes an exported file stand on its own where CSS cannot reach
//! inside an SVG in Blitz.
//!
//! A code with an inset arrives as **two** layers: the document without the
//! picture, and the picture laid into the hole the document left for it.
//! `anyrender_vello_hybrid` keys its GPU atlas by an identity counter and frees
//! nothing, so a picture arriving inside a *new* document on every keystroke
//! fills the atlas and ends in `AtlasLimitReached` two crates down. Out here the
//! picture is one `<img>` whose `src` does not change while the picture does
//! not. Markup rather than a `data:` URL is the same leak in Blitz's own image
//! cache — `blitz-atlas.md` has both measurements.
//!
//! The seam is held by `Qr::inset_box` and by
//! `the_picture_on_the_stage_is_where_the_document_puts_it`. What is saved and
//! copied never goes through any of this: it is `Qr::svg`, one document.
//!
//! # The rest
//!
//! Three columns — a rail of controls, the stage, a second rail — so nothing in
//! a rail can move the code. Height is the scarce thing; the arithmetic is in
//! `ui.css` and `no_control_is_below_the_fold` checks it.
//!
//! The appearance is a class on `.app` rather than a media query, because nothing an
//! app can call from a component moves `prefers-color-scheme`: see [`Appearance`].
//! Text editing on macOS is split between AppKit and Blitz, and both halves had
//! to be arranged for: [`open_the_text_input_client`], [`appkit_has_this_key`],
//! and `blitz-mac-keys.md` for what this app cannot fix. The caret blink is the
//! app's, because the renderer has no clock: [`Caret`].

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_native::winit::event::{ElementState, WindowEvent};
use dioxus_native::winit::keyboard::{Key as WinitKey, NamedKey};
use dioxus_native::winit::window::{Theme as WinitTheme, Window as WinitWindow};
use qrnew_core::{
    ErrorCorrection, Finder, FinderShape, ImageFormat, Logo, ModuleShape, Qr, QrError, QrStyle,
    ReadError, Rgb,
};

use crate::fl;
use crate::themes;

/// The fewest pixels a module gets in a saved or copied image.
///
/// A floor rather than the answer: see [`export_scale`].
const EXPORT_SCALE: u32 = 10;

/// The least a saved or copied code is across, in pixels, border included.
///
/// **Pixels per module on its own is the wrong unit to export in.** It makes
/// the file's size a function of how much text was typed, and the shortest
/// input is the commonest one: two dozen characters is a twenty-one-module
/// code, which at [`EXPORT_SCALE`] saved as a 250-pixel picture — 21mm on
/// paper at 300dpi. Nothing was lost on the way out and the file is not
/// compressed; there was simply not much of it. Somebody saving a code cares
/// that it is big enough to use, not how many modules went into it.
const MIN_EXPORT_PX: u32 = 1000;

/// The most a saved or copied code is across, in pixels, border included.
///
/// Only a picture in the middle ever asks for this much, and this is where it
/// stops being asked: the rasterizer holds the whole thing at once, which is
/// 36 MB of pixmap here, and 380 MB at the size the smallest inset would
/// otherwise want. Past this the file costs more than the detail is worth.
const MAX_EXPORT_PX: u32 = 3000;

/// The least the code's two colours may differ before the app says so, as a
/// difference in [`luminance`] on 0…1.
///
/// **Not the number the reader stops at, deliberately.** Swept against
/// `qrnew_core::read` on a PNG at [`EXPORT_SCALE`], the app's own reader is
/// almost unbounded: a pale foreground on white reads down to a gap of 0.196,
/// a black foreground on a lightening background down to 0.039. It is handed a
/// clean file with square edges and binarizes adaptively; a camera is handed
/// none of that. So this sits at more than twice the reader's floor — a
/// judgement, like [`SAFE_MARGIN`].
///
/// What the sweep settles is that this cannot be a check that the code
/// *decodes*. It decodes long past where anybody could scan it.
pub const SAFE_CONTRAST: f32 = 0.4;

/// How many pixels a module gets when `qr` is saved or copied.
///
/// **A picture in the middle is what decides this.** Everything else in the
/// document is flat rectangles, which come out clean at any scale worth having;
/// the picture is the only fine detail in it, and the only part the app did not
/// draw. Exporting below the picture's own resolution throws away the one thing
/// somebody brought themselves — and it is easy to do without noticing, because
/// the inset is a seventh of the width, so a thousand-pixel code gives a
/// 450-pixel logo 140 pixels to live in.
///
/// So a code with an inset is made wide enough to hold [`MAX_LOGO_SIDE`] in
/// that box — the largest picture the app will take — bounded by
/// [`MAX_EXPORT_PX`], and one without stays at [`MIN_EXPORT_PX`].
///
/// **A whole number of pixels per module**, so every module is the same size as
/// every other and no edge lands halfway through a pixel. That is why the
/// picture is *at least* the size asked for rather than exactly it: rounding the
/// scale up is free, and rounding the size down would put a seam in the code.
fn export_scale(qr: &Qr) -> u32 {
    let across = match qr.inset_box() {
        Some(inset) => (qrnew_core::MAX_LOGO_SIDE as f32 / inset.side) as u32,
        None => MIN_EXPORT_PX,
    };
    across
        .clamp(MIN_EXPORT_PX, MAX_EXPORT_PX)
        .div_ceil(qr.size_in_modules())
        .max(EXPORT_SCALE)
}

/// The narrowest border the app is prepared to vouch for, in modules.
///
/// Two modules of white is enough for a phone camera to find the edge of the
/// code; below that a scan depends on what it was printed on and how steady
/// the hand is. `a_narrow_margin_still_scans` in `qrnew-core` decodes a
/// two-module border at both export sizes.
///
/// One constant and not two: the app opens at the narrowest border it will
/// stand behind, and says so as soon as somebody goes under it.
pub const SAFE_MARGIN: u32 = 2;

/// Width of the blank border the app starts with, in modules.
///
/// The QR standard asks for four (`qrnew_core::DEFAULT_QUIET_ZONE`), which is
/// visibly generous on screen: a third of a small code's width is border. The
/// app opens at [`SAFE_MARGIN`] instead, and the control is right there.
pub const DEFAULT_MARGIN: u32 = SAFE_MARGIN;

/// As wide a border as the stepper will go to.
///
/// Past this the code is a stamp in the middle of an empty page, and the
/// preview stops being a useful picture of what gets saved.
pub const MAX_MARGIN: u32 = 16;

/// How long a button says `Copied.` before it goes back to its own name.
///
/// Left up, a confirmation stops being news and becomes the button's name.
/// Three seconds is long enough to be read by somebody whose eyes were on the
/// code, and gone before the next thing anybody does.
pub const CONFIRM_FOR: Duration = Duration::from_secs(3);

/// Half a blink: how long the caret is drawn, and then how long it is not.
///
/// 530ms is what GTK, Qt and AppKit all ship.
pub const CARET_BEAT: Duration = Duration::from_millis(530);

/// How long to wait after a file dialog closes before changing anything on
/// screen.
///
/// **This is a workaround for an upstream crash**, written up in
/// `blitz-hit-test.md`. Removing a node leaves its id in its parent's
/// `paint_children` until the next `resolve`, and `Node::hit_inner` unwraps
/// `tree().get(id)` — so a pointer event delivered between a removal and the
/// next redraw takes the window with it. Every ordinary click is safe because
/// the redraw lands first; a modal file dialog is not, because it parks a burst
/// of pointer motion that winit delivers all at once the moment the panel goes
/// away, and the mutation rides in ahead of it.
///
/// Waiting hands that burst an unchanged tree and puts the mutation on an empty
/// queue, which is exactly the situation an ordinary click is already in. A
/// twentieth of a second is invisible after a file dialog.
///
/// ponytail: a delay, not a guarantee. Delete it — and the two `await`s that
/// use it — the day `remove_node` keeps `paint_children` honest upstream.
const SETTLE: Duration = Duration::from_millis(50);

/// The blinking caret, and the two things a text field has to tell it.
///
/// **The blink is the app's, because the renderer has no clock.** Blitz paints
/// the focused input's caret on every frame it draws, in `caret-color`, and
/// never asks for a frame on its own account. So [`metronome`] beats, this
/// toggles, `.app` gains or loses `caret-dark`, and `ui.css` turns
/// `caret-color` transparent for half of every beat.
///
/// `fields` is how many text inputs hold the keyboard — zero or one — and
/// while it is zero the beat writes nothing, so the window is not redrawn
/// twice a second for a caret nobody is looking at. `struck` counts
/// keystrokes: a beat that finds one since the last lights the caret rather
/// than toggling it, because a caret that blinks out mid-word has lost the
/// place.
#[derive(Debug, Clone, Copy)]
struct Caret {
    lit: Signal<bool>,
    fields: Signal<u32>,
    struck: Signal<u64>,
}

/// The caret, made once by [`App`] and handed to every field through a
/// context — including the hex field, which is inside [`Picker`].
fn use_caret() -> Caret {
    let caret = Caret {
        lit: use_signal(|| true),
        // One, from the first frame: the window opens with the content field
        // holding the keyboard. **Blitz's autofocus does not raise `focusin`**
        // — it moves the focus from inside the mutator, after the tree is
        // built — so the one field with the keyboard before anybody touches
        // anything is the one that never announces it, and counting from zero
        // would leave the caret solid until the first click elsewhere.
        fields: use_signal(|| 1u32),
        struck: use_signal(|| 0u64),
    };
    use_context_provider(|| caret);

    // The beat. One task for the life of the window, writing only while there
    // is a caret on screen to write about.
    use_future(move || async move {
        let clock = metronome(CARET_BEAT);
        let mut caret = caret;
        let mut seen = 0u64;
        loop {
            clock.tick().await;
            caret.beat(&mut seen);
        }
    });

    caret
}

impl Caret {
    /// What the class list on `.app` gains for the dark half of a beat.
    fn class(&self) -> &'static str {
        if *self.lit.read() { "" } else { " caret-dark" }
    }

    /// A field took the keyboard.
    fn arrived(&mut self) {
        *self.fields.write() += 1;
        self.lit.set(true);
    }

    /// And gave it back. Saturating, because Blitz can clear the focus without
    /// the field that had it ever hearing a `focusout` — see
    /// `clicking_a_chip_blurs_the_field`.
    fn left(&mut self) {
        let now = self.fields.peek().saturating_sub(1);
        self.fields.set(now);
        if now == 0 {
            self.lit.set(true);
        }
    }

    /// Somebody typed.
    fn struck(&mut self) {
        *self.struck.write() += 1;
    }

    /// One beat. `seen` is the caller's memory of the last keystroke, which
    /// turns "something was typed" into "typed *since the last beat*".
    ///
    /// Every read is a `peek`: this runs inside a task that would otherwise
    /// subscribe to the signals it is about to write and wake itself forever.
    fn beat(&mut self, seen: &mut u64) {
        let struck = *self.struck.peek();
        let lit = *self.lit.peek();
        let want = if *self.fields.peek() == 0 {
            true
        } else if struck != *seen {
            *seen = struck;
            true
        } else {
            !lit
        };
        if want != lit {
            self.lit.set(want);
        }
    }
}

/// The middle of the margin field, measured from inside its left border.
///
/// Half the width `ui.css` gives `.count`, less the one-pixel border. It is
/// here rather than in the stylesheet because **Blitz ignores `text-align`
/// inside an `<input>`**: the field's text belongs to a `parley` editor handed
/// a size, a line height and a colour, and no font at all. So the centring is
/// arithmetic over `padding-left` and the field's own `ch`, and
/// `the_margin_number_is_centered_in_its_field` keeps this number and that
/// width together.
///
/// The two answers to how wide a digit is disagree — Stylo resolves `1ch` to
/// 10.000 points, the shaper paints 9.455 (`blitz-fonts.md`) — which lands
/// half a point on screen, under the test's tolerance. The day the editor is
/// handed its font, remeasure this.
const COUNT_MIDDLE: f32 = 30.0;

/// The colours offered in the picker.
///
/// Two rows of eight: a grey ramp, and hues dark enough to still read as the
/// dark side of a code. Nothing here would trouble a scanner as a foreground
/// on white.
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
/// For `--fill`, so the app can be measured with a code on screen without
/// anybody typing one. An absent context means an empty field.
#[derive(Clone)]
pub struct Fill(pub String);

/// The picture in the middle of the code, as it was picked.
///
/// The bytes rather than the path: the file is read once and then belongs to
/// the app, so a picture moved or edited on disk cannot change a code somebody
/// already made. The format comes off those same bytes —
/// `ImageFormat::detect` looks at what is in the file, not what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Inset {
    name: String,
    format: ImageFormat,
    bytes: Vec<u8>,
}

impl Inset {
    /// Reads a file, if it turns out to be a picture.
    ///
    /// `None` covers both ways this goes wrong — the file would not open, and
    /// it opened but is not an image — because the card says the same thing
    /// about either and neither is something the person can act on.
    fn read(path: &std::path::Path) -> Option<Self> {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        Self::adopt(name, std::fs::read(path).ok()?)
    }

    /// The same picture, already in hand.
    ///
    /// The other way in: a theme carries the bytes it was saved with, and
    /// they go through the same detection and the same shrink — because a
    /// theme folder is a place somebody can drop a photograph by hand.
    fn adopt(name: String, bytes: Vec<u8>) -> Option<Self> {
        // What the file *is*, not what it is called: a dialog filter is a
        // convenience and an extension is a claim, so the bytes decide.
        let format = ImageFormat::detect(&bytes)?;

        // And then, once, whatever scaling the picture needs. A photograph is
        // several thousand pixels across — detail no export can use, re-decoded
        // on every redraw, and past four thousand more than a GPU texture atlas
        // will take at all. `vello_hybrid` does not draw such an image smaller,
        // it unwraps the refusal and **the window closes**. So it happens here,
        // at the one moment a picture arrives, rather than in the memo that
        // redraws on every keystroke.
        let (format, bytes) = match qrnew_core::shrink_logo(&bytes) {
            Some(scaled) => (ImageFormat::Png, scaled),
            None => (format, bytes),
        };

        Some(Self {
            name,
            format,
            bytes,
        })
    }
}

/// A button's `Copied.`, which raises itself and then lets go on its own.
///
/// Two signals rather than one flag so a second copy while the first is still
/// confirmed can tell the two countdowns apart: `serial` numbers the copies,
/// and a countdown that finds a newer number has been overtaken. Without it
/// the first click's timer would cut the second click's confirmation short.
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
/// [`Fill`]'s sibling, for measurement — a code with an inset is a second
/// image decoded and composited on every redraw — and because **a native file
/// dialog is the one control neither a test nor a scripted run can touch**.
/// Without a way in, everything downstream of a picture (the thumbnail, error
/// correction locked at 30%, the too-long message) is reachable only by hand.
///
/// A path rather than bytes: `main.rs` is given one on the command line. See
/// [`Inset::read`].
#[derive(Clone)]
pub struct Inlay(pub String);

/// The appearance to open in, provided as a root context by `main.rs`.
///
/// For `--appearance`: the choice is behind a button and a sheet, so there is no
/// other way to photograph a dark window. An absent context is
/// [`Appearance::System`].
#[derive(Clone)]
pub struct Tone(pub Appearance);

/// Somewhere to write the appearance down, provided as a root context by `main.rs`.
///
/// Handed in rather than reached for: a test clicks through the appearance sheet
/// several times, and a component that saved to disk would edit the settings
/// of whoever ran it. An absent context is an app that does not remember —
/// which is also what a machine with no writable home gets.
#[derive(Clone)]
pub struct Remember(pub Arc<dyn Fn(Appearance) + Send + Sync>);

/// Where saved themes live, provided as a root context by `main.rs`.
///
/// Handed in rather than reached for, for [`Remember`]'s reason doubled: the
/// tests save and delete themes, and a component that knew its own path would
/// be editing the themes of whoever ran them. An absent context is an app with
/// no themes — which is also what a machine with no writable home gets, and
/// the topbar simply has one button fewer.
#[derive(Clone)]
pub struct Themes(pub std::path::PathBuf);

/// Which of the two colours the picker is pointed at.
///
/// Not an `Option`: the picker is always on screen, and the wells choose what
/// it edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Well {
    Dark,
    Light,
}

/// How the code's own marks are drawn.
///
/// **One choice for two of `qrnew-core`'s knobs, deliberately.** The core's
/// [`ModuleShape`] and [`FinderShape`] are independent — six pairings, which
/// `every_combination_of_shapes_scans` walks — but six is not a question worth
/// putting to somebody making one code, and rounded modules inside square
/// finders is the pairing nobody picks on purpose. So the app offers the three
/// that are a *look* and the finders follow the modules.
///
/// None of them can make a code that fails to scan: a scanner reads the colour
/// at a module's centre, every shape covers its own centre, and
/// `every_combination_of_shapes_scans` and
/// `..._with_a_logo_in_the_way` decode all of them with a real reader.
///
/// Scanning quickly is another matter, and the card says so for anything but
/// [`Square`]: a rounded or dotted code gives the decoder fewer clean edges, so
/// a phone takes visibly longer to lock on. That cost is paid at the camera,
/// where the decoding tests cannot see it.
///
/// [`Square`]: Look::Square
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Look {
    /// Square modules and square finders: the code as the standard draws it.
    #[default]
    Square,
    /// Corners taken off wherever no neighbour fills them in, so runs of
    /// modules merge into strokes, with the finders softened to match.
    Rounded,
    /// A circle per module, and the same softened finders — which stay whole
    /// rather than breaking into dots, because a finder is the one part of a
    /// code a scanner looks for before it can read anything.
    Dots,
}

impl Look {
    /// The three, in the order the row offers them.
    const ALL: [Self; 3] = [Self::Square, Self::Rounded, Self::Dots];

    /// The name this look goes by in the markup.
    ///
    /// A `data-look` for the tests to select on, like [`Appearance::slug`]: a test
    /// that clicked the visible label would pass in English and nowhere else.
    const fn slug(self) -> &'static str {
        match self {
            Look::Square => "square",
            Look::Rounded => "rounded",
            Look::Dots => "dots",
        }
    }

    /// The look `name` names, for a theme read back off disk. An unknown word
    /// is the default, like [`InsetSize::named`].
    fn named(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|look| look.slug() == name)
            .unwrap_or_default()
    }

    /// The outline each module is given.
    const fn module(self) -> ModuleShape {
        match self {
            Look::Square => ModuleShape::Square,
            Look::Rounded => ModuleShape::Rounded,
            Look::Dots => ModuleShape::Dot,
        }
    }

    /// The outline the three corner squares are given.
    ///
    /// Colours stay at the core's default, the code's own dark: a finder in a
    /// second colour is a fourth colour control in the tallest rail in the
    /// window, and it is the one part of a code that has to stay findable.
    const fn finder(self) -> Finder {
        let shape = match self {
            Look::Square => FinderShape::Square,
            Look::Rounded | Look::Dots => FinderShape::Rounded,
        };
        Finder {
            shape,
            ring: None,
            center: None,
        }
    }
}

/// The four levels, in the order the row offers them, each with the name it
/// goes by in the markup and in a saved theme.
///
/// A list rather than four inherent methods because [`ErrorCorrection`] is
/// `qrnew-core`'s type: the *words* are the interface's business, like
/// [`Look::slug`], and this is the one place they are written down.
const LEVELS: [(ErrorCorrection, &str); 4] = [
    (ErrorCorrection::Low, "low"),
    (ErrorCorrection::Medium, "medium"),
    (ErrorCorrection::Quartile, "quartile"),
    (ErrorCorrection::High, "high"),
];

/// The name `level` goes by.
fn level_slug(level: ErrorCorrection) -> &'static str {
    LEVELS
        .into_iter()
        .find(|(known, _)| *known == level)
        .map_or("medium", |(_, name)| name)
}

/// The level `name` names, for a theme read back off disk. An unknown word is
/// the app's own default, which is what a hand-edited file gets.
fn level_named(name: &str) -> ErrorCorrection {
    LEVELS
        .into_iter()
        .find(|(_, known)| *known == name)
        .map_or(ErrorCorrection::Medium, |(level, _)| level)
}

/// How much of the code the picture in the middle of it takes up.
///
/// Three named sizes rather than a number, because the top of the useful range
/// is not a constant: a logo has to clear the three finder patterns, which sit
/// eight modules in from each edge whatever the version — so the smallest code
/// (twenty-one modules) takes a shade over a fifth of its width, and anything
/// longer takes a third. A percentage field would spend most of its range on
/// values that only work for some of what somebody might type.
///
/// [`Medium`] is [`Logo::DEFAULT_SIZE`] and always fits
/// (`the_default_logo_fits_even_the_smallest_code`). [`Large`] does not, and
/// the app falls back to the middle size when it will not — see
/// `Drawn::capped`.
///
/// [`Medium`]: InsetSize::Medium
/// [`Large`]: InsetSize::Large
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum InsetSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl InsetSize {
    /// The three, in the order the row offers them.
    const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    /// The name this size goes by in the markup, for the tests to select on
    /// and for a theme to file it under.
    const fn slug(self) -> &'static str {
        match self {
            InsetSize::Small => "small",
            InsetSize::Medium => "medium",
            InsetSize::Large => "large",
        }
    }

    /// The size `name` names, for a theme read back off disk.
    ///
    /// A word that is not one of the three is the default, which is the size
    /// that fits every code — a hand-edited file is not worth refusing over.
    fn named(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|size| size.slug() == name)
            .unwrap_or_default()
    }

    /// Side of the picture as a fraction of the code's width, quiet zone not
    /// counted — which is what [`Logo::size`] means.
    ///
    /// An eighth and a quarter around the core's own sixth, spaced so each step
    /// is a visible change: a quarter is twice the *area* of an eighth.
    fn fraction(self) -> f32 {
        match self {
            InsetSize::Small => 1.0 / 8.0,
            InsetSize::Medium => Logo::DEFAULT_SIZE,
            InsetSize::Large => 1.0 / 4.0,
        }
    }
}

/// The code as it was actually drawn.
///
/// `capped` exists because a control can ask for something the code cannot
/// give: [`InsetSize::Large`] does not fit a twenty-one-module code, which is
/// what a few characters plus an inset produces. `qrnew-core` refuses outright
/// — only the caller knows whether to give up the size or the picture — and
/// this app gives up the size. Drawing nothing would be the one certainly
/// wrong answer: the text is fine and the picture is fine.
#[derive(Debug, Clone, PartialEq)]
struct Drawn {
    qr: Qr,
    /// Whether the picture had to be drawn at [`InsetSize::Medium`] because
    /// the size that was asked for did not fit.
    capped: bool,
}

/// Which palette the window is painted in, and who decides.
///
/// [`Appearance::System`] is the default and the only one that hands the decision
/// back to the desktop and follows it live. The other two overrule it, which is
/// worth being able to do: a code is judged against the surface around it, and
/// that is usually the paper it will be printed on rather than whatever the
/// desktop is set to after dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// Whatever the desktop says, changing when the desktop changes.
    System,
    Light,
    Dark,
}

impl Appearance {
    /// The three, in the order the sheet offers them.
    const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The name this appearance goes by in the markup.
    ///
    /// Both half of the class on `.app` — `appearance-{slug}`, which every rule
    /// with a palette in it hangs off — and the `data-appearance` a test selects by. A
    /// test that clicked the visible label would pass in English only.
    pub const fn slug(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    /// The appearance `name` names, for `--appearance` on the command line.
    ///
    /// The same three words the sheet's buttons carry as `data-appearance`.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|appearance| appearance.slug() == name)
    }

    /// What winit is asked to make the title bar.
    ///
    /// `None` is "stop holding an opinion": it clears any appearance the app
    /// set, putting the window back under the desktop — and, on macOS, starts
    /// the `ThemeChanged` events flowing again so `prefers-color-scheme` is
    /// live for the `appearance-system` branch of the stylesheet.
    const fn window(self) -> Option<WinitTheme> {
        match self {
            Appearance::System => None,
            Appearance::Light => Some(WinitTheme::Light),
            Appearance::Dark => Some(WinitTheme::Dark),
        }
    }
}

/// Hand the window to macOS's text input system, which is where its editing keys
/// live.
///
/// **Without this, `Backspace` does nothing at all.** AppKit does not send a
/// window "the Backspace key"; it sends `deleteBackward:`, resolved from the
/// user's own key map, and likewise `moveWordLeft:` and the rest. Blitz
/// implements all of them and hears none, because AppKit only resolves a key
/// into a command for a window whose text input client is on. `blitz-dom` asks
/// on focus through `ShellProvider::set_ime_enabled`, and at the pinned revision
/// the request does not arrive.
///
/// So the app asks once, declaring no extras. Asking early costs nothing: the
/// request that would turn it off on blur is a no-op in `winit` here. Dead keys
/// and the composition window arrive with it, which
/// `composed_text_reaches_the_field` tests.
///
/// `let _`: a window already asked answers `AlreadyEnabled`, not a failure.
#[cfg(target_os = "macos")]
fn open_the_text_input_client(window: &Arc<dyn WinitWindow>) {
    use dioxus_native::winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest};

    let asking = ImeEnableRequest::new(ImeCapabilities::new(), Default::default())
        .expect("a request that declares no capabilities asks for nothing it did not declare");
    let _ = window.request_ime_update(ImeRequest::Enable(asking));
}

/// Let AppKit be the only thing editing a text field, on the keys it has an
/// opinion about.
///
/// The other half of [`open_the_text_input_client`]: **`winit` delivers such a
/// key twice**, once as the command it resolved and again as a plain
/// `KeyboardInput`, so an app that does not implement the command still sees the
/// key. Blitz implements both, so one press of Left moved the caret two
/// characters. Cancelling the key event leaves the command holding it, which is
/// the right way round — the command knows the *user's* key map.
///
/// The list is exactly the keys AppKit resolves into an editing command. `Tab`
/// is not on it (Blitz moves the focus first, so cancelling would strand the
/// keyboard), nor `Escape`, which closes sheets.
///
/// **Nothing held with Cmd is on it, and this cannot fix that.**
/// `NSTextInputContext` declines to interpret a key event with Command down, so
/// no command arrives for Cmd+Left even though `blitz-dom` implements
/// `moveToLeftEndOfLine:`. Cmd is left to Blitz, where it moves by word: the
/// wrong binding for a Mac, but better than a key that does nothing.
/// `blitz-mac-keys.md` is the report and the patch; with it applied, delete this.
#[cfg(target_os = "macos")]
fn appkit_has_this_key(event: &Event<KeyboardData>) {
    if event.modifiers().contains(Modifiers::SUPER) {
        return;
    }
    if matches!(
        event.key(),
        Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::Backspace
            | Key::Delete
            | Key::Enter
    ) {
        event.prevent_default();
    }
}

/// Everywhere else, the key event is all there is and it is already right.
#[cfg(not(target_os = "macos"))]
fn appkit_has_this_key(_: &Event<KeyboardData>) {}

#[component]
pub fn App() -> Element {
    let mut input = use_signal(|| {
        dioxus_core::try_consume_context::<Fill>().map_or_else(String::new, |fill| fill.0)
    });
    let mut ec = use_signal(|| ErrorCorrection::Medium);
    let mut dark = use_signal(|| Rgb::BLACK);
    let mut light = use_signal(|| Rgb::WHITE);
    let mut margin = use_signal(|| DEFAULT_MARGIN);
    let mut look = use_signal(Look::default);
    // The stepper's field keeps its own text for the same reason the hex field
    // does: half-typed input is not a number, and a field rewritten from the
    // value on every keystroke cannot be emptied to type a new one into.
    let mut margin_draft = use_signal(|| DEFAULT_MARGIN.to_string());
    // What was decoded out of an image, and whether it has been copied since.
    //
    // Shown under the button rather than dropped into the field: reading a code
    // and writing one are two different errands.
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
    let mut inset_size = use_signal(InsetSize::default);
    let mut editing = use_signal(|| Well::Dark);
    // **What the colour caution said when a pointer took hold of the picker**,
    // or `None` when nothing is holding it, in which case the caution is read
    // live off the two wells. The argument for freezing it is beside the
    // banner, down in the colours rail.
    //
    // Written from `Picker`'s pointer handlers rather than from an effect
    // watching the colours: an effect here would run on every colour change,
    // and `reset_puts_black_and_white_back` says why that is not free — the
    // picker follows an outside colour through an effect of its own, and a
    // second effect writing a signal `App` renders from starves it for a frame.
    let mut held_caution = use_signal(|| None::<bool>);
    let take_the_caution = move |holding: bool| {
        held_caution.set(holding.then(|| contrast(dark(), light()) < SAFE_CONTRAST));
    };
    // **The two colours change places.** A code drawn light on dark is the one
    // variation somebody reaches for that is not a new colour at all, and
    // reaching it by hand was four steps.
    //
    // It cannot make the code unscannable, which is what lets it sit beside the
    // hex field rather than under a caution: `contrast` is a distance between
    // luminances, and a distance does not care which way round it is measured.
    // The *code* does change — a scanner is not symmetric — and `qrnew-core`
    // draws and reads both ways round; the README has that story.
    let swap = move |()| {
        let (was_dark, was_light) = (dark(), light());
        dark.set(was_light);
        light.set(was_dark);
    };
    let mut about = use_signal(|| false);
    // Where saved looks live, or `None` on a machine that has nowhere to put
    // them — in which case the button that opens them is not drawn at all.
    let library = use_hook(|| dioxus_core::try_consume_context::<Themes>().map(|Themes(dir)| dir));
    // Read once at mount and rewritten by the two things that change the
    // folder, so the sheet does not walk the disk on every render.
    let mut saved = use_signal(|| library.as_deref().map_or_else(Vec::new, themes::list));
    let mut themes_sheet = use_signal(|| false);
    // The name a theme is about to be saved under. Its own draft, like the
    // margin and hex fields: it is emptied by saving rather than by anything
    // the app computes.
    let mut theme_name = use_signal(String::new);
    // **Which theme has been asked about, not which is being deleted.** A theme
    // is a picture somebody chose and a colour they matched by eye, and the
    // cross that takes it away sits a few points from the row that applies it —
    // so the cross asks, and the row it is on answers in place. One at a time:
    // opening a second question closes the first.
    let mut condemned = use_signal(|| None::<String>);
    // Why the last folder offered for importing was not a theme, if it was not.
    // **Two answers rather than one flag**, because they are two different
    // things to do about it: a folder with no settings file in it is the wrong
    // folder, and a settings file with no name in it is a file to fix.
    let mut import_error = use_signal(|| None::<themes::NotATheme>);
    // Neither a question nor a complaint outlives the sheet it was made in. One
    // effect rather than a line in each of the five ways out — the cross, the
    // scrim, Escape twice over, and the button in the top bar.
    use_effect(move || {
        if !themes_sheet() {
            condemned.set(None);
            import_error.set(None);
        }
    });

    // Both stepper buttons, the field itself and an applied theme go through
    // here, so the number and the text under it cannot disagree about what the
    // margin is.
    let mut set_margin = move |next: u32| {
        let next = next.min(MAX_MARGIN);
        margin.set(next);
        margin_draft.set(next.to_string());
    };

    // **A theme is the whole look, including its absence.** One with no
    // picture takes the picture away, because otherwise applying two themes in
    // a row would leave a mark from the first inside the second's colours, and
    // nothing on screen would say where it came from.
    let mut apply = move |theme: themes::Theme| {
        dark.set(theme.mark());
        light.set(theme.ground());
        set_margin(theme.margin.unwrap_or(DEFAULT_MARGIN));
        look.set(Look::named(theme.shape.as_deref().unwrap_or_default()));
        // A *preferred* level, and the one thing in a theme the app is allowed
        // to overrule: a picture in the middle needs 30%, so the row shows High
        // and says why. The preference is kept rather than rewritten, so taking
        // the picture away gives it back.
        ec.set(level_named(theme.error_correction.as_deref().unwrap_or_default()));
        // A *preferred* size: the largest does not fit a short code, and the
        // memo below draws the largest that does — see `Drawn::capped`.
        inset_size.set(InsetSize::named(theme.image_size.as_deref().unwrap_or_default()));
        inset.set(
            theme
                .image_file
                .and_then(|(name, bytes)| Inset::adopt(name, bytes)),
        );
        inset_error.set(false);
        // Two clicks was the ask: the sheet, then the theme.
        themes_sheet.set(false);
    };

    // The other direction: what is on screen now, filed under a name.
    //
    // The list is re-read off disk rather than pushed onto, so what the sheet
    // shows is what a later run will find — including the case where the write
    // silently failed.
    let save_theme = {
        let dir = library.clone();
        move |_| {
            let name = theme_name().trim().to_string();
            let Some(dir) = dir.as_deref() else { return };
            if name.is_empty() {
                return;
            }
            // **A theme holds what it changes.** A control left where the app
            // opened it is a key the file does not need: an absent one already
            // means that value, and every line written is a line somebody
            // reading the theme by hand has to take in.
            let changed = |changed: bool, word: &str| changed.then(|| word.to_string());
            themes::save(
                dir,
                &themes::Theme {
                    name,
                    foreground: (dark() != Rgb::BLACK).then_some(dark()),
                    background: (light() != Rgb::WHITE).then_some(light()),
                    image_file: inset
                        .read()
                        .as_ref()
                        .map(|chosen| (chosen.name.clone(), chosen.bytes.clone())),
                    image_size: changed(
                        inset_size() != InsetSize::default(),
                        inset_size().slug(),
                    ),
                    // The preference, not what the code was drawn at: a theme
                    // saved while an inset held the row at 30% would otherwise
                    // come back as a theme that asks for 30% forever.
                    error_correction: changed(
                        ec() != ErrorCorrection::Medium,
                        level_slug(ec()),
                    ),
                    shape: changed(look() != Look::default(), look().slug()),
                    margin: (margin() != DEFAULT_MARGIN).then_some(margin()),
                },
            );
            saved.set(themes::list(dir));
            theme_name.set(String::new());
            condemned.set(None);
        }
    };

    // A theme somebody else made: the same folder this app writes, picked with
    // the platform's own dialog and copied in beside the rest. The list is
    // re-read off disk afterwards for `save_theme`'s reason — what the sheet
    // shows is what a later run will find.
    let import_theme = {
        let library = library.clone();
        move |_| {
            let library = library.clone();
            spawn(async move {
                let Some(handle) = rfd::AsyncFileDialog::new().pick_folder().await else {
                    return;
                };
                // Before anything on screen changes: see `SETTLE`.
                after(SETTLE).await;
                let Some(dir) = library.as_deref() else { return };
                match themes::import(dir, handle.path()) {
                    Ok(()) => {
                        import_error.set(None);
                        saved.set(themes::list(dir));
                    }
                    Err(why) => import_error.set(Some(why)),
                }
            });
        }
    };

    let mut appearance = use_signal(|| {
        dioxus_core::try_consume_context::<Tone>().map_or(Appearance::System, |Tone(seed)| seed)
    });
    let mut appearance_sheet = use_signal(|| false);
    let mut caret = use_caret();
    let remember = use_hook(dioxus_core::try_consume_context::<Remember>);

    // The title bar belongs to the platform, which will not read a class off
    // `.app`. Cosmetic: a compositor that declines leaves a title bar that does
    // not match, and the window under it is right either way.
    //
    // The window arrives as a context, and there is not one in a test — the
    // harness builds the document with no window at all.
    let window = use_hook(dioxus_core::try_consume_context::<Arc<dyn WinitWindow>>);
    let windowed = window.is_some();
    // The editing keys, on the one platform that does not send them as keys.
    // There is no window in a test, which is what the `Option` is for here and
    // in every other hook that asks for one.
    #[cfg(target_os = "macos")]
    {
        let asking = window.clone();
        use_hook(move || {
            if let Some(window) = &asking {
                open_the_text_input_client(window);
            }
        });
    }
    use_effect(move || {
        if let Some(window) = &window {
            window.set_theme(appearance().window());
        }
    });

    // **Escape closes whichever sheet is open**, answered twice because the two
    // answers cover different halves of *where the keyboard is when the key is
    // pressed*.
    //
    //   * `onkeydown` on `.app`, below: Blitz sends a key to the focused node
    //     and lets it bubble, so this catches every keystroke made while the
    //     keyboard is inside the interface, and it is the half the headless
    //     tests can drive.
    //
    //   * This one, on the window. `clicking_a_chip_blurs_the_field` records the
    //     rule that makes it necessary: a click matching none of Blitz's known
    //     controls *clears* the focus, so after clicking a appearance the keyboard is
    //     on `<html>`, above `.app`, and bubbles away from it.
    //
    // The gate is constant for the life of the component, so hook order does not
    // change under it; upstream's `use_window_event` consumes the window context
    // rather than trying for it, and there is no window in a test.
    if windowed {
        dioxus_native::use_window_event(move |event, _| {
            if let WindowEvent::KeyboardInput { event, .. } = event
                && event.state == ElementState::Pressed
                && !event.repeat
                && event.logical_key == WinitKey::Named(NamedKey::Escape)
            {
                appearance_sheet.set(false);
                themes_sheet.set(false);
                about.set(false);
            }
        });
    }

    // The one generated code, which everything downstream is a view of. A memo
    // rather than a signal written from four handlers: the inputs say what the
    // code is, so nothing can set them and forget to redraw.
    let code = use_memo(move || {
        let text = input();
        if text.is_empty() {
            return None;
        }
        let picture = inset.read().as_ref().map(|chosen| chosen.bytes.clone());
        let asked = inset_size().fraction();
        let mut style = QrStyle {
            dark: dark(),
            light: light(),
            quiet_zone: margin(),
            module: look().module(),
            finder: look().finder(),
            // Padding and clearing stay at the core's defaults — half a module
            // of air and a square cut-out. The size is the one of the three
            // worth a control.
            logo: picture.clone().map(|bytes| Logo {
                size: asked,
                ..Logo::new(bytes)
            }),
            // No `..QrStyle::default()`: every field is written here.
        };
        match Qr::new(&text, ec(), &style) {
            Ok(qr) => Some(Drawn { qr, capped: false }),
            // The picture does not fit *this* code — a statement about how
            // short the text is, not about the picture. Redrawn at the size
            // that fits every code there is, and the row says so.
            //
            // Guarded on the size actually being the larger one, so a logo
            // failure that is not about size cannot send the app round the
            // same encode twice for the same answer.
            Err(QrError::Logo(_)) if asked > Logo::DEFAULT_SIZE => {
                style.logo = picture.map(Logo::new);
                Qr::new(&text, ec(), &style)
                    .ok()
                    .map(|qr| Drawn { qr, capped: true })
            }
            // An input past what the densest code can hold. The line under the
            // field says so.
            Err(_) => None,
        }
    });

    // Kept apart from `code` so a re-render that changes neither the text nor
    // the colours does not rebuild the document.
    //
    // **Without the picture in it.** The code's document already leaves a hole
    // where the inset goes, and the picture is laid into that hole as an
    // `<img>` of its own on the stage; what is saved and copied is still
    // `Qr::svg`, one document with everything in it. The reason is the GPU
    // atlas leak in `anyrender_vello_hybrid` — see this file's header and
    // `blitz-atlas.md`. Out here the picture is one `<img>` whose `src` does
    // not change while the picture does not, so it is uploaded once.
    //
    // The document is **markup rather than a `data:` URL** for the same
    // reason, in Blitz's own image cache, and it is less work per keystroke.
    let preview = use_memo(move || code().as_ref().map(|drawn| drawn.qr.svg_without_inset()));

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
            // Before anything on screen changes: see `SETTLE`.
            after(SETTLE).await;
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

    // The same formats the reader offers, because it is the same list for the
    // same reason: what `resvg` can draw.
    let choose_inset = move |_| {
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
                .pick_file()
                .await
            else {
                return;
            };
            // Before anything on screen changes: see `SETTLE`.
            after(SETTLE).await;
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
        let Some(Drawn { qr, .. }) = code() else { return };
        spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("PNG Image", &["png"])
                .set_file_name("qrcode.png")
                .save_file()
                .await
            else {
                return;
            };
            if let Ok(png) = qr.to_png(export_scale(&qr)) {
                let _ = std::fs::write(handle.path(), png);
            }
        });
    };

    let save_svg = move |_| {
        let Some(Drawn { qr, .. }) = code() else { return };
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
        let Some(Drawn { qr, .. }) = code() else { return };
        let Ok(raster) = qr.to_rgba(export_scale(&qr)) else {
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
    // redraws the code takes it back.
    use_effect(move || {
        code.read();
        // `Confirmation::lower` peeks rather than reads, so this effect does
        // not subscribe to the flag it clears — subscribing would make it run
        // on the write that raises it and put it straight back down.
        copied_image.lower();
    });

    // Whether there is anything to save, copy or look at. It decides both the
    // stage's content and how the three export buttons are drawn.
    let ready = preview().is_some();

    // Whether an inset is in place — asked three times below, so read once here.
    let has_inset = inset.read().is_some();
    let shown_ec = if has_inset { ErrorCorrection::High } else { ec() };
    // Whether the level the code is drawn at is not the level that was asked
    // for. Only ever one way round: an inset raises the row to 30% and nothing
    // lowers it, so this is true exactly when a picture is in the way of a
    // choice somebody made.
    let ec_overridden = has_inset && ec() != ErrorCorrection::High;
    // Whether the size the row points at is the size the code was drawn at.
    // Read out of the memo rather than recomputed by the app: the encode that
    // already happened is the only thing that knows.
    let capped = code.read().as_ref().is_some_and(|drawn| drawn.capped);

    // Where the picture goes on the stage, as the style the layer over the code
    // wears. The code that says where is the code that drew the hole, so the two
    // cannot disagree. The numbers are fractions of the whole document, quiet
    // zone included, and the layer's box is the document — so a percentage is
    // the whole conversion.
    let spot = code
        .read()
        .as_ref()
        .and_then(|drawn| drawn.qr.inset_box())
        .map(|inset| {
            let (edge, side) = (100.0 * inset.offset, 100.0 * inset.side);
            format!("left: {edge}%; top: {edge}%; width: {side}%; height: {side}%")
        });

    // The line under the field: the prompt while it is empty, nothing while a
    // code is being drawn, and text past what a code can hold. Often silent,
    // which `min-height` in `ui.css` is what makes safe.
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

    // Whether either stepper button has anywhere to go. The margin is clamped
    // at both ends, so at 0 and at [`MAX_MARGIN`] one button answered a press
    // by doing nothing, with no way to tell that from a press that had missed.
    // Dimmed and inert instead — the same answer the error-correction row gives
    // while an inset holds it.
    let can_shrink = margin() > 0;
    let can_grow = margin() < MAX_MARGIN;

    // How far the number in the margin field has to be pushed to sit in the
    // middle of it. Half the field, less half the text: `1ch` is the width of
    // a digit in the field's own font, so this is exact rather than tuned.
    let count_pad = margin_draft.read().chars().count() as f32 * 0.5;

    // Cloned rather than borrowed through the markup: holding a `Ref` while the
    // tree is built is a lock held over a lot of other people's code.
    let inset_name = inset.read().as_ref().map(|chosen| chosen.name.clone());
    // A theme has to be called something: the button is dimmed and inert
    // until the field says what.
    let can_save = !theme_name().trim().is_empty();
    let export_ink = if ready { Ink::Plain } else { Ink::Faint };
    let export_class = if ready { "btn" } else { "btn off" };

    rsx! {
        style { {include_str!("ui.css")} }

            // The appearance is a class here rather than a media query in `ui.css`,
            // and both sheets are inside it: a custom property is inherited, so
            // anything painted in the app's colours has to descend from the
            // element the palette is written on.
        div {
            class: "app appearance-{appearance().slug()}{caret.class()}",
            // Escape, for every keystroke made while the keyboard is inside
            // the interface. The other half is on the window; the whole story
            // is above `use_window_event` in this component.
            onkeydown: move |event| {
                if event.key() == Key::Escape && (appearance_sheet() || themes_sheet() || about()) {
                    appearance_sheet.set(false);
                    themes_sheet.set(false);
                    about.set(false);
                }
            },

            header { class: "topbar",
                div { class: "brand",
                    {glyph(Glyph::Code, Ink::Accent, "glyph-brand")}
                    span { {fl!("app-title")} }
                }
                div { class: "spacer" }
                // Only where there is somewhere to keep them. The other two
                // buttons are always drawn, because neither needs a disk.
                if library.is_some() {
                    button {
                        class: "chrome-btn themes-open",
                        onclick: move |_| themes_sheet.toggle(),
                        {glyph(Glyph::Bookmark, Ink::Plain, "glyph")}
                        span { {fl!("themes")} }
                    }
                }
                button {
                    class: "chrome-btn appearance-open",
                    onclick: move |_| appearance_sheet.toggle(),
                    {glyph(Glyph::Appearance, Ink::Plain, "glyph")}
                    span { {fl!("appearance")} }
                }
                button {
                    class: "chrome-btn about-open",
                    onclick: move |_| about.toggle(),
                    {glyph(Glyph::Info, Ink::Plain, "glyph")}
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
                            onkeydown: move |event| {
                                caret.struck();
                                appkit_has_this_key(&event);
                            },
                            onfocusin: move |_| caret.arrived(),
                            onfocusout: move |_| caret.left(),
                        }
                        // **Blitz has no `placeholder`.** There is no such
                        // attribute in `blitz-dom` at all, so setting one left
                        // the field blank.
                        //
                        // A line under the field rather than the overlay the
                        // theme sheet's name field uses, because it is not only
                        // a prompt: it is also where text too long for a code
                        // says so, and that sentence does not belong on top of
                        // the text it is about. It keeps its height when it has
                        // nothing to say, so the button below never moves.
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
                        // error correction to `High` whenever there is a logo.
                        // The row goes on saying what the code is drawn at,
                        // dimmed and inert. The choice underneath is remembered
                        // and comes back when the inset goes.
                        div { class: "segments",
                            // `data-ec` is the level's own name rather than its
                            // label, because the label is a translation and a
                            // test that selected on it would pass in English
                            // and nowhere else.
                            for (level , name) in LEVELS {
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
                                    span {
                                        match level {
                                            ErrorCorrection::Low => fl!("ec-low"),
                                            ErrorCorrection::Medium => fl!("ec-medium"),
                                            ErrorCorrection::Quartile => fl!("ec-quartile"),
                                            ErrorCorrection::High => fl!("ec-high"),
                                        }
                                    }
                                }
                            }
                        }
                        // Three states, one line: the choice is being
                        // overruled, the choice happens to agree with what the
                        // inset needs, or there is no inset and the row is the
                        // whole story.
                        //
                        // **Read off the state rather than remembered from an
                        // apply**, so it is right however the two came to
                        // disagree — a theme carrying 15% and a picture, or a
                        // picture added to a code already set to 15%. It goes
                        // when the picture goes, and the choice underneath comes
                        // back with it.
                        //
                        // A hint rather than the banner the margin and the shape
                        // wear. Those three say *this code may not scan*, which
                        // is the one thing in the window worth a triangle; 30%
                        // error correction is the app doing the safe thing and
                        // saying which choice it took over to do it.
                        p { class: "hint", "data-ec-note": "{ec_overridden}",
                            {
                                if ec_overridden {
                                    fl!("ec-overridden")
                                } else if has_inset {
                                    fl!("ec-locked")
                                } else {
                                    fl!("ec-hint")
                                }
                            }
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
                                class: if can_shrink { "step" } else { "step off" },
                                "data-margin-less": "true",
                                aria_label: fl!("margin-less"),
                                // Said as well as drawn: a button that is only
                                // dimmed is a button nothing reading the window
                                // aloud has been told about.
                                aria_disabled: if can_shrink { "false" } else { "true" },
                                onclick: move |_| {
                                    if can_shrink {
                                        set_margin(margin() - 1);
                                    }
                                },
                                {glyph(Glyph::Minus, step_ink(can_shrink), "glyph")}
                            }
                            input {
                                class: "count",
                                r#type: "text",
                                "data-margin": "true",
                                // **This is what centres the number.** Blitz
                                // paints an input's text at the left edge of its
                                // content box and never looks at `text-align`,
                                // so padding is the only handle: half the field,
                                // less half the text, in the field's own digit
                                // width.
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
                                // Half-typed input may sit in the field while it
                                // is being typed, but it cannot outlive the
                                // keyboard: the code is drawn at the last number
                                // that parsed, so an empty field left behind
                                // would have the app showing one margin and the
                                // field claiming another.
                                onblur: move |_| margin_draft.set(margin().to_string()),
                                onkeydown: move |event| {
                                    caret.struck();
                                    appkit_has_this_key(&event);
                                },
                                onfocusin: move |_| caret.arrived(),
                                onfocusout: move |_| caret.left(),
                            }
                            button {
                                class: if can_grow { "step" } else { "step off" },
                                "data-margin-more": "true",
                                aria_label: fl!("margin-more"),
                                aria_disabled: if can_grow { "false" } else { "true" },
                                onclick: move |_| {
                                    if can_grow {
                                        set_margin(margin() + 1);
                                    }
                                },
                                {glyph(Glyph::Plus, step_ink(can_grow), "glyph")}
                            }
                        }
                        // Only once somebody has gone below two: a caveat
                        // printed permanently stops being read, and this one
                        // arrives at the moment it applies. Set larger than the
                        // sentence below it, because the line saying a code
                        // might not scan should not be the smallest thing here.
                        //
                        // It *replaces* the hint rather than joining it, which
                        // is what pays for it — two paragraphs pushed this card
                        // off the bottom of the rail in the wider face a Linux
                        // machine picks for `system-ui`. See the budget in
                        // `ui.css`.
                        if margin() < SAFE_MARGIN {
                            p { class: "warn", "data-margin-warning": "true",
                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                span { {fl!("margin-warning")} }
                            }
                        } else {
                            p { class: "hint", {fl!("margin-hint")} }
                        }
                    }
                    // Last in the rail, under the margin: what the code is made
                    // of, after how much air is around it.
                    //
                    // The cost of being last is that this card's caution is the
                    // lower of the two, in a column that scrolls, and it is the
                    // likelier of the two to appear. So the room was found
                    // rather than borrowed from the position — see the budget in
                    // `ui.css`. `a_caution_is_never_the_thing_that_scrolls` is
                    // the promise: what goes under the fold is this card's own
                    // bottom edge, below the sentence rather than instead of it.
                    //
                    // In this rail rather than beside the colours, where it
                    // belongs by subject, because that is the rail with no room.
                    div { class: "card",
                        div { class: "card-head",
                            {glyph(Glyph::Shape, Ink::Accent, "glyph")}
                            span { {fl!("section-shape")} }
                        }
                        div { class: "segments segments-3",
                            for choice in Look::ALL {
                                button {
                                    key: "{choice.slug()}",
                                    class: chip_class(look() == choice, false),
                                    "data-look": "{choice.slug()}",
                                    aria_pressed: if look() == choice { "true" } else { "false" },
                                    onclick: move |_| look.set(choice),
                                    span {
                                        match choice {
                                            Look::Square => fl!("shape-square"),
                                            Look::Rounded => fl!("shape-rounded"),
                                            Look::Dots => fl!("shape-dots"),
                                        }
                                    }
                                }
                            }
                        }
                        // The one thing the test suite cannot tell anybody. All
                        // three shapes decode — `every_combination_of_shapes_
                        // scans` proves it with a real reader — but a rounded or
                        // dotted code gives a camera fewer clean edges, so it
                        // takes longer to focus and lock on. Invisible from
                        // inside the repo; paid by somebody holding a phone up
                        // to a printed code.
                        //
                        // Written the way the margin caution is, and for the
                        // same reason: it has nothing to say while the code is
                        // square.
                        if look() != Look::Square {
                            p { class: "warn", "data-shape-warning": "true",
                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                span { {fl!("shape-warning")} }
                            }
                        }
                    }

                }

                section { class: "stage",
                    if let Some(document) = preview() {
                        // The mat is painted in the code's own background
                        // colour, so the rounded corners belong to the mat and
                        // the image never has to be clipped to them.
                        div {
                            class: "preview",
                            style: "background: {light().to_hex()}; border-color: {mat_line(light())}",
                            // Two layers, one picture. The document is square
                            // and so is this box, so the code fills it exactly
                            // and a percentage inside it is a fraction of the
                            // document — which is the unit `inset_box` speaks.
                            div { class: "code",
                                // The document, dropped in whole. Blitz parses
                                // the markup, sees an `<svg>`, and hands that
                                // element's own serialization to `usvg` — the
                                // same parser a `data:` URL would have reached.
                                // `the_code_on_the_stage_is_the_code_in_the_file`
                                // holds the two documents to being one.
                                //
                                // `dangerous_inner_html` is the only way in, and
                                // the name is about markup from somewhere else;
                                // this is `draw.rs`'s, built from an escaped
                                // string. The `alt` the `<img>` carried moves
                                // here: a `<div>` full of paths is not a picture
                                // to anything reading the window aloud.
                                div {
                                    class: "doc",
                                    "data-preview": "true",
                                    role: "img",
                                    aria_label: fl!("app-title"),
                                    dangerous_inner_html: "{document}",
                                }
                                if let (Some(spot), Some(picture)) = (spot.as_ref(), thumbnail()) {
                                    // No `alt`: the code above already names
                                    // the whole thing, and a screen reader
                                    // meeting this twice would be told about a
                                    // picture it cannot describe anyway.
                                    img {
                                        class: "inset",
                                        "data-preview-inset": "true",
                                        src: "{picture}",
                                        alt: "",
                                        style: "{spot}",
                                    }
                                }
                            }
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
                        // The third caution, in the same banner as the other two
                        // and for the same reason: a choice the app is willing
                        // to make, with a price only it knows about.
                        //
                        // The most reachable of the three — a click on a swatch
                        // four pixels from the one somebody meant. It is about
                        // the *file*; `mat_line`, the window's other answer to a
                        // colour near its neighbour, is about the preview.
                        //
                        // **Held while the picker is being dragged**, because it
                        // is *above* the picker: a banner arriving under a
                        // pointer drawing on the square drops the square half an
                        // inch mid-stroke, and a pointer's `element_coordinates`
                        // are measured against the square — so a hand that has
                        // not moved is suddenly on a different colour, which can
                        // cross back over the threshold and move it again.
                        // Nothing is lost by waiting: the banner arrives when
                        // the button comes up.
                        if held_caution()
                            .unwrap_or_else(|| contrast(dark(), light()) < SAFE_CONTRAST)
                        {
                            p { class: "warn", "data-color-warning": "true",
                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                span { {fl!("color-warning")} }
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
                            Picker { color: dark, onhold: take_the_caution, onswap: swap }
                        } else {
                            Picker { color: light, onhold: take_the_caution, onswap: swap }
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

                    // Under the colours: an inset is the last thing anybody adds
                    // and the first thing they take away, and it is the only
                    // control that changes what another one is allowed to say.
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
                            // How big the picture is drawn, and only once there
                            // is one: a size control over an empty card is a
                            // question about nothing, in the taller rail.
                            //
                            // **Every size is always choosable**, because the
                            // row is a preference rather than an instruction —
                            // the same thing the error-correction row is while
                            // an inset holds it at 30%. A size this code has no
                            // room for is taken, drawn at the middle size, and
                            // shown as held with the hint below saying why; a
                            // few more characters and it is drawn as asked,
                            // with nothing to click again.
                            div { class: "segments segments-3",
                                for choice in InsetSize::ALL {
                                    button {
                                        key: "{choice.slug()}",
                                        class: chip_class(inset_size() == choice, capped && inset_size() == choice),
                                        "data-inset-size": "{choice.slug()}",
                                        aria_pressed: if inset_size() == choice { "true" } else { "false" },
                                        onclick: move |_| inset_size.set(choice),
                                        span {
                                            match choice {
                                                InsetSize::Small => fl!("inset-small"),
                                                InsetSize::Medium => fl!("inset-medium"),
                                                InsetSize::Large => fl!("inset-large"),
                                            }
                                        }
                                    }
                                }
                            }
                            // The size that was asked for did not fit, so the
                            // picture is drawn at the middle size — the one
                            // that fits every code there is. The chip above is
                            // already dimmed; this says why, because a dimmed
                            // control explains itself to nobody.
                            //
                            // Read off the encode rather than off an apply, so
                            // it covers a size chosen by hand and a code that
                            // shrank under one just as well as a theme that
                            // asked for more than this code has room for.
                            //
                            // A hint, for the reason the error-correction line
                            // is one: the triangle belongs to the three cautions
                            // about a code that may not scan, and a picture
                            // drawn smaller than asked scans perfectly well.
                            if capped {
                                p { class: "hint", "data-inset-capped": "true",
                                    {fl!("inset-capped")}
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
                        // What an empty card is for: it says what an inset is
                        // and what taking one costs. Once there is a picture the
                        // thumbnail says the first half and the error-correction
                        // hint is already saying the second.
                        if !has_inset {
                            p { class: "hint", {fl!("inset-hint")} }
                        }
                    }
                }
            }

            // The saved looks. Everything in a theme is on the face of the
            // window already — this sheet only remembers a set of answers and
            // gives them back, which is why it is a sheet rather than a fourth
            // card in a rail that has no room.
            if themes_sheet() {
                div { class: "scrim", onclick: move |_| themes_sheet.set(false),
                    div {
                        class: "sheet themes-sheet",
                        onclick: move |event| event.stop_propagation(),
                        div { class: "sheet-head",
                            h2 {
                                {glyph(Glyph::Bookmark, Ink::Accent, "glyph-brand")}
                                span { {fl!("themes")} }
                            }
                            button {
                                class: "sheet-close themes-close",
                                aria_label: fl!("close"),
                                autofocus: true,
                                onclick: move |_| themes_sheet.set(false),
                                {glyph_hover(Glyph::Close, Ink::Faint, Ink::Danger, "glyph")}
                            }
                        }
                        if saved().is_empty() {
                            p { {fl!("themes-empty")} }
                        } else {
                            div { class: "themes-list",
                                for theme in saved() {
                                    div { key: "{theme.name}", class: "theme-row",
                                        // **The question is asked in the row's
                                        // own place.** A second sheet over this
                                        // one would need a second scrim and a
                                        // second way out; here the thing being
                                        // deleted is still on screen, named, and
                                        // the two answers are the same size.
                                        if condemned() == Some(theme.name.clone()) {
                                            span { class: "theme-asking",
                                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                                span { {fl!("themes-remove-ask", name = theme.name.clone())} }
                                            }
                                            button {
                                                class: "btn danger",
                                                "data-theme-remove-yes": "{theme.name}",
                                                onclick: {
                                                    let name = theme.name.clone();
                                                    let dir = library.clone();
                                                    move |_| {
                                                        let Some(dir) = dir.as_deref() else { return };
                                                        themes::remove(dir, &name);
                                                        saved.set(themes::list(dir));
                                                        condemned.set(None);
                                                    }
                                                },
                                                span { {fl!("themes-remove-yes")} }
                                            }
                                            button {
                                                class: "btn",
                                                "data-theme-remove-no": "{theme.name}",
                                                onclick: move |_| condemned.set(None),
                                                span { {fl!("themes-remove-no")} }
                                            }
                                        } else {
                                            button {
                                                class: "theme-apply",
                                                // The name rather than an index:
                                                // a test selecting on a position
                                                // would pass until somebody saved
                                                // a second theme alphabetically
                                                // before it.
                                                "data-theme": "{theme.name}",
                                                onclick: {
                                                    let chosen = theme.clone();
                                                    move |_| apply(chosen.clone())
                                                },
                                                // The two colours as the thing
                                                // they are: a mark on its own
                                                // ground, which is what a code is.
                                                span {
                                                    class: "theme-swatch",
                                                    style: "background: {theme.ground().to_hex()}; border-color: {mat_line(theme.ground())}",
                                                    span {
                                                        class: "theme-mark",
                                                        style: "background: {theme.mark().to_hex()}",
                                                    }
                                                }
                                                span { class: "theme-name", "{theme.name}" }
                                                // Which picture is in it, if
                                                // there is one — the one part of
                                                // a theme the swatch cannot
                                                // show, and the one that tells
                                                // two otherwise similar themes
                                                // apart.
                                                if let Some((file, _)) = theme.image_file.as_ref() {
                                                    span {
                                                        class: "theme-image",
                                                        "data-theme-image": "{theme.name}",
                                                        "{file}",
                                                    }
                                                }
                                            }
                                            button {
                                                class: "sheet-close theme-remove",
                                                "data-theme-remove": "{theme.name}",
                                                aria_label: fl!("themes-remove"),
                                                onclick: {
                                                    let name = theme.name.clone();
                                                    move |_| condemned.set(Some(name.clone()))
                                                },
                                                {glyph_hover(Glyph::Trash, Ink::Faint, Ink::Danger, "glyph")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Bringing one in, under the ones already here: it
                        // adds a row to the list above rather than changing
                        // anything in the window, so it belongs to the list.
                        button {
                            class: "btn wide",
                            "data-theme-import": "true",
                            onclick: import_theme,
                            {glyph(Glyph::Download, Ink::Plain, "glyph")}
                            span { {fl!("themes-import")} }
                        }
                        if let Some(why) = import_error() {
                            p { class: "error",
                                {
                                    match why {
                                        themes::NotATheme::NoFile => fl!("themes-import-error-no-file"),
                                        themes::NotATheme::NoName => fl!("themes-import-error-no-name"),
                                    }
                                }
                            }
                        }
                        // What the row under it does, in the words of what is
                        // on screen right now — a picture in the middle of the
                        // code is the one part of a theme the sheet cannot
                        // show. Directly over the field it is about, with
                        // nothing in between: a sentence explaining a control
                        // has to be the thing nearest to it.
                        p { class: "sheet-sub", "data-themes-subhead": "true",
                            {
                                if has_inset {
                                    fl!("themes-subhead-image")
                                } else {
                                    fl!("themes-subhead-plain")
                                }
                            }
                        }
                        div { class: "theme-save",
                            // **Blitz has no `placeholder`** — there is no such
                            // attribute in `blitz-dom` — so the prompt is a
                            // sibling laid over the field, held out of the way
                            // of the pointer by `pointer-events: none`. That
                            // property *is* implemented (`Node::hit_inner`
                            // consults it), which is what makes an overlay safe
                            // here where the field's own prompt in the Content
                            // card had to be a line underneath.
                            div { class: "ghosted",
                                input {
                                    class: "field",
                                    r#type: "text",
                                    "data-theme-name": "true",
                                    aria_label: fl!("themes-name"),
                                    value: "{theme_name}",
                                    oninput: move |event| theme_name.set(event.value()),
                                    onkeydown: move |event| {
                                        caret.struck();
                                        appkit_has_this_key(&event);
                                    },
                                    onfocusin: move |_| caret.arrived(),
                                    onfocusout: move |_| caret.left(),
                                }
                                if theme_name().is_empty() {
                                    span { class: "ghost", "data-theme-prompt": "true",
                                        {fl!("themes-name-prompt")}
                                    }
                                }
                            }
                            button {
                                class: if can_save { "btn" } else { "btn off" },
                                "data-theme-save": "true",
                                aria_disabled: if can_save { "false" } else { "true" },
                                onclick: save_theme,
                                {glyph(Glyph::Bookmark, step_ink(can_save), "glyph")}
                                span { {fl!("themes-save")} }
                            }
                        }
                    }
                }
            }

            if appearance_sheet() {
                div { class: "scrim", onclick: move |_| appearance_sheet.set(false),
                    div {
                        class: "sheet appearance-sheet",
                        // The scrim closes on a click; the panel is not the
                        // scrim.
                        onclick: move |event| event.stop_propagation(),
                        div { class: "sheet-head",
                            h2 {
                                {glyph(Glyph::Appearance, Ink::Accent, "glyph-brand")}
                                span { {fl!("appearance")} }
                            }
                            button {
                                class: "sheet-close appearance-close",
                                // The word this button used to carry, kept
                                // where a screen reader still reads it: an
                                // unlabelled cross is a shape rather than a
                                // control to anything that cannot see it.
                                aria_label: fl!("close"),
                                // The keyboard comes into the sheet with the
                                // sheet: it is what a modal should do, and it
                                // is what lets the element half of the Escape
                                // handling see the key at all.
                                autofocus: true,
                                onclick: move |_| appearance_sheet.set(false),
                                {glyph_hover(Glyph::Close, Ink::Faint, Ink::Danger, "glyph")}
                            }
                        }
                        // The same segmented row the error-correction levels
                        // use: the same shape of question.
                        div { class: "segments segments-3",
                            for choice in Appearance::ALL {
                                button {
                                    key: "{choice.slug()}",
                                    class: chip_class(appearance() == choice, false),
                                    "data-appearance": "{choice.slug()}",
                                    aria_pressed: if appearance() == choice { "true" } else { "false" },
                                    onclick: {
                                        let remember = remember.clone();
                                        move |_| {
                                            appearance.set(choice);
                                            // Written here rather than in an
                                            // effect on `appearance`, so that
                                            // `--appearance` and the saved value
                                            // itself seed the window without
                                            // writing themselves back.
                                            if let Some(Remember(write)) = &remember {
                                                write(choice);
                                            }
                                        }
                                    },
                                    span {
                                        match choice {
                                            Appearance::System => fl!("appearance-system"),
                                            Appearance::Light => fl!("appearance-light"),
                                            Appearance::Dark => fl!("appearance-dark"),
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
                        class: "sheet about",
                        // The scrim closes on a click; the panel is not the
                        // scrim.
                        onclick: move |event| event.stop_propagation(),
                        div { class: "sheet-head",
                            h2 {
                                {glyph(Glyph::Code, Ink::Accent, "glyph-brand")}
                                span { {fl!("app-title")} }
                            }
                            button {
                                class: "sheet-close about-close",
                                aria_label: fl!("close"),
                                autofocus: true,
                                onclick: move |_| about.set(false),
                                {glyph_hover(Glyph::Close, Ink::Faint, Ink::Danger, "glyph")}
                            }
                        }
                        p { {fl!("app-description")} }
                        // Fluent takes an argument, so the number does not force
                        // this line to be a `format!` in English.
                        p { class: "version",
                            {fl!("version", number = env!("CARGO_PKG_VERSION"))}
                        }
                        // One button, so it takes the width rather than sitting
                        // in half of it: the panel has one thing to press and
                        // one way out, and they do not look alike.
                        div { class: "sheet-actions",
                            button {
                                class: "btn wide",
                                onclick: move |_| {
                                    let _ = open::that(env!("CARGO_PKG_REPOSITORY"));
                                },
                                {glyph(Glyph::External, Ink::Plain, "glyph")}
                                span { {fl!("repository")} }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One of the two colour tiles: the swatch, what it paints, and its hex.
///
/// A tile rather than a bare circle of colour beside a word, which is two
/// things to look at for one fact. It is also the click target that points the
/// picker below at this colour.
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
/// There is no `<input type="color">` in Blitz — it has an accessibility role
/// for one and no widget behind it — so every piece here is an ordinary
/// element, and the square is four stacked CSS background layers over a `div`.
///
/// **The markers are background layers rather than child elements**, which is
/// the one non-obvious decision here. A child on top of the square is what the
/// pointer hits, and Blitz measures element coordinates once against the hit
/// node — so `element_coordinates()` would come back relative to the marker.
/// A background layer has no hit box at all.
///
/// `pointer-events: none` would also answer it — `Node::hit_inner` does consult
/// it, and the theme sheet's name prompt is laid over a field that way. It is
/// the layers here that the square's own arithmetic and its tests are built
/// around, so this stays as it is; the note is here so the next person does not
/// re-derive a limit that is not there.
///
/// `onhold` says when a pointer has taken hold of the square or the strip. It
/// is the picker's own business except that the colour caution above holds
/// still in between; the argument is beside the banner.
#[component]
fn Picker(color: Signal<Rgb>, onhold: EventHandler<bool>, onswap: EventHandler<()>) -> Element {
    // The blink, from the context `App` provides. The hex field is a text
    // field like the other two and blinks like them; it is only in a different
    // component because the colour picker is.
    let mut caret = use_context::<Caret>();
    // The hex field keeps its own text, because half-typed hex is not a colour
    // and a field rewritten from `color` on every keystroke cannot be typed in.
    let mut draft = use_signal(|| color().to_hex());
    let mut valid = use_signal(|| true);

    // Hue, saturation and value are held here rather than derived from the
    // colour on every render, because the conversion back is lossy where a
    // picker is used: black and grey have no hue, so a square dragged into its
    // bottom edge would snap the strip to red and strand whoever was dragging.
    let mut hsv = use_signal(|| to_hsv(color()));
    let mut dragging = use_signal(|| false);
    // Taking hold and letting go are the same two words to the square, to the
    // strip, and to the caution a rail-width above them.
    let mut hold = move |holding: bool| {
        dragging.set(holding);
        onhold.call(holding);
    };

    // The last colour this picker wrote, so a colour arriving from anywhere
    // else can be told apart from one of its own — "Reset to black & white" is
    // the one place another comes from.
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
                    hold(true);
                    pick_in_square(event);
                },
                onpointermove: move |event| {
                    if dragging() {
                        pick_in_square(event);
                    }
                },
                onpointerup: move |_| hold(false),
                onpointerleave: move |_| hold(false),
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
                    hold(true);
                    pick_in_strip(event);
                },
                onpointermove: move |event| {
                    if dragging() {
                        pick_in_strip(event);
                    }
                },
                onpointerup: move |_| hold(false),
                onpointerleave: move |_| hold(false),
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
                    // The same rule the margin field follows: half-typed text
                    // may sit in a field while it is being typed, but it cannot
                    // outlive the keyboard. The code is still drawn in the last
                    // colour that parsed.
                    onblur: move |_| {
                        draft.set(color().to_hex());
                        valid.set(true);
                    },
                    onkeydown: move |event| {
                        caret.struck();
                        appkit_has_this_key(&event);
                    },
                    onfocusin: move |_| caret.arrived(),
                    onfocusout: move |_| caret.left(),
                }
                // Beside the hex rather than under it, in the room the row
                // already had. It belongs to the card rather than to this
                // picker — it moves both colours, and the picker holds one — so
                // it arrives as a handler.
                button {
                    class: "btn swap",
                    "data-swap": "true",
                    onclick: move |_| onswap.call(()),
                    {glyph(Glyph::Swap, Ink::Plain, "glyph")}
                    span { {fl!("color-swap")} }
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
const SQUARE_H: f64 = 158.0;
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
/// handed to `usvg` as a document of its own. A presentation attribute cannot
/// read a custom property and cannot be inside a media query, so each of these
/// is two colours rather than one: see [`glyph`], which draws both and lets
/// the stylesheet hide the one that does not belong to the appearance in force.
///
/// Three of the five are a palette token written a second time — the accent,
/// the caution and the danger, which have to match the chrome they sit in;
/// `an_icon_is_inked_the_colour_the_stylesheet_says` keeps the two files
/// agreeing. The two greys are not tokens: a 1.7-pixel stroke does not carry
/// the weight of text at the same colour, so they were matched by eye.
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
    /// A cross under the pointer, in the `--danger` the window writes a bad hex
    /// field in — the colour a close button turns everywhere else.
    Danger,
}

impl Ink {
    /// The five on a bright window.
    const fn light(self) -> &'static str {
        match self {
            Ink::Accent => "#0F9D63",
            Ink::Plain => "#2B313A",
            Ink::Faint => "#838B95",
            Ink::Warn => "#8A5B06",
            Ink::Danger => "#C0392B",
        }
    }

    /// The same five on a dark one.
    ///
    /// Not the light inks inverted: a line drawing has to stay a shade away
    /// from its surface in both directions, which is a different distance on
    /// paper than on graphite.
    const fn dark(self) -> &'static str {
        match self {
            Ink::Accent => "#4ECB8F",
            Ink::Plain => "#D2D8E0",
            Ink::Faint => "#8C949E",
            Ink::Warn => "#E9C07C",
            Ink::Danger => "#FF9585",
        }
    }
}

/// The icons, as the paths that draw them on a 24×24 grid.
///
/// Hand-drawn rather than pulled from an icon font, for the reason the whole
/// app exists: a font is a file to ship and a licence to honour. Stroked,
/// round-capped and unfilled, which is what keeps them looking like a set.
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
    /// A circle half hatched: the appearance, and the sheet that chooses it.
    Appearance,
    /// A square with something round in the middle of it: the inset, drawn as
    /// what it is, and deliberately not [`Frame`](Glyph::Frame) with the inner
    /// shape filled — the two sit in the same column and have to be told apart
    /// at a glance.
    Inset,
    /// Four modules in the three outlines the app can draw them in: the
    /// shape row, drawn as the thing it changes. No box around them, which is
    /// what keeps it apart from [`Frame`](Glyph::Frame) and
    /// [`Inset`](Glyph::Inset) two cards up the same rail.
    Shape,
    /// A triangle with a bang in it, for the one warning in the window.
    Alert,
    Minus,
    Plus,
    Close,
    External,
    /// A bookmark: the saved looks, and the sheet that keeps them. A shape
    /// that says *put away and come back to* without being a star, which
    /// would read as a rating.
    Bookmark,
    /// A bin, for the one control in the window that destroys something. Kept
    /// apart from [`Close`](Glyph::Close), which is eight points away in the
    /// same sheet and only closes it.
    Trash,
    /// Two arrows passing: the button that exchanges the code's two colours.
    /// Not a half-hatched circle, which is what "invert" usually gets drawn
    /// as, because [`Appearance`](Glyph::Appearance) is already that circle and the two
    /// are eight inches apart in the same window.
    Swap,
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
            // Two cells across and two down: a square, a rounded square and two
            // dots. Three outlines rather than four, because an empty fourth
            // cell reads as a mistake.
            Glyph::Shape => &[
                "M4 4 H11 V11 H4 Z",
                "M15.2 4 H17.8 A2.2 2.2 0 0 1 20 6.2 V8.8 A2.2 2.2 0 0 1 17.8 11 \
                 H15.2 A2.2 2.2 0 0 1 13 8.8 V6.2 A2.2 2.2 0 0 1 15.2 4 Z",
                "M11 16.5 A3.5 3.5 0 1 1 4 16.5 A3.5 3.5 0 1 1 11 16.5",
                "M20 16.5 A3.5 3.5 0 1 1 13 16.5 A3.5 3.5 0 1 1 20 16.5",
            ],
            Glyph::Alert => &["M12 3.9 L21.2 19.9 H2.8 Z", "M12 10 V14.3", "M12 17 h0.4"],
            Glyph::Minus => &["M6.2 12 H17.8"],
            Glyph::Plus => &["M6.2 12 H17.8", "M12 6.2 V17.8"],
            Glyph::Close => &["M6.4 6.4 L17.6 17.6", "M17.6 6.4 L6.4 17.6"],
            // A bin: lid, handle, body, and two lines down the inside of it.
            // **Not the cross**, which in this window closes a panel — the two
            // sit a few points apart in the same sheet, and one of them cannot
            // be undone.
            Glyph::Trash => &[
                "M4.5 6.8 H19.5",
                "M9.6 6.8 V4.9 H14.4 V6.8",
                "M6.6 6.8 L7.4 19.8 H16.6 L17.4 6.8",
                "M10.4 10.2 V16.4",
                "M13.6 10.2 V16.4",
            ],
            // Contrast: a circle split down the middle, one half hatched. Not
            // a sun and not a moon, because those two are the *answers* and
            // this is the button that asks the question.
            Glyph::Appearance => &[
                "M21 12 A9 9 0 1 1 3 12 A9 9 0 1 1 21 12",
                "M12 3 V21",
                "M12 6.2 h3.1",
                "M12 9.1 h5.2",
                "M12 12 h6",
                "M12 14.9 h5.2",
                "M12 17.8 h3.1",
            ],
            Glyph::Bookmark => &["M6.4 3.8 H17.6 V20.2 L12 16.1 L6.4 20.2 Z"],
            Glyph::External => &[
                "M14.2 4.6 H19.4 V9.8",
                "M19.4 4.6 L11.2 12.8",
                "M17 13.8 V19.4 H4.6 V7 H10.2",
            ],
            // One arrow each way, on the same two ends: what the button does,
            // which is exchange rather than negate.
            Glyph::Swap => &[
                "M4.6 8 H19.4",
                "M16.2 4.8 L19.4 8 L16.2 11.2",
                "M19.4 16 H4.6",
                "M7.8 12.8 L4.6 16 L7.8 19.2",
            ],
        }
    }
}

/// The ink a stepper button's sign is drawn in, given whether it can move the
/// margin at all.
///
/// The two-line function exists because an icon's colour is a presentation
/// attribute rather than a style — `ui.css` cannot reach inside the document
/// `glyph` builds — so a dimmed button has to be told to draw a dimmer sign,
/// where every other control in the window would simply be handed a class.
const fn step_ink(live: bool) -> Ink {
    if live { Ink::Plain } else { Ink::Faint }
}

/// How a chip is drawn: selected, held, or neither.
///
/// **Every chip's label goes in a `<span>`.** Bare text inside a `<button>`
/// keeps the colour it was first painted in when the appearance changes under it —
/// the surface repaints and the word does not, leaving dark text on a dark
/// chip until something rebuilds the node. Wrapping the text makes it a node
/// with a style of its own. The headless harness resolves this correctly, so
/// no test would catch it coming back.
const fn chip_class(selected: bool, locked: bool) -> &'static str {
    match (selected, locked) {
        (true, false) => "chip on",
        (true, true) => "chip on off",
        (false, false) => "chip",
        (false, true) => "chip off",
    }
}

/// One icon, as a pair of inline `<svg>`s sized by `class`.
///
/// Blitz serializes the element back to markup and parses it with `usvg`, the
/// same route the preview takes — so an icon here is a real document, not a
/// glyph in a font and not a rasterized image. That is also what makes it a
/// pair: a document of its own is a document CSS cannot reach into, so the ink
/// cannot follow the appearance and the icon has to. One is drawn in each palette's
/// ink and `ui.css` hides the wrong one.
///
/// It is the price of a runtime appearance switch, paid on every icon whether
/// anybody uses it or not. Worth knowing before spending the same trick on
/// anything that appears in a list.
fn glyph(kind: Glyph, ink: Ink, class: &'static str) -> Element {
    rsx! {
        {drawn(kind, ink.light(), format!("{class} lit"))}
        {drawn(kind, ink.dark(), format!("{class} dim"))}
    }
}

/// One icon that answers a hover, as the same mark in two inks.
///
/// It costs what [`glyph`] costs, doubled: four small usvg documents for one
/// mark on screen. That is why it is a second function rather than an
/// argument to the first — it is worth spending on the two crosses that close
/// a sheet and on nothing else in the window.
///
/// The swap is a `display` on the two wrappers, so the appearance half of the
/// choosing is not written twice: `.lit` / `.dim` still picks inside each pair.
/// Both wrappers are flex boxes, which keeps the mark centred — an `<svg>` is
/// inline by default, and a line box under a 34-point button would sit it low.
fn glyph_hover(kind: Glyph, rest: Ink, hot: Ink, class: &'static str) -> Element {
    rsx! {
        span { class: "ink-rest", {glyph(kind, rest, class)} }
        span { class: "ink-hot", {glyph(kind, hot, class)} }
    }
}

/// One icon in one colour.
fn drawn(kind: Glyph, stroke: &'static str, class: String) -> Element {
    rsx! {
        svg {
            class: "{class}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "{stroke}",
            stroke_width: "1.7",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for (index , outline) in kind.paths().iter().enumerate() {
                path { key: "{index}", d: "{outline}" }
            }
        }
    }
}

/// The dashed outline's colour on a mat painted `mat`.
///
/// The outline tells the code's own background apart from the window behind
/// it, and the mat is whatever colour somebody picked — so a fixed grey answers
/// the case the app opens in and disappears the moment anybody clicks the
/// middle of the greyscale row. The line is derived from the mat instead:
/// pushed towards black if the mat is light, towards white if it is dark, which
/// keeps it on the mat's own hue and independent of the appearance.
///
/// The two fractions differ because the eye does. A third of the way to black
/// off white reads without looking; the same third towards white off black is
/// barely there, so the dark side gets more.
fn mat_line(mat: Rgb) -> String {
    /// Luminance above which a mat counts as light, on 0…1.
    const LIGHT_ABOVE: f32 = 0.5;
    /// How far a light mat's line is pushed towards black.
    const TOWARDS_BLACK: f32 = 0.32;
    /// And a dark one's towards white.
    const TOWARDS_WHITE: f32 = 0.44;

    let (target, amount) = if luminance(mat) > LIGHT_ABOVE {
        (0.0, TOWARDS_BLACK)
    } else {
        (255.0, TOWARDS_WHITE)
    };
    let mix = |channel: u8| (f32::from(channel) + (target - f32::from(channel)) * amount) as u8;

    Rgb::new(mix(mat.r), mix(mat.g), mix(mat.b)).to_hex()
}

/// How light a colour is, on 0…1.
///
/// The 299/587/114 weighting, which is what `read` in `qrnew-core` flattens a
/// photograph to before handing it to `rqrr`. Deliberately the same one.
fn luminance(colour: Rgb) -> f32 {
    (299.0 * f32::from(colour.r) + 587.0 * f32::from(colour.g) + 114.0 * f32::from(colour.b))
        / 255_000.0
}

/// How far apart the code's two colours are, on 0…1.
///
/// Luminance and not hue: a scanner flattens the picture to grey and looks for
/// the step. Two colours nothing alike to look at can be the same colour to a
/// camera — the palette's leaf green on its dark red is a gap of 0.038 — which
/// is the case a window full of swatches has to say something about.
/// [`SAFE_CONTRAST`] is where it starts saying it.
fn contrast(dark: Rgb, light: Rgb) -> f32 {
    (luminance(dark) - luminance(light)).abs()
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
/// A dark outline, a white ring inside it and the picked colour in the middle —
/// three filled squares, largest at the bottom, so it reads on a white corner
/// and a black one alike.
///
/// **Not a `radial-gradient`, which drew it before.** Blitz resolves a radial
/// gradient's centre in CSS pixels and then adds it to a rectangle already
/// measured in device pixels, so on a 2× display the ring landed at half the
/// offset it was given. `background-position` and `background-size` are both
/// multiplied by the scale before use, and `linear-gradient(c, c)` is a flat
/// fill, so a marker built out of those is drawn where it was put at any scale.
///
/// The centre is held half a marker inside the box. A background layer is
/// clipped to its element — unlike the child element a browser would use — so
/// an unclamped marker on a fully black or fully saturated colour would be a
/// sliver against the edge, which is exactly when somebody is looking for it.
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
///
/// **Bytes rather than a string slice, and that is a crash rather than a
/// preference.** This reads the hex field on every keystroke, the field takes
/// any text somebody can type, and the length it switched on was `str::len` —
/// bytes — while the slices it took were expected to be characters. Three bytes
/// is `abc` and also `aé`, where `&text[1..2]` is not a character boundary: the
/// field panics, and a panic in a Dioxus event handler takes the window with
/// it. Every path below indexes a byte and asks whether it is a hex digit.
pub(crate) fn parse_hex(text: &str) -> Option<Rgb> {
    let text = text.trim().trim_start_matches('#').as_bytes();
    // A byte at or above 0x80 becomes a Latin-1 character here, and none of
    // those is a hex digit — so a multi-byte character is refused one byte at
    // a time rather than sliced through.
    let digit = |at: usize| char::from(text[at]).to_digit(16).map(|value| value as u8);
    let pair = |at: usize| Some(digit(at)? * 16 + digit(at + 1)?);

    match text.len() {
        3 => Some(Rgb::new(digit(0)? * 17, digit(1)? * 17, digit(2)? * 17)),
        6 => Some(Rgb::new(pair(0)?, pair(2)?, pair(4)?)),
        _ => None,
    }
}

/// Bytes, as something an `<img>` can point at.
///
/// Base64 rather than percent-encoding because a QR document is mostly path
/// data and the characters needing escapes are common in it: base64 costs a
/// third more, escaping everything costs three times more, and this is rebuilt
/// on every keystroke.
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

/// A beat every `period`, for as long as the window is open.
///
/// [`after`] is a countdown and this is a clock, and the difference is a
/// thread: `after` starts one per wait and lets it die, which is right for a
/// confirmation claimed a few times an hour and wrong for a caret claimed twice
/// a second. The thread here sleeps between beats for the life of the process
/// and is never joined.
///
/// It beats whether or not anybody is waiting, and a beat nobody took is
/// dropped on the next one — `done` is a flag rather than a count — so a task
/// that falls behind resumes on the current beat instead of catching up through
/// a backlog of stale ones.
fn metronome(period: Duration) -> Metronome {
    let beat = Arc::new(Mutex::new(Countdown::default()));
    let keeping = Arc::clone(&beat);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(period);
            // Woken outside the lock, for the reason `after` gives: the waker
            // runs the app's own code on the way through.
            let waker = {
                let mut countdown = keeping.lock().unwrap();
                countdown.done = true;
                countdown.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    });
    Metronome { beat }
}

struct Metronome {
    beat: Arc<Mutex<Countdown>>,
}

impl Metronome {
    /// The next beat.
    fn tick(&self) -> Tick<'_> {
        Tick(&self.beat)
    }
}

struct Tick<'a>(&'a Arc<Mutex<Countdown>>);

impl Future for Tick<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut countdown = self.0.lock().unwrap();
        if countdown.done {
            countdown.done = false;
            Poll::Ready(())
        } else {
            countdown.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// A future that completes once `delay` has passed.
///
/// **There is no timer in the dependency list to borrow one from**, and that is
/// the point of the dependency list: `tokio` is in the tree, pulled in by
/// something else, but only as `rt` — no time driver and nothing running one.
/// The alternative to twenty lines here is a crate whose whole job is to spawn
/// the thread below.
///
/// The thread starts on the first poll rather than on construction, so a
/// countdown dropped before anyone waits on it costs nothing. The waker is
/// stored on every poll, not only the first: a task polled from somewhere else
/// afterwards has a new waker, and the old one would wake nobody.
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
                // the way through, and it has no business doing that while this
                // thread holds something it will not be back to release.
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

    /// **A saved code is big enough to use whatever is in it.** The shortest
    /// input makes the smallest code, so a fixed number of pixels per module
    /// gave the commonest case the worst file.
    #[test]
    fn a_saved_code_is_never_smaller_than_the_minimum() {
        let style = QrStyle::default();
        for text in ["hi", "https://example.org", &"x".repeat(1000)] {
            let qr = Qr::new(text, ErrorCorrection::Medium, &style).expect("a code");
            let across = qr.size_in_modules() * export_scale(&qr);
            assert!(
                across >= MIN_EXPORT_PX,
                "{} modules came out {across} pixels across",
                qr.size_in_modules()
            );
        }

        // Past the point where the floor is what binds, the scale is the one
        // that always was: a code that big is already wide enough.
        let long = Qr::new(&"x".repeat(2000), ErrorCorrection::Low, &style).expect("a code");
        assert_eq!(export_scale(&long), EXPORT_SCALE);
    }

    /// **A picture in the middle is not exported below its own resolution.**
    /// The inset is a fraction of the width, so the size that is generous for
    /// the code starves the one thing in it the app did not draw.
    #[test]
    fn a_saved_inset_gets_pixels_of_its_own() {
        let picture = Qr::new(
            "hi",
            ErrorCorrection::Medium,
            &QrStyle {
                quiet_zone: DEFAULT_MARGIN,
                logo: Some(Logo::new(
                    br#"<svg xmlns="http://www.w3.org/2000/svg" width="447" height="447"><rect width="447" height="447"/></svg>"#
                        .to_vec(),
                )),
                ..QrStyle::default()
            },
        )
        .expect("a code with a picture in it");

        let across = picture.size_in_modules() * export_scale(&picture);
        let box_px = across as f32 * picture.inset_box().expect("a picture").side;
        // Not the whole 512: [`MAX_EXPORT_PX`] is what stops it, and that is
        // the trade being made rather than a rounding. The picture that
        // prompted this is 447 pixels and comes out at 420.
        assert!(
            box_px >= qrnew_core::MAX_LOGO_SIDE as f32 * 0.8,
            "a {} pixel picture was given {box_px} pixels",
            qrnew_core::MAX_LOGO_SIDE
        );
        assert!(across <= MAX_EXPORT_PX, "{across} pixels is past the ceiling");
    }

    /// **An icon's ink and the stylesheet's have to be the same colour.**
    ///
    /// They are written twice — once as a presentation attribute in [`Ink`],
    /// because CSS cannot reach inside an SVG in Blitz, and once as a custom
    /// property in `ui.css` — with no way to write them once. So the two files
    /// are compared: the light palette comes first in the stylesheet and the
    /// dark one second, which is the order [`Ink`] answers in.
    #[test]
    fn an_icon_is_inked_the_colour_the_stylesheet_says() {
        /// Every value `token` is given in `ui.css`, in order and without
        /// repeats.
        ///
        /// The repeats are the dark palette's, written twice — once for the
        /// class and once for the media query.
        /// `the_dark_palette_says_the_same_thing_twice` is what holds those.
        fn palette(token: &str) -> Vec<String> {
            let name = format!("{token}:");
            let mut seen: Vec<String> = Vec::new();
            for value in include_str!("ui.css")
                .lines()
                .filter_map(|line| line.trim().strip_prefix(&name))
                .map(|value| value.trim().trim_end_matches(';').to_ascii_lowercase())
            {
                if !seen.contains(&value) {
                    seen.push(value);
                }
            }
            seen
        }

        // The two greys are left out on purpose: they are not tokens, and
        // why they are not is on the type. `--danger` is here because the
        // cross under the pointer has to be the red the rest of the window
        // writes an error in, and a hover is not a state a screenshot catches.
        for (ink, token) in [
            (Ink::Accent, "--accent"),
            (Ink::Warn, "--warn"),
            (Ink::Danger, "--danger"),
        ] {
            assert_eq!(
                palette(token),
                vec![
                    ink.light().to_ascii_lowercase(),
                    ink.dark().to_ascii_lowercase(),
                ],
                "{token} in ui.css against Ink::{ink:?}",
            );
        }
    }

    /// **The dark palette is written twice, so the two have to agree.**
    ///
    /// It applies when somebody picked dark and when somebody left the choice to
    /// a dark desktop — a selector and a media query, which CSS gives no way to
    /// share a block between. A colour edited in one copy and not the other
    /// would be a appearance that looked different depending on how it was reached.
    #[test]
    fn the_dark_palette_says_the_same_thing_twice() {
        /// Every `--token: value` between `selector` and the `}` that ends it.
        fn block(selector: &str) -> Vec<(String, String)> {
            let after = include_str!("ui.css")
                .split_once(selector)
                .unwrap_or_else(|| panic!("no {selector} in ui.css"))
                .1;
            after
                .split_once('}')
                .expect("the block is closed")
                .0
                .lines()
                .filter_map(|line| line.trim().strip_prefix("--"))
                .filter_map(|declaration| declaration.split_once(':'))
                .map(|(name, value)| {
                    (
                        name.trim().to_string(),
                        value.trim().trim_end_matches(';').to_string(),
                    )
                })
                .collect()
        }

        let chosen = block(".appearance-dark {");
        assert!(chosen.len() > 20, "the palette is most of the window");
        assert_eq!(chosen, block(".appearance-system {"), "the two copies have drifted");
    }

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

    /// **The hex field takes anything a keyboard can produce, so this must
    /// too.**
    ///
    /// Each of these is three or six *bytes* and fewer in characters, which is
    /// the shape that used to slice through the middle of one and panic. `aé`
    /// is the whole bug in two characters.
    #[test]
    fn hex_refuses_text_that_is_not_ascii_instead_of_panicking() {
        for text in ["aé", "éa", "ééé", "ff€", "€ff", "«»f", "ﬀ", "２ｆ６"] {
            assert_eq!(parse_hex(text), None, "{text:?}");
        }
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
    /// Tested here rather than through the interface because raising a
    /// confirmation means putting something on the clipboard first, and CI has
    /// no display to have one on. So: it does not finish early, it does finish,
    /// and it wakes whoever was waiting rather than relying on another poll.
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

    /// **The clock behind the caret, which has to beat more than once.**
    ///
    /// That is the whole difference between it and the countdown above: a
    /// metronome that fired once would leave a caret that never came back. The
    /// beat also has to survive being read — a `Tick` returning `Ready` twice
    /// for one beat would blink at whatever rate the runtime polls at.
    #[test]
    fn a_metronome_beats_again_and_again() {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        let period = Duration::from_millis(60);
        let clock = metronome(period);

        assert_eq!(
            std::pin::pin!(clock.tick()).poll(&mut cx),
            Poll::Pending,
            "the first beat has not happened yet"
        );

        for beat in 1..=3 {
            // Slack in one direction only, as above: a sleeping thread may
            // oversleep, and what is asserted is that it did not undersleep.
            std::thread::sleep(period * 3);
            assert_eq!(
                std::pin::pin!(clock.tick()).poll(&mut cx),
                Poll::Ready(()),
                "beat {beat} arrived"
            );
            assert_eq!(
                std::pin::pin!(clock.tick()).poll(&mut cx),
                Poll::Pending,
                "and was taken, rather than staying on the counter for beat {beat} to be read again"
            );
        }
    }
}
