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
//! Both rails are now full: Content, Error correction, Margin and Shape down
//! the first, Colors and Inset down the second. Height is the scarce thing
//! here and the picker is what holds most of it, which is why adding the Inset
//! card took thirty pixels off the saturation square and the row of inset
//! sizes took twelve more, and why the Shape card was paid for out of every
//! card's padding and every gap between them — the arithmetic is in `ui.css`,
//! and `no_control_is_below_the_fold` checks it at the size a maximized window
//! actually gets on a laptop screen rather than at the size the window falls
//! back to.
//!
//! Two of the cards say something a decision costs, in the same banner: a
//! margin under two may be hard for a scanner to find, and a code that is not
//! drawn in squares takes a camera longer to lock onto. Neither is allowed to
//! be the thing a rail scrolls away, which is a promise about the shortest
//! window rather than about the order of the cards — see the Shape card, which
//! is last and whose caution is therefore the lower of the two.
//!
//! The three export buttons are drawn from the first frame rather than
//! appearing with the first character, dimmed until there is something to
//! export, so the stage never rearranges itself while it is being looked at.
//!
//! # Light, dark, and following the desktop
//!
//! Three answers and the person at the window picks, from a sheet behind the
//! button beside About. Following the desktop is the default, because it is
//! the answer that is right without anybody being asked — but it is only a
//! default. Somebody comparing a code against the paper it will be printed on
//! wants a light window at ten at night, and the desktop's setting has no
//! opinion about that worth overriding theirs.
//!
//! **The choice is a class on `.app`, not a media query**, and that is forced
//! rather than chosen. `prefers-color-scheme` is real here — Blitz hands Stylo
//! winit's window theme — but nothing an app can call from inside a component
//! moves it. The one lever, `View::set_theme_override`, belongs to the shell
//! and is not reachable from a Dioxus component; and asking winit to change
//! the window's own theme does not stand in for it, because macOS deliberately
//! *suppresses* the `ThemeChanged` event when the appearance was set by the
//! program rather than by the desktop. So [`Theme`] writes `theme-system`,
//! `theme-light` or `theme-dark` onto the root element, `ui.css` hangs both
//! palettes off that, and `prefers-color-scheme` is consulted only inside the
//! `theme-system` branch — which is the one case where the desktop really is
//! the authority and the event really does arrive.
//!
//! The window is still asked, mind: [`App`] calls `set_theme` on winit so the
//! title bar matches the window under it. That is cosmetic and best-effort —
//! a compositor may decline — and nothing in the interface depends on it.
//!
//! An icon is the one thing this cannot reach. Its ink is a presentation
//! attribute on a document CSS cannot see inside, so [`glyph`] draws every
//! icon twice, once in each palette's ink, and the stylesheet hides the one
//! that does not apply. A node and a small usvg document per icon is what a
//! runtime theme switch costs here, and it is why the choice is spent on the
//! window rather than on anything smaller.

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
/// text belongs to a `parley` editor that is handed a size, a line height and
/// a colour, and no font at all — so the centring is arithmetic the app does
/// with `padding-left` and the field's own `ch`. The long version of the story
/// is above `.count` in `ui.css`, and
/// `the_margin_number_is_centered_in_its_field` is what stops this number and
/// that width drifting apart.
///
/// It is arithmetic across two different answers to how wide a digit is, and
/// `blitz-fonts.md` has both measured: Stylo resolves `1ch` here to 10.000
/// points and the shaper paints 9.455. Half a point of the difference lands on
/// screen, which is under the tolerance the test holds and over nothing. It is
/// worth knowing that the day the editor is handed its font, the second number
/// moves and this one has to be measured again.
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

        // And then, once, whatever scaling the picture needs to be one the app
        // can carry. A photograph is several thousand pixels across, which is
        // detail no export can use, an image re-decoded on every redraw — and,
        // past four thousand and change, more than a GPU texture atlas will
        // take at all. `vello_hybrid` does not draw such an image smaller, it
        // unwraps the refusal, and **the window closes**: this is the line
        // between choosing a photograph and losing the app. It happens here,
        // at the one moment a picture arrives, rather than in the memo that
        // redraws the code on every keystroke.
        let (format, bytes) = match qrnew_core::shrink_logo(&bytes) {
            Some(scaled) => (ImageFormat::Png, scaled),
            None => (format, bytes),
        };

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

/// The theme to open in, provided as a root context by `main.rs`.
///
/// It exists for `--theme`, which exists because the choice is behind a button
/// and a sheet: there is no way to photograph a dark window from the outside
/// otherwise, and "here is what it looks like" is a question about this app
/// that gets asked. Nothing provides it in a test, and an absent context is
/// [`Theme::System`] — which is also what somebody who never opens the sheet
/// gets.
#[derive(Clone)]
pub struct Tone(pub Theme);

/// Somewhere to write the theme down, provided as a root context by `main.rs`.
///
/// The app does not do its own file writing, and this is why: a test clicks
/// through the theme sheet several times, and a component that saved to disk
/// would be a test suite that edited the settings of whoever ran it. `main.rs`
/// supplies the closure, nothing supplies it in a test, and an absent context
/// is an app that simply does not remember — which is also what a machine with
/// no writable home gets.
#[derive(Clone)]
pub struct Remember(pub Arc<dyn Fn(Theme) + Send + Sync>);

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

/// How the code's own marks are drawn.
///
/// **One choice for two of `qrnew-core`'s knobs, deliberately.** The core
/// holds a [`ModuleShape`] and a [`FinderShape`] and they are independent —
/// `every_combination_of_shapes_scans` there walks all six pairings — but six
/// is not a question worth putting to somebody making one code. Rounded
/// modules inside square finders is the pairing nobody picks on purpose: it
/// reads as a style that was applied to most of the code and missed the
/// corners. So the app offers the three that are a *look*, and the finders
/// follow the modules rather than being asked about separately.
///
/// Nothing here can make a code that does not *scan*, and that is not a hope:
/// a scanner reads the colour at the centre of a module, every shape in the
/// core covers its own centre, and `every_combination_of_shapes_scans` and
/// `every_combination_of_shapes_scans_with_a_logo_in_the_way` decode all of
/// them with a real reader.
///
/// Scanning is not the same as scanning *quickly*, though, and the card says
/// so as soon as anything but [`Square`] is chosen. A phone pointed at a
/// rounded or dotted code takes visibly longer to lock onto it: the decoder
/// gets fewer clean edges to work from, so autofocus has more to do before the
/// first frame it can read. That is a cost paid at the camera, where the
/// decoding tests cannot see it, which is exactly why it has to be written
/// down rather than left to the test suite.
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
    /// A `data-look` for the tests to select on, for the same reason
    /// [`Theme::slug`] is one: a test that clicked the visible label would
    /// pass in English and nowhere else.
    const fn slug(self) -> &'static str {
        match self {
            Look::Square => "square",
            Look::Rounded => "rounded",
            Look::Dots => "dots",
        }
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
    /// Colours are left at the core's default, which is the code's own dark:
    /// a finder in a second colour is a fourth colour control in a window
    /// whose colour rail is already the tallest thing in it, and it is the one
    /// part of the code that has to stay findable.
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

/// How much of the code the picture in the middle of it takes up.
///
/// Three named sizes rather than a number, because the useful range is narrow
/// and its top end is not a constant. A logo has to stay clear of the three
/// finder patterns, which sit eight modules in from each edge whatever the
/// code's version — so on the smallest code there is, twenty-one modules
/// across, the largest picture that fits is a shade over a fifth of the width,
/// while on anything longer than a few characters it is a third. A percentage
/// field would spend most of its range on values that only work for some of
/// what somebody might type.
///
/// [`Medium`] is [`Logo::DEFAULT_SIZE`] and always fits: that is what
/// `the_default_logo_fits_even_the_smallest_code` in `qrnew-core` checks.
/// [`Large`] does not always fit, and the app draws it at the middle size when
/// it does not — see `Drawn::capped`.
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

    /// The name this size goes by in the markup, for the tests to select on.
    const fn slug(self) -> &'static str {
        match self {
            InsetSize::Small => "small",
            InsetSize::Medium => "medium",
            InsetSize::Large => "large",
        }
    }

    /// Side of the picture as a fraction of the code's width, quiet zone not
    /// counted — which is exactly what [`Logo::size`] means.
    ///
    /// An eighth and a quarter around the core's own sixth. They are spaced so
    /// that each step is a visible change in the preview rather than a nudge:
    /// a quarter is twice the *area* of an eighth.
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
/// The second field is here because one of the app's controls can ask for
/// something the code in front of it cannot give: [`InsetSize::Large`] does
/// not fit a twenty-one-module code, and a twenty-one-module code is what a
/// few characters plus an inset produces. `qrnew-core` refuses that outright —
/// deliberately, since only the caller knows whether to give up the size or
/// the picture — and this app is the caller, and it gives up the size. Drawing
/// nothing would be the one answer that is certainly wrong: the text is fine,
/// the picture is fine, and the placeholder would say neither.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Drawn {
    qr: Qr,
    /// Whether the picture had to be drawn at [`InsetSize::Medium`] because
    /// the size that was asked for did not fit.
    capped: bool,
}

/// Which palette the window is painted in, and who decides.
///
/// [`Theme::System`] is the default and the only one of the three that is an
/// answer *about* the question rather than to it: it hands the decision back
/// to the desktop and follows it live. The other two are somebody overruling
/// that, which is a thing worth being able to do — a code is judged against
/// the surface around it, and the surface that matters is usually the paper it
/// is going to be printed on rather than whatever the desktop is set to after
/// dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Whatever the desktop says, changing when the desktop changes.
    System,
    Light,
    Dark,
}

impl Theme {
    /// The three, in the order the sheet offers them.
    const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The name this theme goes by in the markup.
    ///
    /// It is both half of the class on `.app` — `theme-{slug}`, which every
    /// themed rule in `ui.css` hangs off — and the `data-theme` a test selects
    /// its button by. One name for both, because a test that clicked on the
    /// visible label would be a test that passed in English and nowhere else.
    pub const fn slug(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// The theme `name` names, for `--theme` on the command line.
    ///
    /// The same three words the sheet's buttons carry as `data-theme`, since
    /// there is no sense in the flag and the markup disagreeing about what a
    /// theme is called.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.slug() == name)
    }

    /// What winit is asked to make the title bar.
    ///
    /// `None` is not "no opinion" so much as "stop holding one": it clears any
    /// appearance the app has set, which is what puts the window back under
    /// the desktop's control — and, on macOS, what starts the `ThemeChanged`
    /// events flowing again so that `prefers-color-scheme` is live for the
    /// `theme-system` branch of the stylesheet.
    const fn window(self) -> Option<WinitTheme> {
        match self {
            Theme::System => None,
            Theme::Light => Some(WinitTheme::Light),
            Theme::Dark => Some(WinitTheme::Dark),
        }
    }
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
    let mut look = use_signal(Look::default);
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
    let mut inset_size = use_signal(InsetSize::default);
    let mut editing = use_signal(|| Well::Dark);
    let mut about = use_signal(|| false);
    let mut theme = use_signal(|| {
        dioxus_core::try_consume_context::<Tone>().map_or(Theme::System, |Tone(seed)| seed)
    });
    let mut theme_sheet = use_signal(|| false);
    let remember = use_hook(dioxus_core::try_consume_context::<Remember>);

    // The title bar belongs to the platform, and the platform will not read a
    // class off `.app` — so the one thing the stylesheet cannot reach is asked
    // for here. It is cosmetic: a compositor that declines leaves a title bar
    // that does not match, and the window under it is right either way.
    //
    // The window arrives as a context, and there is not one in a test: the
    // harness builds the document with no window at all. `try_consume_context`
    // is what lets the same component run in both, the way `Fill` and `Inlay`
    // already do.
    let window = use_hook(dioxus_core::try_consume_context::<Arc<dyn WinitWindow>>);
    let windowed = window.is_some();
    use_effect(move || {
        if let Some(window) = &window {
            window.set_theme(theme().window());
        }
    });

    // **Escape closes whichever sheet is open.** A modal is the one place in
    // this window where the next click has to land somewhere in particular,
    // and the key that means "not this" is the one everybody reaches for
    // first: the scrim and the Close button were the only two ways out, and a
    // scrim is a thing you have to guess is clickable.
    //
    // It is answered twice, because the two answers cover different halves of
    // the same question — *where the keyboard is when the key is pressed*.
    //
    //   * `onkeydown` on `.app`, below. Blitz sends a key to the focused node
    //     and lets it bubble, so this catches every keystroke made while the
    //     keyboard is anywhere inside the interface. It is also the half the
    //     headless tests can drive, which is why the sheets take the keyboard
    //     when they open — see the `autofocus` on their Close buttons.
    //
    //   * This one, on the window itself. `clicking_a_chip_blurs_the_field`
    //     records the upstream rule that makes it necessary: a click that
    //     matches none of Blitz's known controls *clears* the focus, and a
    //     plain `<button>` matches none of them — so after clicking a theme in
    //     the sheet the keyboard is on `<html>`, which is above `.app` and
    //     bubbles away from it rather than through it. A winit key event is
    //     delivered before any of that applies.
    //
    // Setting a signal that is already false is nothing, so the overlap on the
    // keystrokes both of them see costs a comparison.
    //
    // The hook is gated on there being a window at all, and the gate is a
    // constant for the life of this component — `window` comes from a
    // `use_hook` — so the hook order does not change under it. Upstream's
    // `use_window_event` consumes the window context rather than trying for
    // it, and there is no window in a test.
    if windowed {
        dioxus_native::use_window_event(move |event, _| {
            if let WindowEvent::KeyboardInput { event, .. } = event
                && event.state == ElementState::Pressed
                && !event.repeat
                && event.logical_key == WinitKey::Named(NamedKey::Escape)
            {
                theme_sheet.set(false);
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
            // worth a control: it is the one somebody looks at the preview and
            // wants a different answer to.
            logo: picture.clone().map(|bytes| Logo {
                size: asked,
                ..Logo::new(bytes)
            }),
            // No `..QrStyle::default()`: every field of it is written here,
            // and the shape row is what filled the last two. A struct update
            // that updates nothing is a line that would go on looking like the
            // app is leaving something to the core.
        };
        match Qr::new(&text, ec(), &style) {
            Ok(qr) => Some(Drawn { qr, capped: false }),
            // The picture does not fit *this* code — which is a statement
            // about how short the text is, not about the picture. Redrawn at
            // the size that fits every code there is, and the row says so.
            //
            // Guarded on the size actually being the larger one, so that a
            // logo failure that is not about size cannot send the app round
            // the same encode twice for the same answer.
            Err(QrError::Logo(_)) if asked > Logo::DEFAULT_SIZE => {
                style.logo = picture.map(Logo::new);
                Qr::new(&text, ec(), &style)
                    .ok()
                    .map(|qr| Drawn { qr, capped: true })
            }
            // An input past what the densest code can hold. The libcosmic
            // build showed the placeholder for that and said nothing; the line
            // under the field says it now.
            Err(_) => None,
        }
    });

    // Kept apart from `code` so that a re-render that changes neither the text
    // nor the colours does not re-encode the SVG into base64.
    let preview = use_memo(move || {
        code()
            .as_ref()
            .map(|drawn| data_url("image/svg+xml", drawn.qr.svg().as_bytes()))
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
            if let Ok(png) = qr.to_png(EXPORT_SCALE) {
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
    // The largest inset the code on screen can carry, as the fraction of its
    // width that [`InsetSize::fraction`] is measured in — and `None` while
    // there is no code to measure.
    //
    // The number moves with the text, because the rule behind it is a number
    // of *modules* and the module count is what the text decides. So the row
    // has to ask the code in front of it rather than knowing the answer: the
    // sizes it can offer at all are the sizes this code can take.
    //
    // `size_in_modules` counts the quiet zone, which the fraction does not.
    let room = code.read().as_ref().map(|drawn| {
        let modules = drawn.qr.size_in_modules() - 2 * margin();
        qrnew_core::largest_logo_size(Logo::DEFAULT_PADDING, modules)
    });
    // Whether the size the row is pointing at is the size the code was drawn
    // at. Read out of the memo rather than recomputed: the encode that already
    // happened is the only thing that actually knows, and `room` above is the
    // app's own arithmetic about the same rule. The two agree, and where a
    // rounding at the boundary makes them disagree the memo is the one that is
    // right — which is why the chip is dimmed by either of them.
    let capped = code.read().as_ref().is_some_and(|drawn| drawn.capped);
    // Whether a size in the row is one the code in front of it cannot take: it
    // does not fit, or it is the one that was asked for and did not fit. The
    // second half is what covers a code that *shrank* — text deleted out from
    // under a size that was fine when it was chosen.
    let held = move |choice: InsetSize| {
        room.is_some_and(|room| choice.fraction() > room) || (capped && inset_size() == choice)
    };

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

        // The theme is a class here rather than a media query in `ui.css`,
        // and both sheets are inside it rather than beside it: a custom
        // property is inherited, so anything painted in the app's colours has
        // to be a descendant of the element the palette is written on.
        div {
            class: "app theme-{theme().slug()}",
            // Escape, for every keystroke made while the keyboard is inside
            // the interface. The other half is on the window; the whole story
            // is above `use_window_event` in this component.
            onkeydown: move |event| {
                if event.key() == Key::Escape && (theme_sheet() || about()) {
                    theme_sheet.set(false);
                    about.set(false);
                }
            },

            header { class: "topbar",
                div { class: "brand",
                    {glyph(Glyph::Code, Ink::Accent, "glyph-brand")}
                    span { {fl!("app-title")} }
                }
                div { class: "spacer" }
                button {
                    class: "chrome-btn theme-open",
                    onclick: move |_| theme_sheet.toggle(),
                    {glyph(Glyph::Theme, Ink::Faint, "glyph")}
                    span { {fl!("theme")} }
                }
                button {
                    class: "chrome-btn about-open",
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
                            // **The label is in a `<span>`, and it has to
                            // be.** Bare text inside a `<button>` keeps the
                            // colour it was first painted in when the theme
                            // changes under it: the surface repaints and the
                            // word does not, which leaves dark text on a dark
                            // chip until something else makes Blitz rebuild
                            // that node — clicking it, in practice, so the row
                            // corrects itself one segment at a time. Wrapping
                            // the text is what makes it a node with a style of
                            // its own. Every other button in this file happens
                            // to be built this way already, for the icon; the
                            // two segmented rows are the only ones that were
                            // not, and they were the only ones that broke. The
                            // headless harness resolves this correctly, so
                            // there is no test that would catch it coming
                            // back.
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
                                    span { {label.clone()} }
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
                        //
                        // It also *replaces* the hint rather than joining it,
                        // which is what pays for it. The hint says what the
                        // control does, and somebody who has driven the number
                        // below two has already found that out by doing it;
                        // the caution is the same sentence's worth of room
                        // spent on the thing that now matters more. Two
                        // paragraphs here is what pushed this card off the
                        // bottom of the rail in the wider face a Linux machine
                        // picks for `system-ui` — see the budget in `ui.css`.
                        if margin() < SAFE_MARGIN {
                            p { class: "warn", "data-margin-warning": "true",
                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                span { {fl!("margin-warning")} }
                            }
                        } else {
                            p { class: "hint", {fl!("margin-hint")} }
                        }
                    }
                    // Last in the rail, under the margin, which is the order
                    // the two read in: what the code is made of, after how
                    // much air is around it, both downstream of what it says.
                    //
                    // The cost of being last is that this card's caution is
                    // the lower of the two, in a column that scrolls — and it
                    // is the likelier of the two to appear, needing one click
                    // where the margin's needs somebody to walk under the
                    // app's own default. So the room it needs was found rather
                    // than borrowed from the position: a point off every
                    // card's padding and two off the gap between them, which
                    // is what keeps the banner on screen at the shortest
                    // window in the widest face. `a_caution_is_never_the_
                    // thing_that_scrolls` is the promise; what goes under the
                    // fold there is this card's own bottom edge, below the
                    // sentence rather than instead of it.
                    //
                    // It is in this rail rather than beside the colours, where
                    // it arguably belongs by subject, because that is the rail
                    // with no room: the picker leaves it a few dozen points of
                    // slack at the height `no_control_is_below_the_fold` holds
                    // the window to, and this card is a hundred and six of them
                    // before it says anything.
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
                                    // In a `<span>`, like every other chip in
                                    // the window: bare text inside a `<button>`
                                    // keeps the ink it was first painted in
                                    // when the theme changes under it.
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
                        // The one thing the test suite cannot tell anybody.
                        // All three shapes decode — `every_combination_of_
                        // shapes_scans` proves it with a real reader — but
                        // decoding in a test is not the same experience as
                        // holding a phone up to one: a rounded or dotted code
                        // gives the camera fewer clean edges, so it takes
                        // longer to focus and longer to lock on. That is a
                        // real cost, it is invisible from inside the repo, and
                        // the person paying it is standing in front of a
                        // printed code wondering whether it works.
                        //
                        // Written the way the margin caution is written, for
                        // the same reason: a caveat printed permanently is
                        // read once and then stops being read, and this one
                        // has nothing to say while the code is square. It is
                        // the same banner and the same ink, because it is the
                        // same kind of statement — the app volunteering that a
                        // choice it is perfectly willing to make has a price.
                        if look() != Look::Square {
                            p { class: "warn", "data-shape-warning": "true",
                                {glyph(Glyph::Alert, Ink::Warn, "glyph")}
                                span { {fl!("shape-warning")} }
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
                            // How big the picture is drawn, and only once
                            // there is one to draw: a size control over an
                            // empty card is a question about nothing, and the
                            // card is in the taller of the two rails.
                            //
                            // A size this code has no room for is dimmed and
                            // inert, the same way the error-correction row is
                            // while an inset holds it at 30%. It is the honest
                            // shape for it: the row's top end is set by the
                            // code's module count, the module count is set by
                            // how much text there is, and a chip that took the
                            // click and changed nothing would be the app
                            // pretending otherwise. A few more characters and
                            // it comes back.
                            div { class: "segments segments-3",
                                for choice in InsetSize::ALL {
                                    button {
                                        key: "{choice.slug()}",
                                        class: chip_class(inset_size() == choice, held(choice)),
                                        "data-inset-size": "{choice.slug()}",
                                        aria_pressed: if inset_size() == choice { "true" } else { "false" },
                                        onclick: move |_| {
                                            if !held(choice) {
                                                inset_size.set(choice);
                                            }
                                        },
                                        // In a `<span>`, like every other chip
                                        // in the window: bare text inside a
                                        // `<button>` keeps the ink it was
                                        // first painted in when the theme
                                        // changes under it.
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
                        div {
                            class: "preview",
                            style: "background: {light().to_hex()}; border-color: {mat_line(light())}",
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

            if theme_sheet() {
                div { class: "scrim", onclick: move |_| theme_sheet.set(false),
                    div {
                        class: "sheet theme-sheet",
                        // The scrim closes on a click; the panel is not the
                        // scrim.
                        onclick: move |event| event.stop_propagation(),
                        h2 {
                            {glyph(Glyph::Theme, Ink::Accent, "glyph-brand")}
                            span { {fl!("theme")} }
                        }
                        // The same segmented row the error-correction levels
                        // use, because it is the same shape of question: a
                        // short closed list where the answer in force is worth
                        // seeing without opening anything.
                        div { class: "segments segments-3",
                            for choice in Theme::ALL {
                                button {
                                    key: "{choice.slug()}",
                                    class: chip_class(theme() == choice, false),
                                    "data-theme": "{choice.slug()}",
                                    aria_pressed: if theme() == choice { "true" } else { "false" },
                                    onclick: {
                                        let remember = remember.clone();
                                        move |_| {
                                            theme.set(choice);
                                            // Written here rather than in an
                                            // effect on `theme`, so that
                                            // `--theme` and the saved value
                                            // itself seed the window without
                                            // writing themselves back.
                                            if let Some(Remember(write)) = &remember {
                                                write(choice);
                                            }
                                        }
                                    },
                                    span {
                                        match choice {
                                            Theme::System => fl!("theme-system"),
                                            Theme::Light => fl!("theme-light"),
                                            Theme::Dark => fl!("theme-dark"),
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "sheet-actions",
                            button {
                                class: "btn theme-close",
                                // The keyboard comes into the sheet with the
                                // sheet: it is what a modal should do, and it
                                // is what lets the element half of the Escape
                                // handling see the key at all.
                                autofocus: true,
                                onclick: move |_| theme_sheet.set(false),
                                {glyph(Glyph::Close, Ink::Plain, "glyph")}
                                span { {fl!("close")} }
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
                        h2 {
                            {glyph(Glyph::Code, Ink::Accent, "glyph-brand")}
                            span { {fl!("app-title")} }
                        }
                        p { {fl!("app-description")} }
                        p { class: "version", {format!("Version {}", env!("CARGO_PKG_VERSION"))} }
                        div { class: "sheet-actions",
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
                                // The keyboard comes into the sheet with the
                                // sheet: it is what a modal should do, and it
                                // is what lets the element half of the Escape
                                // handling see the key at all.
                                autofocus: true,
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
                    // The same rule the margin field follows, and for the same
                    // reason: half-typed text may sit in a field while it is
                    // being typed, but it cannot outlive the keyboard. The
                    // code is still drawn in the last colour that parsed, so a
                    // field left reading `#2f6` — or reading nothing — would be
                    // the window showing one colour and the field claiming
                    // another. Whatever is applied is what comes back.
                    onblur: move |_| {
                        draft.set(color().to_hex());
                        valid.set(true);
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
/// the stylesheet hide the one that does not belong to the theme in force.
///
/// Two of the four are a palette token written out a second time — the accent
/// and the caution, which have to be exactly the green and the gold of the
/// chrome they sit in, and `an_icon_is_inked_the_colour_the_stylesheet_says`
/// keeps the two files saying the same number. The two greys are not tokens:
/// a 1.7-pixel stroke does not carry the same weight as a line of text at the
/// same colour, so they were matched by eye against the words beside them and
/// land between the ink steps rather than on one.
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
    /// The four on a bright window.
    const fn light(self) -> &'static str {
        match self {
            Ink::Accent => "#0F9D63",
            Ink::Plain => "#2B313A",
            Ink::Faint => "#838B95",
            Ink::Warn => "#8A5B06",
        }
    }

    /// The same four on a dark one.
    ///
    /// Not the light inks inverted: an icon is a line drawing, and a line has
    /// to stay a shade away from the surface it is on in both directions —
    /// which is a different distance on paper than it is on graphite.
    const fn dark(self) -> &'static str {
        match self {
            Ink::Accent => "#4ECB8F",
            Ink::Plain => "#D2D8E0",
            Ink::Faint => "#8C949E",
            Ink::Warn => "#E9C07C",
        }
    }
}

/// The icons, as the paths that draw them on a 24×24 grid.
///
/// Hand-drawn rather than pulled from an icon font, for the reason the whole
/// app exists: a font is a file to ship and a licence to honour, and nineteen
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
    /// A circle half hatched: the theme, and the sheet that chooses it.
    Theme,
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
            // Two cells across and two down: a square, a rounded square and
            // two dots. Three outlines rather than four shapes' worth of
            // information, because the fourth cell empty reads as a mistake
            // and the dot is the one worth showing twice.
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
            // Contrast: a circle split down the middle, one half hatched. Not
            // a sun and not a moon, because those two are the *answers* and
            // this is the button that asks the question.
            Glyph::Theme => &[
                "M21 12 A9 9 0 1 1 3 12 A9 9 0 1 1 21 12",
                "M12 3 V21",
                "M12 6.2 h3.1",
                "M12 9.1 h5.2",
                "M12 12 h6",
                "M12 14.9 h5.2",
                "M12 17.8 h3.1",
            ],
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

/// One icon, as a pair of inline `<svg>`s sized by `class`.
///
/// Blitz serializes the element back to markup and parses it with `usvg`, the
/// same route the preview takes — so an icon here is a real document, not a
/// glyph in a font and not a rasterized image. That is also what makes it a
/// pair: a document of its own is a document CSS cannot reach into, so the ink
/// cannot follow the theme and the icon has to. One is drawn in each palette's
/// ink and `ui.css` hides the wrong one.
///
/// It is the price of letting somebody change the theme while the window is
/// open, and it is paid on every icon whether they ever do or not: a node and
/// a small usvg document that is laid out nowhere and painted never. Worth
/// knowing before spending the same trick on anything that appears in a list.
fn glyph(kind: Glyph, ink: Ink, class: &'static str) -> Element {
    rsx! {
        {drawn(kind, ink.light(), format!("{class} lit"))}
        {drawn(kind, ink.dark(), format!("{class} dim"))}
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
/// The outline is what tells the code's own background apart from the window
/// behind it, and `ui.css` can only guess at one of those two: the mat is
/// whatever colour somebody picked. A fixed grey answers the case the app
/// opens in — white on near-white — and disappears the moment anybody clicks
/// the middle of the greyscale row, which is four pixels from where the colour
/// is chosen. So the line is derived from the mat instead.
///
/// It is the mat pushed away from itself: towards black if the mat is light,
/// towards white if it is dark. That keeps a line on the mat's own hue rather
/// than a grey laid over it, and it is also what makes the line independent of
/// the theme: a line half a palette away from the mat clears the page on
/// either side of it, because a mat light enough to need a dark line is
/// already lighter than a dark window, and one dark enough to need a light
/// line is already darker than a light one.
///
/// The two fractions differ because the eye does. A third of the way to black
/// off white is a line you read without looking at it; the same third of the
/// way to white off black is barely there, so the dark side gets more.
fn mat_line(mat: Rgb) -> String {
    /// Luminance above which a mat counts as light, on 0…1.
    const LIGHT_ABOVE: f32 = 0.5;
    /// How far a light mat's line is pushed towards black.
    const TOWARDS_BLACK: f32 = 0.32;
    /// And a dark one's towards white.
    const TOWARDS_WHITE: f32 = 0.44;

    // The 299/587/114 weighting, which is what `read` in `qrnew-core` flattens
    // a photograph to before handing it to `rqrr`. Deliberately the same one:
    // if the app is going to call a colour light or dark, it should call it
    // what its own reader would.
    let luminance = (299.0 * f32::from(mat.r) + 587.0 * f32::from(mat.g) + 114.0 * f32::from(mat.b))
        / 255_000.0;
    let (target, amount) = if luminance > LIGHT_ABOVE {
        (0.0, TOWARDS_BLACK)
    } else {
        (255.0, TOWARDS_WHITE)
    };
    let mix = |channel: u8| (f32::from(channel) + (target - f32::from(channel)) * amount) as u8;

    Rgb::new(mix(mat.r), mix(mat.g), mix(mat.b)).to_hex()
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
///
/// **Bytes rather than a string slice, and that is a crash rather than a
/// preference.** This reads whatever is in the hex field on every keystroke,
/// the field takes any text somebody can type, and the length it switched on
/// was `str::len` — a count of bytes — while the slices it then took were
/// expected to be characters. Three bytes is `abc` and it is also `aé`, and
/// `&text[1..2]` inside that second one is not a character boundary: the field
/// panics, and a panic in a Dioxus event handler takes the window with it.
/// Every path below now indexes a byte and asks whether that byte is a hex
/// digit, which anything outside ASCII simply is not.
fn parse_hex(text: &str) -> Option<Rgb> {
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

    /// **An icon's ink and the stylesheet's have to be the same colour.**
    ///
    /// They are written twice — once as a presentation attribute in [`Ink`],
    /// because CSS cannot reach inside an SVG in Blitz, and once as a custom
    /// property in `ui.css` — and there is no way to have them written once.
    /// So the two files are compared instead: the light palette comes first in
    /// the stylesheet and the dark one second, which is the order [`Ink`]
    /// answers in.
    #[test]
    fn an_icon_is_inked_the_colour_the_stylesheet_says() {
        /// Every value `token` is given in `ui.css`, in order and without
        /// repeats.
        ///
        /// The repeats are the dark palette's, which is written twice — once
        /// for the class and once for the media query. Two colours are what
        /// this test is about; three would only be saying that
        /// `the_dark_palette_says_the_same_thing_twice` is passing, which is
        /// that test's job.
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
        // why they are not is on the type.
        for (ink, token) in [(Ink::Accent, "--accent"), (Ink::Warn, "--warn")] {
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
    /// It applies when somebody picked dark and when somebody left the choice
    /// to a dark desktop, and those are a selector and a media query — CSS
    /// gives no way to share one block between them, so `ui.css` repeats it.
    /// A colour edited in one copy and not the other would be a theme that
    /// looked subtly different depending on how it was arrived at, and nothing
    /// on screen would say which copy was in force.
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

        let chosen = block(".theme-dark {");
        assert!(chosen.len() > 20, "the palette is most of the window");
        assert_eq!(chosen, block(".theme-system {"), "the two copies have drifted");
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
    /// Each of these is three or six *bytes* and fewer than that in
    /// characters, which is the shape that used to slice through the middle of
    /// one and panic — in an event handler, on a keystroke, taking the window
    /// with it. `aé` is the whole bug in two characters.
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
