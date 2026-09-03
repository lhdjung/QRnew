// SPDX-License-Identifier: MPL-2.0

//! QRnew's interface, driven end to end without a window.
//!
//! `blitz-test-harness` builds the same [`App`] the binary launches, lays it
//! out with Stylo and Taffy, hit-tests it and dispatches real pointer, key and
//! IME events through the real event pipeline — with no window, no GPU and no
//! compositor, so this runs in CI on any platform.
//!
//! Two of these tests are about the *pointer* rather than about QRnew, and
//! they are here because Blitz surprised the app twice: a press that drifts
//! two pixels becomes a text-selection drag and never becomes a click, and a
//! marker positioned with a `radial-gradient` lands at half its offset on a
//! HiDPI display. Both were fixed in the stylesheet, and both would come
//! straight back the first time somebody tidied the fix away.
//!
//! **The libcosmic build had no test like this and could not have had one**:
//! testing `src/app.rs` meant opening a COSMIC window. Everything below is
//! about the interface, not about `qrnew-core` — the core's own tests already
//! hold the encoding, the shapes and the round trip.
//!
//! Assertions go against the preview's own document, read back out of the
//! stage, so what is checked is the SVG the app would also save to disk —
//! which `the_code_on_the_stage_is_the_code_in_the_file` is the proof of.

use blitz_test_harness::{Harness, HarnessOptions};
use blitz_traits::events::{BlitzImeEvent, UiEvent};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::ColorScheme;
use dioxus::prelude::VirtualDom;
use qrnew::ui::{App, Fill, Inlay, Remember, Theme, Tone};

/// Press a key that edits text, the way the platform running the test delivers
/// it.
///
/// **On macOS such a key arrives twice.** AppKit resolves it against the
/// system's key-binding table and hands the window the command it means —
/// `moveToBeginningOfDocument:` for Home, `deleteForward:` for Delete — and
/// then `winit` delivers the key event as well, so that an app which does not
/// implement the command still sees the key. `ui.rs` cancels the second of the
/// two, because Blitz acts on both and one press would otherwise do the thing
/// twice; the whole story is above `appkit_has_this_key` there.
///
/// A harness that sent only the key event would therefore not be a Mac, and the
/// app it drove would sit still. Everywhere else there is no command to send
/// and the key event is the whole story.
fn edit(
    harness: &mut Harness<dioxus_native::DioxusDocument>,
    key: keyboard_types::Key,
    command: &str,
) {
    #[cfg(target_os = "macos")]
    {
        harness.dispatch(UiEvent::AppleStandardKeybinding(command.into()));
        harness.pump();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = command;
    harness.press(key);
}

/// The preview's SVG, read back off the stage.
///
/// The app drops the document into `.doc` as markup, so what comes back here
/// is the `<svg>` element Blitz parsed out of it, re-serialized — which is
/// also, exactly, what Blitz hands `usvg` to draw. So this is the document on
/// screen rather than a copy of it, and
/// `the_code_on_the_stage_is_the_code_in_the_file` is what says it is still
/// the document in the file.
///
/// `None` when there is no code on screen, which is the placeholder state.
fn preview(harness: &Harness<dioxus_native::DioxusDocument>) -> Option<String> {
    let node = harness.query("[data-preview] svg")?;
    Some(harness.base().get_node(node)?.outer_html())
}

/// One document's markup, in the one form both the stage and the file can be
/// compared in.
///
/// A document that has been through the DOM differs from the file it came from
/// in exactly two ways, and `the_code_on_the_stage_is_the_code_in_the_file` is
/// what holds it to those two: the XML declaration is not an element, so it
/// does not survive being parsed into one; and Blitz's serializer writes a
/// space before the slash of an empty element. Neither is a difference `usvg`
/// can see, and nothing else moves.
fn body(document: &str) -> String {
    document
        .split_once("?>")
        .map_or(document, |(_, rest)| rest)
        .trim()
        .replace(" />", "/>")
}

/// The number of modules across the code, read off the SVG's own `viewBox`.
///
/// Taken from the document rather than from `qrnew-core`, so that the test
/// says something about what reached the screen.
fn modules_across(svg: &str) -> u32 {
    let box_attr = svg
        .split_once("viewBox=\"")
        .expect("the generated SVG has a viewBox")
        .1;
    let box_attr = box_attr.split_once('"').unwrap().0;
    box_attr
        .split_whitespace()
        .nth(2)
        .expect("a viewBox has four numbers")
        .parse()
        .expect("the viewBox is written in whole modules")
}

/// The `d` of every `<path>` in a generated code, in document order.
///
/// `draw.rs` writes the modules first, then the finder rings, then the finder
/// centres, so the first and last of these are the two halves of a shape
/// question: whether the modules are curved, and whether the corners followed
/// them.
fn outlines(svg: &str) -> Vec<&str> {
    svg.split(r#"<path fill=""#)
        .skip(1)
        .filter_map(|path| path.split_once(r#" d=""#))
        .filter_map(|(_, data)| data.split_once('"'))
        .map(|(data, _)| data)
        .collect()
}

/// Whether a path is drawn with curves.
///
/// `draw.rs` writes every curve in this document as an arc command and uses no
/// other curve command, so one letter answers it.
/// How wide the picture in the middle of the code is drawn, in points.
///
/// Measured on the screen rather than read out of a string, because the stage
/// draws the code and the picture as two layers — see the `preview` memo in
/// `ui.rs` — and the point of the arrangement is that they land as one. The
/// preview box is a fixed size whatever the code inside it is, so unlike the
/// module units this replaces, these numbers compare across codes.
fn inset_width(harness: &Harness<dioxus_native::DioxusDocument>) -> f32 {
    harness.layout_rect("[data-preview-inset]").width
}

fn curved(path: &str) -> bool {
    path.contains('a') || path.contains('A')
}

fn decode_base64(text: &str) -> Vec<u8> {
    let value = |byte: u8| -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("{} is not base64", other as char),
        }
    };

    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for chunk in text.as_bytes().chunks(4) {
        let kept = chunk.iter().filter(|&&byte| byte != b'=').count();
        let mut block = 0u32;
        for slot in 0..4 {
            let digit = chunk
                .get(slot)
                .filter(|&&byte| byte != b'=')
                .map_or(0, |&byte| value(byte));
            block |= digit << (18 - 6 * slot);
        }
        for byte in 0..kept - 1 {
            out.push(((block >> (16 - 8 * byte)) & 0xff) as u8);
        }
    }
    out
}

/// A harness with the app in it, ready for the first event.
///
/// The viewport is set rather than left at the harness default, because the
/// interface is a three-column layout that opens maximized and its stage sizes
/// the preview off `vh`. 1280×860 is the size `main.rs` falls back to when the
/// window is un-maximized, so the tests lay the app out at a size it actually
/// opens at.
fn app() -> Harness<dioxus_native::DioxusDocument> {
    let mut harness = Harness::from_component(App);
    harness.set_viewport_size(1280, 860);
    harness.pump();
    harness
}

/// The app after a few clicks, for the states a control has to be used to
/// reach. The viewport is whatever `app` opens at until a caller says
/// otherwise.
fn app_after(clicks: &[&str]) -> Harness<dioxus_native::DioxusDocument> {
    let mut harness = app();
    for click in clicks {
        harness.click(click);
    }
    harness.pump();
    harness
}

/// The same app, opened with text already in the field.
///
/// `Fill` is the root context `main.rs` provides for `--fill`, and it is the
/// only way to get a long input in front of the component without typing it:
/// `type_text` is one dispatched key event per character, and the case worth
/// testing is thousands of them.
fn app_filled(text: &str) -> Harness<dioxus_native::DioxusDocument> {
    let vdom = VirtualDom::new(App).with_root_context(Fill(text.to_string()));
    let mut harness = Harness::from_vdom(vdom, HarnessOptions::default());
    harness.set_viewport_size(1280, 860);
    harness.pump();
    harness
}

/// A picture on disk for the inset to be, and the path to it.
///
/// Written rather than checked in: it is nine lines of SVG, and a fixture file
/// is one more thing to keep in step with the code that reads it. `name` keeps
/// two tests running at once from writing the same path.
fn an_image(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("qrnew-{name}.svg"));
    std::fs::write(
        &path,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12">
             <circle cx="6" cy="6" r="5" fill="#1b3f8f"/>
           </svg>"##,
    )
    .expect("the temporary directory is writable");
    path
}

/// A picture too large for the app to carry, and the path to it.
///
/// A QR code stands in for a photograph. `qrnew-core` will write one at any
/// size and is already a dev-dependency, which beats checking in a two
/// megapixel fixture — and what matters about a photograph here is only that
/// it is larger than [`qrnew_core::MAX_LOGO_SIDE`].
fn a_large_image(name: &str) -> std::path::PathBuf {
    let png = qrnew_core::render_png(
        "a stand-in for a photograph",
        qrnew_core::ErrorCorrection::Low,
        &qrnew_core::QrStyle::default(),
        24,
    )
    .expect("the core draws a code at any scale");

    let path = std::env::temp_dir().join(format!("qrnew-{name}.png"));
    std::fs::write(&path, png).expect("the temporary directory is writable");
    path
}

/// The app opened with text in the field and a picture in the middle of it.
///
/// `Inlay` is the root context `main.rs` provides for `--inset`, and it is the
/// only way to get one in front of the component: choosing a picture means
/// working a native file dialog, which the harness has no way to touch and no
/// business opening.
fn app_with_inset(text: &str, name: &str) -> Harness<dioxus_native::DioxusDocument> {
    app_with_picture(text, &an_image(name))
}

fn app_with_picture(text: &str, image: &std::path::Path) -> Harness<dioxus_native::DioxusDocument> {
    let vdom = VirtualDom::new(App)
        .with_root_context(Fill(text.to_string()))
        .with_root_context(Inlay(image.to_string_lossy().into_owned()));
    let mut harness = Harness::from_vdom(vdom, HarnessOptions::default());
    harness.set_viewport_size(1280, 860);
    harness.pump();
    harness
}

/// The one resource this app ever loads, and the only fetcher it needs.
///
/// It is `blitz_shell::DataUriNetProvider` in four lines, because that one is
/// behind a feature of a crate the app does not depend on directly and this
/// one has to decode exactly one shape: the base64 `data:` URL [`data_url`] in
/// `ui.rs` writes. The window gets a provider of the same kind from
/// `dioxus-native`'s `data-uri` feature; the harness is handed nothing and
/// fetches nothing, which is why every test above this line lays out an
/// `<img>` that never loaded.
struct DataUri;

impl NetProvider for DataUri {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        let Some((_, payload)) = url.split_once(";base64,") else {
            return;
        };
        let bytes = Bytes::from(decode_base64(payload));
        handler.bytes(url.clone(), bytes);
    }
}

/// The app with the preview actually **decoded and laid out**, rather than
/// standing in the document as an `<img>` whose `src` nothing ever fetched.
///
/// Everything about the code as a *document* — what is in the SVG, what is
/// saved, what is copied — is checked off the `data:` URL by [`preview`] and
/// needs none of this. What needs it is the code as a *box on the stage*: an
/// image that never loaded has no intrinsic size, so it is laid out by the
/// stylesheet alone and cannot show a bug in how the two are combined. The
/// theme once did exactly that — see
/// `the_theme_does_not_take_the_code_off_the_stage`.
fn app_drawn(text: &str, image: Option<&std::path::Path>) -> Harness<dioxus_native::DioxusDocument> {
    let mut vdom = VirtualDom::new(App).with_root_context(Fill(text.to_string()));
    if let Some(image) = image {
        vdom = vdom.with_root_context(Inlay(image.to_string_lossy().into_owned()));
    }
    let mut harness = Harness::from_vdom(
        vdom,
        HarnessOptions {
            net_provider: Some(std::sync::Arc::new(DataUri)),
            ..HarnessOptions::default()
        },
    );
    harness.set_viewport_size(1280, 860);
    harness.pump();
    harness
}

/// The width and height a PNG declares in its header.
fn png_size(png: &[u8]) -> (u32, u32) {
    let number = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    (number(16), number(20))
}

#[test]
fn nothing_is_drawn_until_something_is_typed() {
    let harness = app();
    assert!(preview(&harness).is_none());
    assert!(harness.query(".placeholder").is_some());
}

#[test]
fn typing_generates_a_code() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();

    let svg = preview(&harness).expect("typing draws a code");
    assert!(svg.starts_with("<svg "), "{}", &svg[..40.min(svg.len())]);
    assert!(harness.query(".placeholder").is_none());
}

#[test]
fn the_field_holds_the_keyboard_before_anything_is_clicked() {
    // `autofocus` on the field, which is the whole of QRnew's focus policy:
    // the window opens and the app is ready to be typed into.
    let mut harness = app();
    assert_eq!(harness.focused(), harness.query(".field"));

    harness.type_text("hello");
    harness.pump();
    assert!(preview(&harness).is_some());
}

#[test]
fn error_correction_changes_the_code() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();
    let medium = preview(&harness).expect("Medium is the level the app starts on");

    harness.click("[data-ec=\"low\"]");
    harness.pump();
    let low = preview(&harness).expect("a code survives changing the level");

    harness.click("[data-ec=\"high\"]");
    harness.pump();
    let high = preview(&harness).expect("a code survives changing the level");

    assert_ne!(low, medium);
    assert_ne!(medium, high);
}

#[test]
fn high_correction_makes_a_denser_code() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org/a-url-long-enough-to-need-the-room");
    harness.pump();

    harness.click("[data-ec=\"low\"]");
    harness.pump();
    let low = modules_across(&preview(&harness).unwrap());

    harness.click("[data-ec=\"high\"]");
    harness.pump();
    let high = modules_across(&preview(&harness).unwrap());

    assert!(
        high > low,
        "High correction spends more modules on recovery than Low: {high} against {low}"
    );
}

/// **The shape row reaches the code, and the finders follow the modules.**
///
/// `ModuleShape` and `FinderShape` have been in `qrnew-core` since before the
/// rewrite — drawn, decoded and held by `every_combination_of_shapes_scans` —
/// and until now there was no way to ask for either of them from the window.
/// This is the test that says the row is wired to the code rather than only to
/// its own `aria-pressed`.
///
/// The finders are asserted separately from the modules because the app couples
/// them: one control sets both, and a shape that softened the modules and left
/// three square corners behind is the exact fault that coupling exists to
/// prevent. `draw.rs` writes the finder centres last, so the last `<path>` in
/// the document is theirs — a circle where the code is rounded, a plain
/// rectangle where it is square.
#[test]
fn the_shape_row_redraws_the_code_and_its_finders() {
    let mut harness = app_filled("https://example.org");
    let mut drawn: Vec<(&str, String)> = Vec::new();

    for look in ["square", "rounded", "dots"] {
        harness.click(&format!("[data-look=\"{look}\"]"));
        harness.pump();
        assert_eq!(
            harness
                .attr(&format!("[data-look=\"{look}\"]"), "aria-pressed")
                .as_deref(),
            Some("true"),
            "the row says which shape the code is drawn in"
        );

        let svg = preview(&harness).expect("a code survives changing its shape");
        let paths = outlines(&svg);
        let modules = paths.first().expect("the modules are the first path");
        let centers = paths.last().expect("the finder centres are the last");
        assert_eq!(
            curved(modules),
            look != "square",
            "{look}: the modules are curved exactly when they should be"
        );
        assert_eq!(
            curved(centers),
            look != "square",
            "{look}: the finder centres follow the modules"
        );
        drawn.push((look, svg));
    }

    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
        assert_ne!(
            drawn[a].1, drawn[b].1,
            "{} and {} are different codes",
            drawn[a].0, drawn[b].0
        );
    }
}

/// The shape the app opens on is the one the standard describes.
///
/// Somebody who never finds this card gets a plain code, and a plain code is
/// drawn with no curve anywhere in it — finders included.
#[test]
fn the_code_is_square_until_somebody_says_otherwise() {
    let harness = app_filled("https://example.org");
    let svg = preview(&harness).unwrap();

    for path in outlines(&svg) {
        assert!(!curved(path), "nothing in an untouched code is curved: {path}");
    }
    assert_eq!(
        harness.attr("[data-look=\"square\"]", "aria-pressed").as_deref(),
        Some("true")
    );
}

/// **Escape closes whichever sheet is open.**
///
/// Both of them, from wherever the keyboard happens to be. The handler sits on
/// the root element and Blitz bubbles a key event up from the focused node, so
/// this is as much a test of that route as of the app — and in particular of
/// the `autofocus` on each Close button, without which the focus after opening
/// a sheet is `<html>` (see `clicking_a_chip_blurs_the_field`) and a keystroke
/// bubbles away from the app rather than through it.
#[test]
fn escape_closes_a_sheet() {
    for (open, panel) in [(".about-open", ".about"), (".theme-open", ".theme-sheet")] {
        let mut harness = app();
        harness.click(open);
        harness.pump();
        assert!(harness.query(panel).is_some(), "{panel} opened");

        harness.press(keyboard_types::Key::Escape);
        harness.pump();
        assert!(harness.query(panel).is_none(), "{panel} closed on Escape");
    }
}

/// **A sheet opens with the keyboard in it, and loses it to its own buttons.**
///
/// Both halves matter, and together they are why Escape is answered twice.
/// The first is what the element handler needs: a key event goes to the
/// focused node and bubbles, so the keyboard has to be somewhere under `.app`
/// for the handler there to see it, and `autofocus` on the Close button is
/// what puts it there.
///
/// The second is the reason that is not enough. Clicking a theme clears the
/// focus to `<html>` — the same upstream rule `clicking_a_chip_blurs_the_field`
/// records — which is *above* `.app`, so from there a keystroke bubbles away
/// from the app rather than through it. That case is caught by the winit
/// handler in `App`, which no harness can reach: there is no window here to
/// deliver a `WindowEvent`.
///
/// **If the second assertion starts failing, upstream has stopped clearing the
/// focus** and the window half of the Escape handling can go.
#[test]
fn a_sheet_takes_the_keyboard_and_its_own_buttons_take_it_away() {
    let mut harness = app();
    harness.click(".theme-open");
    harness.pump();
    assert_eq!(
        harness.focused(),
        harness.query(".theme-close"),
        "the sheet opens with the keyboard in it"
    );

    harness.click("[data-theme=\"dark\"]");
    harness.pump();
    assert_ne!(
        harness.focused(),
        harness.query(".theme-close"),
        "and a click on one of its own chips takes it out again"
    );
}

/// Escape with nothing open is not a keystroke the window has any use for.
#[test]
fn escape_with_nothing_open_leaves_the_window_alone() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();
    let before = preview(&harness).unwrap();

    harness.press(keyboard_types::Key::Escape);
    harness.pump();
    assert_eq!(preview(&harness).unwrap(), before);
}

/// The hex field fills itself back in when the keyboard leaves it.
///
/// The same rule as `an_emptied_margin_field_restores_what_is_applied`, and it
/// is the same fault underneath: half-typed text is allowed in the field while
/// it is being typed, the code goes on being drawn in the last colour that
/// parsed, and a field left reading `#2f` is the window showing one colour and
/// the field claiming another.
#[test]
fn a_half_typed_hex_field_restores_what_is_applied() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();

    harness.click("[data-swatch=\"#8b1a1a\"]");
    harness.pump();
    let picked = preview(&harness).unwrap();

    // One character past a colour, rather than a field emptied on the way to
    // one: deleting `#8b1a1a` back to nothing passes through `8b1a1a` and
    // `a1a`, and both of those are colours the field is right to apply.
    harness.click("[data-hex]");
    edit(
        &mut harness,
        keyboard_types::Key::End,
        "moveToEndOfDocument:",
    );
    harness.type_text("f");
    harness.pump();
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#8b1a1af"),
        "half-typed text may sit in the field"
    );
    assert_eq!(
        preview(&harness).unwrap(),
        picked,
        "and the code is still drawn in the colour that parsed"
    );

    // Anywhere else will do; the field simply has to lose the keyboard.
    harness.click(".field");
    harness.pump();
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#8b1a1a")
    );
}

#[test]
fn a_swatch_recolors_the_code() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();
    assert!(preview(&harness).unwrap().contains("#000000"));

    // The well opens the picker; the picker's swatch sets the colour.
    harness.click("[data-well=\"dark\"]");
    harness.pump();
    harness.click("[data-swatch=\"#8b1a1a\"]");
    harness.pump();

    let svg = preview(&harness).expect("recolouring keeps the code");
    assert!(svg.contains("#8b1a1a"), "the dark modules take the swatch");
    assert!(
        !svg.contains("#000000"),
        "and nothing is left painted black"
    );
}

/// **The mat is outlined, and the outline stays visible on any mat.**
///
/// The mat is painted in the code's own background colour, and that colour is
/// a real part of the exported file: the quiet zone a scanner needs is drawn
/// in it. On a light window the mat and the page can be the same colour —
/// `#f5f4f2` is on the palette and `--bg` is `#f4f5f7` — and a code whose
/// border cannot be seen is a code whose border nobody can judge. `.preview`
/// is therefore outlined with a dash, and `mat_line` in `ui.rs` derives the
/// dash's colour from the mat, because a grey fixed against white vanishes on
/// the middle of the greyscale row.
///
/// So this walks the mat from white to the middle grey to black and asks the
/// same question at each stop: is the line still a line?
#[test]
fn the_mat_is_outlined_whatever_colour_the_mat_is() {
    /// How far apart, on 0…255 of brightness, counts as visible.
    const CLEARLY: i32 = 24;

    let luma = |(r, g, b): (u8, u8, u8)| {
        (i32::from(r) * 299 + i32::from(g) * 587 + i32::from(b) * 114) / 1000
    };
    // The mat's colour and its outline's, both off the one inline `style`.
    let mat_and_line = |harness: &Harness<dioxus_native::DioxusDocument>| {
        let style = harness
            .attr(".preview", "style")
            .expect("the mat carries its colours inline");
        let value = |property: &str| {
            let at = style
                .find(property)
                .unwrap_or_else(|| panic!("no {property} in {style:?}"));
            hex(style[at..].split_once('#').expect("a hex colour").1)
        };
        (value("background"), value("border-color"))
    };

    // Both themes, because the page behind the mat is a different colour in
    // each and the line has to clear the mat in front of it either way.
    for theme in ["light", "dark"] {
        let mut harness = app();
        harness.click(".theme-open");
        harness.pump();
        harness.click(&format!("[data-theme=\"{theme}\"]"));
        harness.pump();
        harness.click(".theme-close");
        harness.pump();

        harness.click(".field");
        harness.type_text("https://example.org");
        harness.pump();
        harness.click("[data-well=\"light\"]");
        harness.pump();

        // White, the middle of the greyscale row, and black. The middle one is
        // the case a fixed grey gets wrong.
        for swatch in ["#ffffff", "#9a9793", "#000000"] {
            harness.click(&format!("[data-swatch=\"{swatch}\"]"));
            harness.pump();

            let (mat, line) = mat_and_line(&harness);
            assert_eq!(mat, hex(swatch), "the mat took {swatch}");
            assert!(
                (luma(mat) - luma(line)).abs() >= CLEARLY,
                "on a {theme} window the outline is still visible on {swatch}: \
                 mat {mat:?} against line {line:?}"
            );
        }
    }
}

#[test]
fn the_hex_field_recolors_the_code() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    harness.click("[data-well=\"light\"]");
    harness.pump();
    harness.click("[data-hex]");
    assert_eq!(
        harness.focused(),
        harness.query("[data-hex]"),
        "clicking the hex field gives it the keyboard"
    );

    // The field opens holding `#ffffff`, and it is cleared the way a person
    // would clear it — except with Home and Delete rather than Backspace.
    // **Backspace is deliberate**: `blitz-dom` handles it on macOS only
    // through AppKit's standard key bindings, which arrive from the window
    // and which a headless harness cannot produce, so a Backspace here is a
    // no-op on one platform and works on the other two. Home and Delete go
    // through `apply_keypress_event` everywhere.
    edit(
        &mut harness,
        keyboard_types::Key::Home,
        "moveToBeginningOfDocument:",
    );
    for _ in 0.."#ffffff".len() {
        edit(&mut harness, keyboard_types::Key::Delete, "deleteForward:");
    }
    assert_eq!(harness.attr("[data-hex]", "value").as_deref(), Some(""));
    // Typed in capitals on purpose. Every hex the *app* writes is lower case —
    // the wells, the field it fills in, the SVG — but what somebody pastes in
    // is their business, so the draft keeps the case it was given and the
    // colour it parses to comes back out in the app's own casing.
    harness.type_text("#F5F4F2");
    harness.pump();
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#F5F4F2")
    );

    let svg = preview(&harness).expect("recolouring keeps the code");
    assert!(
        svg.contains("#f5f4f2"),
        "the background takes the typed hex, written the app's way"
    );
}

/// The stylesheet and the pointer maths agree about how big the square is.
///
/// `ui.rs` divides a pointer's offset by a constant because the element cannot
/// be measured from inside an event handler; the stylesheet is told the same
/// number by hand. This is the test that catches somebody changing one.
#[test]
fn the_square_is_the_size_the_maths_assumes() {
    let harness = app();

    let square = harness.layout_rect("[data-square]");
    assert_eq!((square.width, square.height), (310.0, 158.0));
    let strip = harness.layout_rect("[data-strip]");
    assert_eq!((strip.width, strip.height), (310.0, 22.0));

    // And 310 is a number that fits: the colour rail's width, less its gutter
    // and the card's padding, is exactly it. Widen one of those and the square
    // hangs out of the card it lives in, which is the failure this catches.
    let rail = harness.layout_rect(".rail-colors");
    assert!(
        square.x >= rail.x && square.x + square.width <= rail.x + rail.width,
        "the square sits inside the rail: {}..{} against {}..{}",
        square.x,
        square.x + square.width,
        rail.x,
        rail.x + rail.width
    );
}

/// The colour the modules are painted with, as three channels.
fn dark_fill(svg: &str) -> (u8, u8, u8) {
    let fill = svg
        .split_once("<path fill=\"")
        .expect("the modules are painted with a fill")
        .1
        .split_once('"')
        .unwrap()
        .0;
    hex(fill)
}

/// `rrggbb` as three bytes, with or without a leading `#`.
fn hex(text: &str) -> (u8, u8, u8) {
    let text = text.trim_start_matches('#');
    let byte = |at: usize| u8::from_str_radix(&text[at..at + 2], 16).expect("a hex pair");
    (byte(0), byte(2), byte(4))
}

/// The square sets saturation and value; the strip sets hue.
#[test]
fn the_square_and_the_strip_pick_a_color() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();
    harness.click("[data-well=\"dark\"]");
    harness.pump();

    // Black is hue 0, so the top-right corner of the square — nearly full
    // saturation, nearly full value — is red before the strip is touched.
    let square = harness.layout_rect("[data-square]");
    harness.click_at(square.x + square.width - 2.0, square.y + 2.0);
    harness.pump();
    let (r, g, b) = dark_fill(&preview(&harness).expect("picking a colour keeps the code"));
    assert!(
        r > 200 && g < 60 && b < 60,
        "the top-right corner of the square is the pure hue, not ({r}, {g}, {b})"
    );

    // A third of the way along the strip is 120 degrees, which is green.
    let strip = harness.layout_rect("[data-strip]");
    harness.click_at(strip.x + strip.width / 3.0, strip.y + strip.height / 2.0);
    harness.pump();
    let svg = preview(&harness).expect("changing the hue keeps the code");
    let (r, g, b) = dark_fill(&svg);
    assert!(
        g > 200 && r < 60 && b < 60,
        "a third of the way along the strip is green, not ({r}, {g}, {b})"
    );

    // And the hex field followed, which is what says the two halves of the
    // picker are looking at one colour rather than two.
    assert_eq!(
        harness.attr("[data-hex]", "value"),
        Some(format!("#{r:02x}{g:02x}{b:02x}"))
    );
}

/// Dragging works, and stops when the button comes up.
#[test]
fn a_drag_across_the_square_tracks_the_pointer() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();
    harness.click("[data-well=\"dark\"]");
    harness.pump();

    let square = harness.layout_rect("[data-square]");
    let left = (square.x + 4.0, square.y + square.height / 2.0);
    let right = (square.x + square.width - 4.0, square.y + 4.0);
    harness.drag(left, right, 6);
    harness.pump();
    let dragged = dark_fill(&preview(&harness).unwrap());
    assert!(
        dragged.0 > 200,
        "the drag ended at full saturation: {dragged:?}"
    );

    // The button is up now, so moving back across the square changes nothing.
    harness.move_mouse_to(left.0, left.1);
    harness.pump();
    assert_eq!(dark_fill(&preview(&harness).unwrap()), dragged);
}

#[test]
fn the_two_wells_hold_separate_colors() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    harness.click("[data-well=\"dark\"]");
    harness.pump();
    harness.click("[data-swatch=\"#1b3f8f\"]");
    harness.pump();

    harness.click("[data-well=\"light\"]");
    harness.pump();
    harness.click("[data-swatch=\"#f5f4f2\"]");
    harness.pump();

    let svg = preview(&harness).expect("recolouring keeps the code");
    assert!(
        svg.contains("#1b3f8f"),
        "the foreground kept its own colour"
    );
    assert!(
        svg.contains("#f5f4f2"),
        "and the background took the second"
    );
}

#[test]
fn reset_puts_black_and_white_back() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    harness.click("[data-well=\"dark\"]");
    harness.pump();
    harness.click("[data-swatch=\"#8b1a1a\"]");
    harness.pump();
    assert!(preview(&harness).unwrap().contains("#8b1a1a"));

    harness.click("[data-reset]");
    harness.pump();

    let svg = preview(&harness).expect("resetting keeps the code");
    assert!(svg.contains("#000000"));
    assert!(svg.contains("#ffffff"));

    // And the picker followed it. It did not before: the hex field kept its
    // own copy of the colour and only ever wrote to it, so resetting turned
    // the code black while the field went on reading `#8b1a1a`.
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#000000"),
        "the hex field says what the code is actually painted with"
    );
    assert_eq!(harness.text_content("[data-well=\"dark\"]"), "Foreground#000000");
}

/// **And it follows a reset that takes the colour caution down with it.**
///
/// The same promise as the test above, in the one state where the window has
/// two things to redraw at once. The picker follows an outside colour through
/// an effect of its own, and the caution is the other half of what a reset
/// changes — so this is the state where a caution written as a second effect
/// starves the first: the code goes black, the banner goes away, and the hex
/// field is left reading the colour that has just been reset. It is why the
/// held caution is written from the picker's pointer handlers instead.
#[test]
fn the_picker_follows_a_reset_that_takes_the_caution_down() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    // White on white: a blank square, and the card says so.
    harness.click("[data-well=\"dark\"]");
    harness.pump();
    harness.click("[data-swatch=\"#ffffff\"]");
    harness.pump();
    assert!(harness.query("[data-color-warning]").is_some());

    harness.click("[data-reset]");
    harness.pump();
    assert!(
        harness.query("[data-color-warning]").is_none(),
        "the caution goes with the colours that earned it"
    );
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#000000"),
        "and the hex field still says what the code is actually painted with"
    );
}

/// Composed text reaches the field.
///
/// `dioxus-assessment.md` named this the one honest blocker for QRnew: with no
/// IME there is no way to type Japanese, Chinese, Korean or a composed accent
/// into a field that *is* the whole application, and a QR code holding
/// Japanese text is an ordinary thing to want. Blitz has since grown IME —
/// `blitz-dom` applies preedit and commit to the focused text input, and
/// `blitz-shell` enables it on the window and tracks the cursor area — so this
/// test is here to say whether it actually works end to end, and to fail the
/// day it stops.
#[test]
fn composed_text_reaches_the_field() {
    let mut harness = app();
    harness.click(".field");

    // What an IME sends while a candidate is being chosen, and then when it is
    // accepted. The preedit is deliberately not the committed text: a field
    // that pasted the preedit in would still pass a test that only checked the
    // end state.
    harness.ime(BlitzImeEvent::Enabled);
    harness.ime(BlitzImeEvent::Preedit("にほん".into(), Some((9, 9))));
    harness.pump();
    // winit sends an empty preedit immediately before every commit, and Blitz
    // relies on it: `Commit` inserts at the selection without clearing the
    // composing region first. Leave this line out and the field ends up
    // holding "にほん日本語" — which is what a first draft of this test found,
    // and it is the harness being unrealistic rather than Blitz being wrong.
    harness.ime(BlitzImeEvent::Preedit(String::new(), None));
    harness.ime(BlitzImeEvent::Commit("日本語".into()));
    harness.pump();

    let svg = preview(&harness).expect("committed text draws a code");
    assert!(svg.contains("<svg "));

    // And it is that text, not something else: the code the app drew is the
    // code the core makes from those three characters.
    let expected = qrnew_core::Qr::new(
        "日本語",
        qrnew_core::ErrorCorrection::Medium,
        &qrnew_core::QrStyle {
            // Not `QrStyle::default()`: the core's default margin is the four
            // modules the standard asks for, and the app opens on a narrower
            // one of its own.
            quiet_zone: qrnew::ui::DEFAULT_MARGIN,
            ..qrnew_core::QrStyle::default()
        },
    )
    .expect("three characters fit in a code")
    .into_svg();
    assert_eq!(body(&svg), body(&expected));
}

/// **One press of an arrow key moves the caret one character.**
///
/// It is a test about a *number*, and the number is one. On macOS the app
/// receives such a key twice — the command AppKit resolved it into, and the key
/// event behind it — and Blitz acts on both, so a field that does not cancel
/// one of them moves two characters for every press. `edit` above delivers the
/// pair the way the platform does; what is asserted here is that only one of
/// them lands.
#[test]
fn an_arrow_moves_the_caret_one_character() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("abcd");
    harness.pump();

    edit(&mut harness, keyboard_types::Key::ArrowLeft, "moveLeft:");
    edit(&mut harness, keyboard_types::Key::ArrowLeft, "moveLeft:");
    harness.type_text("X");
    harness.pump();

    assert_eq!(
        harness.attr(".field", "value").as_deref(),
        Some("abXcd"),
        "two presses, two characters"
    );
}

/// **Backspace deletes**, which on macOS it did not.
///
/// `blitz-dom` leaves `Backspace` out of its key handling on macOS on purpose,
/// because AppKit is supposed to send `deleteBackward:` instead and nothing was
/// switching on the part of the window that receives it, so three presses of
/// Backspace on "hello world" left "hello world". `open_the_text_input_client`
/// in `ui.rs` is the fix and this is what it is for.
#[test]
fn backspace_deletes_the_character_before_the_caret() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("abcd");
    harness.pump();

    edit(
        &mut harness,
        keyboard_types::Key::Backspace,
        "deleteBackward:",
    );
    harness.pump();

    assert_eq!(harness.attr(".field", "value").as_deref(), Some("abc"));
}

/// And when something is selected, it takes the selection whole rather than the
/// one character before it.
#[test]
fn backspace_takes_a_selection_whole() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("abcd");
    harness.pump();

    harness.press_with(
        keyboard_types::Key::Character("a".into()),
        action_modifier(),
    );
    edit(
        &mut harness,
        keyboard_types::Key::Backspace,
        "deleteBackward:",
    );
    harness.pump();

    assert_eq!(harness.attr(".field", "value").as_deref(), Some(""));
}

/// **Option and an arrow jump a word**, which is what a Mac keyboard means by
/// them, and it is AppKit rather than the app that decides so.
///
/// The app's part is to stay out of the way: `moveWordLeft:` is what the
/// system's key-binding table resolves Option+Left into, `blitz-dom` already
/// implements it, and cancelling the key event that arrives beside it is what
/// stops the caret moving a word *and* a character. There is no equivalent
/// binding to test anywhere else, which is why this one is macOS only.
#[cfg(target_os = "macos")]
#[test]
fn option_and_an_arrow_jump_a_word() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("alpha beta");
    harness.pump();

    harness.dispatch(UiEvent::AppleStandardKeybinding("moveWordLeft:".into()));
    harness.pump();
    harness.press_with(
        keyboard_types::Key::ArrowLeft,
        keyboard_types::Modifiers::ALT,
    );
    harness.type_text("X");
    harness.pump();

    assert_eq!(
        harness.attr(".field", "value").as_deref(),
        Some("alpha Xbeta"),
        "one word, and not a word and a character"
    );
}

/// Select-all, on whichever key the platform spells it with.
fn action_modifier() -> keyboard_types::Modifiers {
    if cfg!(target_os = "macos") {
        keyboard_types::Modifiers::SUPER
    } else {
        keyboard_types::Modifiers::CONTROL
    }
}

/// A click on a button takes the keyboard away from the field.
///
/// Blitz walks up from the click target looking for something it knows how to
/// focus, and a plain `<button>` is on none of its lists, so the focus lands
/// on `<html>`. This is upstream's behaviour at the pinned revision and it is
/// recorded rather than worked around, because for QRnew it is also what a
/// browser does: click a button, and the field you were typing in blurs.
///
/// It used to matter for one place in particular: reading a code out of a
/// file put the text in the field, so the app had to hand the keyboard back
/// afterwards. It does not any more — what an image says is shown beside the
/// button that read it — so nothing in QRnew now depends on this. **If this
/// test starts failing, upstream has changed the rule** and the note in
/// `dioxus-assessment.md` about handing the keyboard back can be dropped.
#[test]
fn clicking_a_chip_blurs_the_field() {
    let mut harness = app();
    assert_eq!(harness.focused(), harness.query(".field"));

    harness.click("[data-ec=\"high\"]");
    harness.pump();

    assert_ne!(harness.focused(), harness.query(".field"));
}

#[test]
fn the_about_panel_opens_and_closes() {
    let mut harness = app();
    assert!(harness.query(".about").is_none());

    harness.click(".about-open");
    harness.pump();
    assert!(harness.query(".about").is_some());

    harness.click(".about-close");
    harness.pump();
    assert!(harness.query(".about").is_none());
}

#[test]
fn clicking_beside_the_about_panel_closes_it() {
    let mut harness = app();
    harness.click(".about-open");
    harness.pump();

    // Inside the panel is not outside it: the panel stops the click, or every
    // press on its own buttons would also dismiss it.
    let panel = harness.layout_rect(".about");
    harness.click_at(panel.x + panel.width / 2.0, panel.y + 8.0);
    harness.pump();
    assert!(harness.query(".about").is_some(), "the panel is not its own scrim");

    harness.click_at(24.0, 24.0);
    harness.pump();
    assert!(harness.query(".about").is_none());
}

/// **A press that drifts is still a press.**
///
/// Blitz turns a pointer that moves more than two pixels between down and up
/// into a text-selection drag, and it does not dispatch a click at the end of
/// one — so holding the button down for a moment, which is what most people do
/// when they are reading the thing they are about to press, did nothing at all
/// and selected the label instead. `user-select: none` in `ui.css` is the
/// whole fix, and it is one line, which is exactly why this test exists.
#[test]
fn a_press_that_drifts_still_counts() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    let chip = harness.layout_rect("[data-ec=\"high\"]");
    let from = (chip.x + 8.0, chip.y + chip.height / 2.0);
    let to = (from.0 + 9.0, from.1 + 3.0);
    harness.drag(from, to, 3);
    harness.pump();

    assert_eq!(
        harness.attr("[data-ec=\"high\"]", "aria-pressed").as_deref(),
        Some("true"),
        "a slow press on a button still works it"
    );

    // The same on the scrim, which is a click target for exactly one thing.
    harness.click(".about-open");
    harness.pump();
    harness.drag((24.0, 24.0), (31.0, 29.0), 3);
    harness.pump();
    assert!(
        harness.query(".about").is_none(),
        "and a slow press beside the About panel still closes it"
    );
}

/// **The layout guarantee**, and the reason the interface is three columns.
///
/// Using the colour picker must not move the code. In the single-column build
/// it did — the picker was a block between the wells and the preview, so
/// adjusting a colour pushed the thing being coloured off the bottom of the
/// window. Here the picker is a column of its own, so the preview's rectangle
/// is the same before and after — and the same again when the picker is
/// pointed at the other colour, which is the one thing left that can change
/// the rail's height.
#[test]
fn working_the_picker_does_not_move_the_code() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();

    assert!(
        harness.query("[data-square]").is_some(),
        "the picker is on screen without being opened"
    );
    let before = harness.layout_rect(".preview");

    harness.click("[data-well=\"light\"]");
    harness.pump();
    harness.click("[data-swatch=\"#8b1a1a\"]");
    harness.pump();
    let after = harness.layout_rect(".preview");

    assert_eq!(
        (before.x, before.y, before.width, before.height),
        (after.x, after.y, after.width, after.height),
        "the preview stayed exactly where it was"
    );
}

/// **The one cost of the shape row that no decoding test can see.**
///
/// All three shapes scan — `every_combination_of_shapes_scans` decodes each of
/// them with a real reader — but decoding a rendered image is not the same
/// thing as holding a phone up to a printed one. A rounded or dotted code
/// gives a camera fewer clean edges, and it takes measurably longer to focus
/// and lock on. That is a cost paid outside the repo, so the window says it,
/// and it says it only when it applies.
#[test]
fn anything_but_square_says_what_it_costs() {
    let mut harness = app_filled("https://example.org");

    assert!(
        harness.query("[data-shape-warning]").is_none(),
        "a square code has nothing to warn about"
    );

    for look in ["rounded", "dots"] {
        harness.click(&format!("[data-look=\"{look}\"]"));
        harness.pump();
        assert!(
            harness.query("[data-shape-warning]").is_some(),
            "{look} is cautioned about"
        );
        assert!(
            !harness.text_content("[data-shape-warning]").trim().is_empty(),
            "{look}: and the caution says something"
        );
    }

    harness.click("[data-look=\"square\"]");
    harness.pump();
    assert!(
        harness.query("[data-shape-warning]").is_none(),
        "and it goes away again with the shape that caused it"
    );
}

/// The three cautions are one banner, and the two in a rail come in the rail's
/// own order.
///
/// The same box is the load-bearing half. A window with three ways of saying
/// "this still works, and here is what it costs" has three things to learn
/// instead of one, and the second and third are read as something else. The
/// order is the cards' order — margin, then shape — and it is pinned here so
/// that moving a card is a decision somebody makes rather than one that
/// happens. The colour caution is in the other rail, so it is held to the box
/// and not to the order.
#[test]
fn the_cautions_are_all_the_same_banner() {
    let harness = app_after(&[
        "[data-look=\"dots\"]",
        "[data-margin-less]",
        "[data-margin-less]",
        "[data-swatch=\"#ffffff\"]",
    ]);

    let margin = harness.layout_rect("[data-margin-warning]");
    let shape = harness.layout_rect("[data-shape-warning]");
    let color = harness.layout_rect("[data-color-warning]");
    assert!(
        margin.y + margin.height <= shape.y,
        "the margin caution is above the shape's: {} against {}",
        margin.y + margin.height,
        shape.y
    );
    assert_eq!(
        shape.height, margin.height,
        "and they are the same box, not two ideas of what a caution looks like"
    );
    assert_eq!(
        color.height, margin.height,
        "and so is the colour caution, a rail away"
    );
}

/// **The colour caution is a caution, not a failure.**
///
/// `SAFE_CONTRAST` is deliberately well above where the app's own reader gives
/// up — the argument is above the constant in `ui.rs` — and this is the half of
/// that argument a test can hold: a code drawn at exactly the gap the app
/// starts warning at still decodes. So the banner is the window saying it has
/// stopped vouching, in a state where everything still works, which is the same
/// thing the margin caution is.
#[test]
fn the_colour_caution_arrives_while_the_code_still_reads() {
    // A grey foreground on white, at the threshold. The greys are equal in all
    // three channels, so their luminance is the level itself.
    let level = ((1.0 - qrnew::ui::SAFE_CONTRAST) * 255.0).round() as u8;
    let png = qrnew_core::render_png(
        "https://example.org",
        qrnew_core::ErrorCorrection::Medium,
        &qrnew_core::QrStyle {
            dark: qrnew_core::Rgb::new(level, level, level),
            light: qrnew_core::Rgb::WHITE,
            quiet_zone: qrnew::ui::DEFAULT_MARGIN,
            ..qrnew_core::QrStyle::default()
        },
        10,
    )
    .expect("the core draws a code in any colours");

    assert!(
        qrnew_core::read(&png).is_ok(),
        "a code at the gap the app cautions about still reads"
    );
}

/// **Two colours a scanner cannot tell apart are a code that does not scan,
/// and the app is the only one in the room who knows.**
///
/// It is the easiest mistake the window allows. The greyscale row is eight
/// swatches in a line and the two on the end are white and near-white; a click
/// four pixels from the one somebody meant paints the foreground the colour of
/// the background, and what comes out is a blank square that saves, copies and
/// exports exactly like a code. Nothing else in the window says so — the mat's
/// dashed outline keeps the *preview* honest about where the paper ends, which
/// is a different question.
///
/// Luminance and not hue, which is the half worth testing rather than
/// asserting: the palette's leaf green on its dark red is two colours nobody
/// would call alike and a gap of 0.038 to a camera, which flattens the picture
/// to grey before it looks for anything.
#[test]
fn the_colors_card_says_when_a_scanner_could_not_tell_them_apart() {
    let mut harness = app();
    assert!(
        harness.query("[data-color-warning]").is_none(),
        "black on white says nothing"
    );

    // The foreground well is the one the picker opens on.
    harness.click("[data-swatch=\"#ffffff\"]");
    harness.pump();
    assert!(
        harness.query("[data-color-warning]").is_some(),
        "white on white is a blank square, and the card says so"
    );

    // Two colours that look nothing alike and read as one.
    harness.click("[data-swatch=\"#1f5c36\"]");
    harness.click("[data-well=\"light\"]");
    harness.click("[data-swatch=\"#8b1a1a\"]");
    harness.pump();
    assert!(
        harness.query("[data-color-warning]").is_some(),
        "green on red is a gap of 0.038, whatever it looks like"
    );

    // And it goes away with the choice that caused it.
    harness.click("[data-reset]");
    harness.pump();
    assert!(
        harness.query("[data-color-warning]").is_none(),
        "resetting to black and white takes the caution back"
    );
}

/// **The square holds still under the hand that is drawing on it.**
///
/// The colour caution is a banner between the wells and the picker, so it
/// moves the picker every time it comes or goes, and a drag is the one way of
/// changing a colour where a pointer is on the picker while that happens. The
/// jump is the visible half; the other half is that `element_coordinates` are
/// measured against the square, so a square that has moved hands the same
/// pointer a different colour and the rest of the drag lands somewhere nobody
/// aimed at — and the colour that moved it can move it back.
///
/// So the caution is settled at the ends of a drag and not during one. The
/// wait costs nothing: the pointer is still on the colour that earned the
/// banner when the banner arrives.
#[test]
fn the_caution_waits_for_the_drag_to_finish() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();
    harness.click("[data-well=\"dark\"]");
    harness.pump();

    // The square's left edge runs from black at the bottom to white at the
    // top, and the background is white: drawing up that edge takes the
    // foreground to the one colour a scanner could not tell the paper from.
    let square = harness.layout_rect("[data-square]");
    let x = square.x + 4.0;
    harness.mouse_down_at(x, square.y + square.height - 4.0);
    harness.move_mouse_to(x, square.y + 4.0);

    assert!(
        harness.query("[data-color-warning]").is_none(),
        "the caution is not drawn while the square is being dragged"
    );
    assert_eq!(
        harness.layout_rect("[data-square]").y,
        square.y,
        "so the square is where the pointer left it"
    );

    harness.mouse_up_at(x, square.y + 4.0);
    assert!(
        harness.query("[data-color-warning]").is_some(),
        "and it arrives when the button comes up"
    );
    assert!(
        harness.layout_rect("[data-square]").y > square.y,
        "which is when the picker is allowed to move"
    );
}

/// **The inset's size is a control now, and it reaches the drawn code.**
///
/// `qrnew-core` has carried `Logo::size` since before the rewrite and the
/// window had no way to ask for anything but the default. The picture's box on
/// the stage is what says it arrived: three sizes are three different widths,
/// and the order they come in is the order the row offers.
#[test]
fn the_inset_row_draws_the_picture_at_the_size_it_asks_for() {
    let mut harness = app_with_inset("https://example.org/a-long-enough-address", "fold");
    let mut widths = Vec::new();

    for size in ["small", "medium", "large"] {
        harness.click(&format!("[data-inset-size=\"{size}\"]"));
        harness.pump();
        assert_eq!(
            harness
                .attr(&format!("[data-inset-size=\"{size}\"]"), "aria-pressed")
                .as_deref(),
            Some("true"),
            "the row says which size the picture is drawn at"
        );
        preview(&harness).expect("a code survives resizing its picture");
        widths.push((size, inset_width(&harness)));
    }

    assert!(
        widths[0].1 < widths[1].1 && widths[1].1 < widths[2].1,
        "each size is bigger than the one before it: {widths:?}"
    );
}

/// A size the code has no room for is offered as held rather than as a lie.
///
/// The ceiling is not a constant: a logo has to stay eight modules clear of
/// every edge, which is a fixed *number of modules* and so a share of the code
/// that grows with it. On the smallest code there is — a few characters, which
/// with an inset is all a 21-module code holds — a quarter of the width does
/// not fit, and one line of text later it does. The row asks the code in front
/// of it, so it can only offer what that code can take.
#[test]
fn a_code_too_small_for_a_size_does_not_offer_it() {
    let mut harness = app_with_inset("hi", "fold");

    assert_eq!(
        modules_across(&preview(&harness).expect("a short input still draws")),
        21 + 2 * 2,
        "two characters and a picture is the smallest code there is"
    );
    let large = harness
        .attr("[data-inset-size=\"large\"]", "class")
        .expect("the row is on screen");
    assert!(large.contains("off"), "the largest size is held: {large:?}");

    harness.click("[data-inset-size=\"large\"]");
    harness.pump();
    assert_eq!(
        harness
            .attr("[data-inset-size=\"medium\"]", "aria-pressed")
            .as_deref(),
        Some("true"),
        "and clicking it changes nothing, which is what held means"
    );

    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();
    let large = harness.attr("[data-inset-size=\"large\"]", "class").unwrap();
    assert!(
        !large.contains("off"),
        "a longer address is a bigger code, and it has the room: {large:?}"
    );
}

/// Text deleted out from under a size that fitted leaves a code, not a hole.
///
/// This is the half the row cannot prevent: the size is chosen against one
/// code and the next keystroke makes a different one. `qrnew-core` refuses a
/// logo that does not fit rather than shrinking it — deliberately, since only
/// the caller knows which of the two to give up — and the app is the caller,
/// and it gives up the size. Drawing nothing would be the one answer that is
/// certainly wrong: the text is fine and the picture is fine.
#[test]
fn a_shrinking_code_keeps_its_picture_and_says_the_size_is_held() {
    let mut harness = app_with_inset("https://example.org", "fold");
    harness.click("[data-inset-size=\"large\"]");
    harness.pump();
    preview(&harness).expect("a code with a large picture");
    let asked = inset_width(&harness);

    // Cleared from the front with Home and Delete rather than with Backspace,
    // for the reason `the_hex_field_recolors_the_code` sets out: Backspace
    // reaches `blitz-dom` through AppKit on macOS and never from a headless
    // harness. Two characters are left, which is a 21-module code.
    harness.click(".field");
    edit(
        &mut harness,
        keyboard_types::Key::Home,
        "moveToBeginningOfDocument:",
    );
    for _ in 0.."https://example.org".len() - 2 {
        edit(&mut harness, keyboard_types::Key::Delete, "deleteForward:");
    }
    harness.pump();

    let svg = preview(&harness).expect("the code is still drawn, at a size that fits");
    assert_eq!(modules_across(&svg), 21 + 2 * 2, "down to the smallest code");
    assert!(
        inset_width(&harness) < asked,
        "the picture was drawn smaller rather than not at all"
    );
    let large = harness.attr("[data-inset-size=\"large\"]", "class").unwrap();
    assert!(
        large.contains("on") && large.contains("off"),
        "and the row shows the size it asked for as held: {large:?}"
    );
}

/// Everything is on screen at once: neither rail has to be scrolled to reach
/// the bottom of it.
///
/// This is what the second column bought, and it is worth asserting rather
/// than eyeballing, because one more control in either card takes it away
/// again without anything else looking wrong.
///
/// **Both states of the inset card**, because they are different heights and
/// the taller one is not the one the app opens in.
///
/// **And both at 820 as well as at 860**, because 860 is the size the window
/// falls back to and not the size it opens at. Maximized on a 1512x982 laptop
/// screen, the menu bar and the Dock leave 833 points of window, of which the
/// title bar takes another 32 — so the number the guarantee has to hold at is
/// the smaller one. It did not, when the Inset card was first written: the
/// colours rail went over by fifteen pixels and quietly scrolled, which is how
/// the picker came to be thirty pixels shorter than it was.
///
/// **And the margin caution**, which is what this test was missing: it only
/// ever looked at states nobody had touched a control to reach, so that
/// caution had been overflowing the rail at 820 since it was written and no
/// run said so. It fits now — a `<p>` that never zeroed the user agent's own
/// margin was thirty points of it, and the card shows its hint or its caution
/// rather than both.
///
/// **And the colour caution**, which is a banner added to the tighter rail
/// with no hint to trade for it, and which the rail turns out to hold at both
/// heights. That is worth a row here rather than only the weaker promise: it
/// is the state a click on the wrong swatch reaches, so it is the caution most
/// people will actually see.
///
/// The shape caution is the one state a rail cannot always take, and
/// `a_caution_is_never_the_thing_that_scrolls` is what covers it instead: it
/// is a card's worth of addition with no hint to trade for it, and in the
/// wider face a Linux machine picks for `system-ui` the Shape card ends five
/// points past the rail at 820. Five points of a bottom edge, under a sentence
/// that is itself fully on screen — which is the line that test holds, and as
/// close to this one as the room in that rail goes.
#[test]
fn no_control_is_below_the_fold() {
    for height in [860, 820] {
        for (state, mut harness) in [
            ("as it opens", app()),
            ("with a picture in it", app_with_inset("https://example.org", "fold")),
            ("cautioning about the margin", app_after(&["[data-margin-less]"; 2])),
            ("cautioning about the colours", app_after(&["[data-swatch=\"#ffffff\"]"])),
        ] {
            harness.set_viewport_size(1280, height);
            harness.pump();
            for rail in [".rail-main", ".rail-colors"] {
                let box_ = harness.layout_rect(rail);
                let last = harness.layout_rect(&format!("{rail} .card:last-child"));
                assert!(
                    last.y + last.height <= box_.y + box_.height,
                    "{rail} fits without scrolling at {height} {state}: \
                     ends at {} against {}",
                    last.y + last.height,
                    box_.y + box_.height
                );
            }
        }
    }
}

/// Whatever else a rail has to scroll, the caution is not it.
///
/// This is the weaker promise that survives where `no_control_is_below_the_
/// fold` cannot reach: a caution is a banner of sixty-odd points appearing in
/// a rail that is already nearly full, and the shape's has no hint to take the
/// room from the way the margin's does. So the guarantee is about the one box
/// that matters rather than about the whole column — the sentence saying a
/// code may be slower to scan is on screen at the moment it becomes true, and
/// anything that has to go under the fold goes under it from further down.
///
/// Each caution on its own, which is how they arrive. Both at once, at 820 in
/// a wide face, is twenty-five points past what the rail holds; the shape's is
/// the lower of the two and it is the one that would go under, and the answer
/// to that state is the scrollbar the rail already has.
///
/// The colour caution is in the other rail and is held to the same promise
/// here, though it does not need it: `no_control_is_below_the_fold` covers
/// that rail in the same state, which is the stronger claim. It is in this
/// list so that the day it stops being true, the failure says which of the two
/// promises went.
#[test]
fn a_caution_is_never_the_thing_that_scrolls() {
    for height in [860, 820] {
        for (rail, caution, mut harness) in [
            (
                ".rail-main",
                "[data-shape-warning]",
                app_after(&["[data-look=\"dots\"]"]),
            ),
            (
                ".rail-main",
                "[data-margin-warning]",
                app_after(&["[data-margin-less]"; 2]),
            ),
            (
                ".rail-colors",
                "[data-color-warning]",
                app_after(&["[data-swatch=\"#ffffff\"]"]),
            ),
        ] {
            harness.set_viewport_size(1280, height);
            harness.pump();
            let rail = harness.layout_rect(rail);
            let warn = harness.layout_rect(caution);
            assert!(
                warn.y >= rail.y && warn.y + warn.height <= rail.y + rail.height,
                "{caution} is on screen at {height}: {} to {} inside {} to {}",
                warn.y,
                warn.y + warn.height,
                rail.y,
                rail.y + rail.height,
            );
        }
    }
}

/// The controls and the code are side by side, not stacked.
///
/// The code is the middle column, so this is a rail on either side of it and
/// neither one touching it — which is two assertions rather than the one it
/// was when both rails were on the same side.
#[test]
fn the_controls_and_the_code_are_in_separate_columns() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();

    let main = harness.layout_rect(".rail-main");
    let colors = harness.layout_rect(".rail-colors");
    let preview = harness.layout_rect(".preview");
    assert!(
        main.x + main.width <= preview.x,
        "the first rail ends before the code begins: {} against {}",
        main.x + main.width,
        preview.x
    );
    assert!(
        preview.x + preview.width <= colors.x,
        "the code ends before the last rail begins: {} against {}",
        preview.x + preview.width,
        colors.x
    );
    assert!(
        preview.width >= 400.0,
        "the code is still the largest thing in the window, not {}",
        preview.width
    );
}

/// **Blitz has no `placeholder` attribute**, so the app draws its own prompt.
///
/// This is the test that would have caught the previous build setting one and
/// getting nothing: it asserts on what is on screen, not on what was asked
/// for. The line then falls silent — it used to count the characters typed,
/// which is a running total of nothing — and the point of this half is that
/// falling silent does not move the button underneath it.
#[test]
fn the_field_has_a_prompt_and_then_falls_silent() {
    let mut harness = app();
    let prompt = harness.text_content(".note");
    assert!(
        !prompt.trim().is_empty(),
        "an empty field is prompted for something to do"
    );
    let button = harness.layout_rect("[data-read]");

    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    assert!(
        harness.text_content(".note").trim().is_empty(),
        "the line has nothing to say about a code that is being drawn: {:?}",
        harness.text_content(".note")
    );
    assert!(harness.query(".note.bad").is_none());
    assert_eq!(
        harness.layout_rect("[data-read]").y,
        button.y,
        "and saying nothing takes up the same room as saying something"
    );
}

/// Text past what the densest code can hold says so.
///
/// The libcosmic build — and this one until now — showed the empty-stage
/// placeholder and explained nothing, which reads as the app having stopped
/// working rather than as the input being too long.
#[test]
fn text_too_long_for_a_code_says_why() {
    let harness = app_filled(&"x".repeat(3000));

    assert!(preview(&harness).is_none(), "no code was drawn");
    assert!(
        harness.query(".note.bad").is_some(),
        "and the field says why: {:?}",
        harness.text_content(".note")
    );
}

/// **The marker goes where the pointer went.**
///
/// The picker used to draw it with `radial-gradient(circle at Xpx Ypx, …)`,
/// and Blitz resolves that centre in CSS pixels and then adds it to a
/// rectangle it has already measured in device pixels. On a 1× display that is
/// the same number and everything looks right; on a 2× display the mark landed
/// at half the offset it was given, so the colour under the pointer was
/// correct and the ring was somewhere else — which is what it looked like from
/// the outside: "the colour follows the cursor, the circle does not".
///
/// `background-position` *is* multiplied by the scale, so the marker is placed
/// with that instead. This test pins both halves: the position tracks the
/// click, and no radial gradient has crept back in.
#[test]
fn the_picker_marks_where_it_was_clicked() {
    let mut harness = app();

    let square = harness.layout_rect("[data-square]");
    let target = (square.x + square.width * 0.75, square.y + square.height * 0.25);
    harness.click_at(target.0, target.1);
    harness.pump();

    let style = harness.attr("[data-square]", "style").expect("the square is painted");
    assert!(
        !style.contains("radial-gradient"),
        "nothing here may be positioned with a radial gradient: {style}"
    );

    // The marker's outermost layer is a 20px square, so its top-left corner is
    // ten pixels up and left of where the pointer was.
    let x = square.width * 0.75 - 10.0;
    let y = square.height * 0.25 - 10.0;
    let corner = format!("{x:.1}px {y:.1}px");
    assert!(
        style.contains(&corner),
        "the marker sits at {corner}, not at: {style}"
    );

    // The hue strip carries the same marker, moved by the same maths.
    let strip = harness.layout_rect("[data-strip]");
    harness.click_at(strip.x + strip.width / 2.0, strip.y + strip.height / 2.0);
    harness.pump();
    let style = harness.attr("[data-strip]", "style").expect("the strip is painted");
    let x = strip.width / 2.0 - 10.0;
    assert!(
        style.contains(&format!("{x:.1}px 1.0px")),
        "the strip's marker followed the click: {style}"
    );
}

/// The wells point the picker at one colour or the other.
///
/// They used to open and close it, which is what made the second column worth
/// having: with the picker always on screen the wells only have to say what it
/// is editing, and switching between them is one click rather than two.
#[test]
fn the_wells_choose_what_the_picker_edits() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#000000"),
        "the picker starts on the foreground"
    );

    harness.click("[data-well=\"light\"]");
    harness.pump();
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#ffffff"),
        "and follows the well that was clicked"
    );

    // Clicking the well it is already on leaves it there rather than taking
    // the picker away, which is what the old toggle did.
    harness.click("[data-well=\"light\"]");
    harness.pump();
    assert!(harness.query("[data-square]").is_some());
    assert_eq!(harness.attr("[data-hex]", "value").as_deref(), Some("#ffffff"));
}

/// The margin control widens and narrows the blank border.
///
/// `modules_across` reads the SVG's own `viewBox`, and the quiet zone is part
/// of that box on both sides — so a margin one module wider is a document two
/// modules wider, which is what makes this measurable from the outside at all.
#[test]
fn the_stepper_resizes_the_margin() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    // Five characters fit in the smallest code there is: 21 modules, plus the
    // app's own margin on either side.
    let opened = modules_across(&preview(&harness).unwrap());
    assert_eq!(opened, 21 + 2 * qrnew::ui::DEFAULT_MARGIN);
    assert_eq!(
        harness.attr("[data-margin]", "value").as_deref(),
        Some(qrnew::ui::DEFAULT_MARGIN.to_string().as_str()),
        "and the field says the same number the code was drawn with"
    );

    harness.click("[data-margin-more]");
    harness.pump();
    assert_eq!(modules_across(&preview(&harness).unwrap()), opened + 2);
    assert_eq!(harness.attr("[data-margin]", "value").as_deref(), Some("3"));

    harness.click("[data-margin-less]");
    harness.click("[data-margin-less]");
    harness.click("[data-margin-less]");
    harness.pump();
    assert_eq!(
        modules_across(&preview(&harness).unwrap()),
        21,
        "a margin of nothing is the bare code"
    );

    // And it stops there rather than wrapping around, which is the one thing
    // an unsigned counter does wrong when nobody is looking.
    harness.click("[data-margin-less]");
    harness.pump();
    assert_eq!(harness.attr("[data-margin]", "value").as_deref(), Some("0"));
    assert_eq!(modules_across(&preview(&harness).unwrap()), 21);
}

/// The field takes a number, and refuses to take a silly one.
#[test]
fn the_margin_field_clamps_what_it_is_given() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    harness.click("[data-margin]");
    edit(
        &mut harness,
        keyboard_types::Key::Home,
        "moveToBeginningOfDocument:",
    );
    for _ in 0..4 {
        edit(&mut harness, keyboard_types::Key::Delete, "deleteForward:");
    }
    harness.type_text("6");
    harness.pump();
    assert_eq!(modules_across(&preview(&harness).unwrap()), 21 + 12);

    // Past the maximum the field corrects itself to it, rather than refusing
    // the keystroke and leaving somebody wondering which one did not land.
    harness.type_text("00");
    harness.pump();
    assert_eq!(
        harness.attr("[data-margin]", "value").as_deref(),
        Some(qrnew::ui::MAX_MARGIN.to_string().as_str())
    );
    assert_eq!(
        modules_across(&preview(&harness).unwrap()),
        21 + 2 * qrnew::ui::MAX_MARGIN
    );
}

/// The caution about narrow margins is there when it applies and not before.
///
/// It used to be the second half of the sentence under the card, printed
/// whether or not it was true of the value on screen — which is the shape of
/// warning that gets read once and then stops being read. It is now beside the
/// number, and it appears the moment somebody goes under two.
#[test]
fn the_margin_warning_only_appears_below_two() {
    let mut harness = app();
    assert!(
        harness.query("[data-margin-warning]").is_none(),
        "nothing to warn about at the margin the app opens with"
    );

    harness.click("[data-margin-less]");
    harness.pump();
    assert_eq!(harness.attr("[data-margin]", "value").as_deref(), Some("1"));
    let warning = harness.text_content("[data-margin-warning]");
    assert!(
        warning.contains("scanners"),
        "the caution is on screen, not {warning:?}"
    );

    harness.click("[data-margin-more]");
    harness.pump();
    assert!(
        harness.query("[data-margin-warning]").is_none(),
        "and it goes away again with the value that caused it"
    );
}

/// An emptied margin field fills itself back in when the keyboard leaves it.
///
/// A half-typed number is allowed to sit in the field — that is the only way
/// to replace a value by typing — but the code goes on being drawn at the last
/// number that parsed. So an empty field left behind is the app showing one
/// margin and the field claiming another, and whatever is applied is what has
/// to come back.
#[test]
fn an_emptied_margin_field_restores_what_is_applied() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    harness.click("[data-margin]");
    edit(
        &mut harness,
        keyboard_types::Key::Home,
        "moveToBeginningOfDocument:",
    );
    for _ in 0..4 {
        edit(&mut harness, keyboard_types::Key::Delete, "deleteForward:");
    }
    harness.pump();
    assert_eq!(
        harness.attr("[data-margin]", "value").as_deref(),
        Some(""),
        "the field can be emptied on the way to a new number"
    );
    assert_eq!(
        modules_across(&preview(&harness).unwrap()),
        21 + 2 * qrnew::ui::DEFAULT_MARGIN,
        "and the code is still drawn at the margin that was applied"
    );

    // Anywhere else will do; the field simply has to lose the keyboard.
    harness.click(".field");
    harness.pump();
    assert_eq!(
        harness.attr("[data-margin]", "value").as_deref(),
        Some(qrnew::ui::DEFAULT_MARGIN.to_string().as_str())
    );
}

/// **The window is the same width on both sides of the code**, which stopped
/// being free the moment the stage moved into the middle.
///
/// A rail reserves eight points on its right for the overlay scrollbar that
/// may or may not come down it. While both rails were on the same side of the
/// stage, both of those gutters fell inside a gap and neither was visible;
/// with a rail on each side, the far one's gutter is against the window's own
/// edge, and eight points of air on one side only is the kind of thing that is
/// seen without being noticed. `.rail-colors` and `.body`'s asymmetric padding
/// are the two halves of the answer, and this is what says they still add up:
/// the cards are the same width, they end the same distance from their edges,
/// and the code is the same distance from each of them.
///
/// Measured at the width the window falls back to and at the narrowest it can
/// be dragged, because the stage is the only flexible column and both numbers
/// are its.
#[test]
fn the_stage_sits_evenly_between_the_two_rails() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");

    for width in [1160, 1280] {
        harness.set_viewport_size(width, 860);
        harness.pump();

        let body = harness.layout_rect(".body");
        let left = harness.layout_rect(".rail-main .card:first-child");
        let right = harness.layout_rect(".rail-colors .card:first-child");
        let stage = harness.layout_rect(".stage");

        assert_eq!(
            left.width, right.width,
            "the two rails hold cards of one width at {width}"
        );
        assert_eq!(
            left.x - body.x,
            body.x + body.width - (right.x + right.width),
            "and the outer card edges are the same distance from the window's at {width}"
        );
        assert_eq!(
            stage.x - (left.x + left.width),
            right.x - (stage.x + stage.width),
            "and the code is the same distance from each of them at {width}"
        );
    }
}

/// The code starts at the height the controls start at.
///
/// Centring the stage put the one thing the window is about lower than
/// everything beside it. This is a layout claim rather than a taste one: the
/// three columns share a top edge.
#[test]
fn the_code_is_level_with_the_controls() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();

    let card = harness.layout_rect(".rail-main .card:first-child");
    let preview = harness.layout_rect(".preview");
    assert!(
        (preview.y - card.y).abs() < 2.0,
        "the code and the first card begin together: {} against {}",
        preview.y,
        card.y
    );
}

/// **Every hex the app writes is lower case.**
///
/// It is one string built in one place — `Rgb::to_hex` — and this is the test
/// that says so from the outside: the tile somebody reads the colour off, the
/// field they edit it in, and the document that gets saved.
#[test]
fn the_app_writes_hex_in_lower_case() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    harness.click("[data-well=\"dark\"]");
    harness.pump();
    harness.click("[data-swatch=\"#8b1a1a\"]");
    harness.pump();

    let well = harness.text_content("[data-well=\"dark\"]");
    assert!(well.ends_with("#8b1a1a"), "the tile reads {well:?}");
    assert_eq!(
        harness.attr("[data-hex]", "value").as_deref(),
        Some("#8b1a1a")
    );
    assert!(preview(&harness).unwrap().contains("#8b1a1a"));
}

/// **The number sits in the middle of its field.**
///
/// `text-align: center` is what a browser would need and what this field used
/// to carry, and in Blitz it does nothing at all: a text field's content
/// belongs to a `parley::PlainEditor` that `create_text_editor` hands a font
/// size, a line height and a brush — not an alignment, and not a width to
/// align inside. The glyphs are painted at the content box's left edge, so the
/// number sat against the left border with a stepper button on either side of
/// it, looking like a value that had come loose.
///
/// The centring is arithmetic now, done with `padding-left` in `ch` units, and
/// this measures the result rather than the intent: where the text is actually
/// painted, against the middle of the box it is painted in. It is also what
/// ties `COUNT_MIDDLE` in `ui.rs` to `.count`'s width in `ui.css` — change one
/// without the other and the number comes off centre here.
///
/// **The pixel of slack is Blitz's, and it is worth knowing about.** `1ch` is
/// resolved by `BlitzFontMetricsProvider`, which asks fontique for the advance
/// of `0` in the *specified* family list; the glyphs are painted by parley,
/// which resolves that same list its own way. On macOS the two land on faces
/// 5% apart — `ch` comes back 10px where the digit actually drawn is 9.45px at
/// this size — so two digits end up half a pixel left of centre. The digits
/// themselves are tabular, which is what keeps the error to that: `16` is
/// exactly twice the width of `4`.
#[test]
fn the_margin_number_is_centered_in_its_field() {
    let mut harness = app();

    // Two digits and then one, because the padding is written per digit and a
    // formula that is right for one width is not necessarily right for both.
    for expected in ["16", "4"] {
        harness.click("[data-margin]");
        edit(
            &mut harness,
            keyboard_types::Key::Home,
            "moveToBeginningOfDocument:",
        );
        for _ in 0..4 {
            edit(&mut harness, keyboard_types::Key::Delete, "deleteForward:");
        }
        harness.type_text(expected);
        harness.pump();
        assert_eq!(
            harness.attr("[data-margin]", "value").as_deref(),
            Some(expected)
        );

        let node = harness.node("[data-margin]");
        let doc = harness.base();
        let field = doc.get_node(node).expect("the margin field");
        let layout = field.final_layout();
        // The painted width of the digits, straight out of the editor that
        // paints them — this is the one measurement the app cannot take for
        // itself, which is why the arithmetic it does instead is checked here.
        let text = field
            .data
            .downcast_element()
            .and_then(|element| element.text_input_data())
            .and_then(|input| input.editor.try_layout())
            .expect("the field lays its text out through a parley editor")
            .width();

        let starts_at = layout.border.left + layout.padding.left;
        let slack = layout.size.width - (starts_at + text);
        assert!(
            (starts_at - slack).abs() < 1.5,
            "{expected:?} sits {starts_at} from the left and {slack} from the right \
             in a field {} wide",
            layout.size.width,
        );
    }
}

/// The caution about a narrow margin is a banner, not a footnote.
///
/// It used to be the last item in the stepper's row, in whatever space the two
/// buttons and the field left over — 12.5px of grey-gold beside a 17px number.
/// That is the size and the position of an aside, and this one is the only
/// line in the window that says a code might not scan. It is a block of its
/// own under the control now, the full width of the card, with an icon.
#[test]
fn the_margin_warning_is_a_banner_under_the_control() {
    let mut harness = app();
    harness.click("[data-margin-less]");
    harness.pump();

    assert!(
        harness.query(".stepper [data-margin-warning]").is_none(),
        "it is no longer tucked into the stepper's row"
    );
    assert!(
        harness.query("[data-margin-warning] .glyph").is_some(),
        "and it carries the one alert icon in the app"
    );

    let stepper = harness.layout_rect(".stepper");
    let warning = harness.layout_rect("[data-margin-warning]");
    assert!(
        warning.y >= stepper.y + stepper.height,
        "it sits below the control it is about, not beside it: {} against {}",
        warning.y,
        stepper.y + stepper.height,
    );
    assert!(
        warning.width > stepper.width * 0.9,
        "and it has the width of the card rather than the room left over: {} of {}",
        warning.width,
        stepper.width,
    );
}

/// **A picture reaches the middle of the code.**
///
/// The whole path, end to end: the bytes are read off disk, the format is
/// worked out from the bytes rather than from the file's name, and they come
/// out again on the stage, in the hole the code left for them. The card shows
/// what was chosen, so that a picture that has landed somewhere unexpected can
/// be told from one that was never read.
///
/// It is the layer that is checked rather than the document, because the
/// document on screen does not carry the picture any more — see
/// `the_picture_on_the_stage_is_where_the_document_puts_it` for why. What gets
/// saved is still one document with everything in it, which is `qrnew-core`'s
/// to prove and does.
#[test]
fn an_inset_reaches_the_code_and_the_card() {
    let harness = app_with_inset("https://example.org", "reaches");

    preview(&harness).expect("a code is drawn to put the picture in");
    let drawn = harness
        .attr("[data-preview-inset]", "src")
        .expect("the picture is on the stage");
    assert!(
        drawn.starts_with("data:image/svg+xml;base64,"),
        "declared as what the bytes are rather than as what the name claimed"
    );

    assert!(
        harness.query("[data-inset-thumb]").is_some(),
        "the card shows it back"
    );
    assert!(
        harness.text_content("[data-inset-name]").contains("qrnew-"),
        "under the name of the file it came from: {:?}",
        harness.text_content("[data-inset-name]")
    );
}

/// **An inset takes error correction over, and the row says so.**
///
/// `Qr::new` raises the level to `High` whenever there is a logo, whatever it
/// was asked for — the modules the picture covers have to be paid for
/// somewhere. Before this the four buttons went on showing 15% while the code
/// was being drawn at 30%, which is the interface lying about the file it is
/// about to save. The choice underneath is remembered rather than overwritten,
/// so removing the picture gives it back.
#[test]
fn an_inset_holds_error_correction_at_thirty_percent() {
    let mut harness = app_with_inset("https://example.org", "correction");

    let denser = modules_across(&preview(&harness).unwrap());
    assert_eq!(
        harness.attr("[data-ec=\"high\"]", "aria-pressed").as_deref(),
        Some("true"),
        "the row shows the level the code is actually drawn at"
    );
    assert!(
        harness
            .attr("[data-ec=\"high\"]", "class")
            .is_some_and(|class| class.contains("off")),
        "and shows itself as held rather than chosen"
    );

    // A press lands on a button that is not taking any, and changes nothing.
    harness.click("[data-ec=\"low\"]");
    harness.pump();
    assert_eq!(
        harness.attr("[data-ec=\"high\"]", "aria-pressed").as_deref(),
        Some("true"),
    );
    assert_eq!(modules_across(&preview(&harness).unwrap()), denser);

    // Take the picture away and the row is a control again, still set to what
    // it was set to before the inset arrived.
    harness.click("[data-inset-remove]");
    harness.pump();
    assert!(harness.query("[data-inset-thumb]").is_none());
    assert_eq!(
        harness
            .attr("[data-ec=\"medium\"]", "aria-pressed")
            .as_deref(),
        Some("true"),
        "the level the app opens at comes back"
    );
    assert!(
        modules_across(&preview(&harness).unwrap()) < denser,
        "and the code is the loose one again"
    );
}

/// Text that no longer fits *because* of the inset says which two things can
/// give.
///
/// Error correction at 30% costs capacity, so an inset can be what takes a
/// code past what it can hold. "That is more text than a single QR code can
/// hold" would be true and useless there: the text is not the only thing that
/// can move.
#[test]
fn text_too_long_with_an_inset_says_the_inset_can_go() {
    // Comfortably inside what a code holds at 15%, and past what one holds at
    // 30% — which is the window this message exists for.
    let harness = app_with_inset(&"x".repeat(1500), "toolong");

    assert!(preview(&harness).is_none(), "no code was drawn");
    let note = harness.text_content(".note.bad");
    assert!(
        note.contains("image"),
        "the way out includes the picture: {note:?}"
    );
}

/// **`Copied.` lets go of the button again.**
///
/// A confirmation that stays up stops being a confirmation and becomes the
/// button's name, and then it is a claim about the clipboard that nothing is
/// checking. It comes down after `CONFIRM_FOR`, on a countdown that runs on a
/// thread of its own and wakes the app when it finishes — which is the half of
/// this that a unit test cannot reach: `a_countdown_finishes_when_it_says_it_will`
/// proves the timer fires and wakes its waker, and this proves the wake-up
/// travels through Blitz's event loop and back into the document.
///
/// **Conditional on there being a clipboard.** Raising the confirmation means
/// actually putting an image on one, and `arboard` has nothing to talk to on a
/// CI runner with no display. Where there is no clipboard the button says what
/// it always said, and the test has nothing to check.
#[test]
fn a_copied_button_goes_back_to_its_own_name() {
    let mut harness = app_filled("https://example.org");

    let copy = "[data-copy-image]";
    let name = harness.text_content(copy);
    harness.click(copy);
    harness.pump();

    let confirmed = harness.text_content(copy);
    if confirmed == name {
        eprintln!("no clipboard on this machine; nothing to confirm");
        return;
    }

    // Slack in one direction only, and on the far side of the countdown: the
    // thread is allowed to oversleep and the app is allowed to be polled late.
    std::thread::sleep(qrnew::ui::CONFIRM_FOR + std::time::Duration::from_millis(400));
    harness.pump();
    assert_eq!(
        harness.text_content(copy),
        name,
        "the button is called what it was called before the click"
    );
}

/// **A picture larger than the renderer's texture atlas is scaled down as it
/// is taken in, not carried at the size it arrived.**
///
/// This is the crash that made the app close: `vello_hybrid` keeps its images
/// in a 4096-pixel atlas and does not draw one that will not fit — it refuses,
/// and unwraps the refusal. Any photograph off a phone is over that line, and
/// choosing one took the window with it.
///
/// Checked on what reaches the screen rather than on `shrink_logo`, which
/// `qrnew-core` tests on its own: what is worth proving here is that a picture
/// coming through the one door there is comes out the other side already
/// small.
///
/// The picture is drawn as its own layer over the code now, so the `<img>` on
/// the stage is where it is read from — and the same bytes are what the code
/// is built with, so measuring one measures both. That layer is also the
/// second half of the answer to this crash: it is a size the atlas can take,
/// *and* it is uploaded once instead of once per keystroke.
#[test]
fn a_picture_too_large_to_carry_is_scaled_down_as_it_is_taken_in() {
    let path = a_large_image("large");
    let on_disk = png_size(&std::fs::read(&path).unwrap());
    assert!(
        on_disk.0 > qrnew_core::MAX_LOGO_SIDE,
        "the fixture has to be too large to begin with: {on_disk:?}"
    );

    let harness = app_with_picture("https://example.org", &path);
    let drawn = harness.attr("[data-preview-inset]", "src").unwrap();
    let carried = png_size(&decode_base64(
        drawn
            .strip_prefix("data:image/png;base64,")
            .expect("the picture on the stage is the PNG the app made"),
    ));

    assert_eq!(
        carried,
        (qrnew_core::MAX_LOGO_SIDE, qrnew_core::MAX_LOGO_SIDE)
    );

    // And the thumbnail is the same picture, not the file that was picked.
    let thumbnail = harness.attr("[data-inset-thumb]", "src").unwrap();
    assert_eq!(thumbnail, drawn, "one picture, drawn in two places");
}

/// **The picture on the stage is where the saved document puts it.**
///
/// The preview is two layers: the code with a hole where the inset goes, and
/// the picture laid into the hole. That is not a nicety — handing the renderer
/// the picture inside a *new* document on every keystroke fills
/// `vello_hybrid`'s image atlas, which keys what it has already uploaded by an
/// identity counter and frees nothing, and the end of that is
/// `AtlasLimitReached` unwrapped two crates down. At the 512-pixel cap
/// `shrink_logo` imposes, eight atlases of 4096 square hold 392 of them.
///
/// Two layers are only worth having if they land as one, and `qrnew-core`'s
/// own tests only go as far as the numbers it hands over. This is the other
/// half: that the window puts the picture where it was told to, in points on
/// the screen.
#[test]
fn the_picture_on_the_stage_is_where_the_document_puts_it() {
    let harness = app_with_inset("https://example.org", "spot");

    let svg = preview(&harness).expect("a code with a picture in it");
    assert!(
        !svg.contains("<image"),
        "the document on screen carries no picture, which is the whole point"
    );

    let code = harness.layout_rect("[data-preview]");
    let inset = harness.layout_rect("[data-preview-inset]");

    // **Over the code, not under it**, which layout alone cannot say and a
    // headless harness cannot paint. Hit testing is the way to ask: it walks
    // `paint_children` in reverse, so the node it returns for a point is the
    // one drawn last there. A picture in exactly the right place and behind
    // the code it belongs in front of would pass every other line here.
    let (x, y) = harness.center_of("[data-preview-inset]");
    assert_eq!(
        harness.hit_node(x, y),
        harness.node("[data-preview-inset]"),
        "the picture is painted over the code"
    );

    // Centred on both axes, to within the rounding a percentage of a box that
    // is not a whole number of points can produce.
    let left = inset.x - code.x;
    let right = (code.x + code.width) - (inset.x + inset.width);
    let top = inset.y - code.y;
    assert!(
        (left - right).abs() < 1.0,
        "centred across the code: {left} against {right}"
    );
    assert!(
        (left - top).abs() < 1.0,
        "and the same distance down: {left} against {top}"
    );

    // And as wide as the document says. The core's default share is a share of
    // the *code*, which the quiet zone does not widen, so the fraction of the
    // box on screen is smaller than the fraction of the code by that much.
    let document = modules_across(&svg) as f32;
    let modules = document - 2.0 * qrnew::ui::DEFAULT_MARGIN as f32;
    let expected = code.width * qrnew_core::Logo::DEFAULT_SIZE * modules / document;
    assert!(
        (inset.width - expected).abs() < 1.0,
        "the picture is {} points across where the document asks for {expected}",
        inset.width
    );
}

/// **The theme is the desktop's until somebody says otherwise.**
///
/// Three answers, and two mechanisms behind them that have to agree. The
/// palette is a class on `.app` — a class rather than a media query because
/// nothing a component can call moves `prefers-color-scheme`, and because
/// macOS stops reporting theme changes the moment the app sets an appearance
/// of its own. The *icons* cannot be a class at all: an icon's ink is a
/// presentation attribute on an SVG that Blitz hands to `usvg` as a document
/// of its own, so `glyph` in `ui.rs` draws each one twice and `ui.css` hides
/// one of the pair.
///
/// If those two ever disagree the window is half-themed — dark surfaces under
/// icons drawn for a light one — and nothing fails until somebody looks. So
/// each case below checks both: the class the app is wearing, and which half
/// of every icon pair survived layout.
#[test]
fn the_theme_is_the_desktops_until_somebody_picks_one() {
    // How many of a class are actually laid out. A hidden icon keeps its node
    // and loses its box, which is exactly the distinction being tested.
    let shown = |harness: &Harness<dioxus_native::DioxusDocument>, class: &str| {
        harness
            .query_all(class)
            .into_iter()
            .filter(|node| harness.layout_rect_of(*node).width > 0.0)
            .count()
    };

    // The desktop, what gets picked from the sheet, and the palette that
    // should come out of the two together.
    for (desktop, pick, wanted) in [
        (ColorScheme::Light, None, "light"),
        (ColorScheme::Dark, None, "dark"),
        // Either desktop, overruled in both directions.
        (ColorScheme::Dark, Some("light"), "light"),
        (ColorScheme::Light, Some("dark"), "dark"),
        // And back to the desktop, which is the answer somebody arrives at by
        // changing their mind rather than by never opening the sheet.
        (ColorScheme::Dark, Some("system"), "dark"),
    ] {
        let vdom = VirtualDom::new(App).with_root_context(Fill("https://example.org".into()));
        let mut harness = Harness::from_vdom(
            vdom,
            HarnessOptions {
                color_scheme: desktop,
                ..HarnessOptions::default()
            },
        );
        harness.set_viewport_size(1280, 860);
        harness.pump();

        let chosen = pick.unwrap_or("system");
        if let Some(pick) = pick {
            harness.click(".theme-open");
            harness.pump();
            harness.click(&format!("[data-theme=\"{pick}\"]"));
            harness.pump();
            harness.click(".theme-close");
            harness.pump();
        }

        let case = format!("{desktop:?} desktop, {chosen} picked");
        assert!(
            harness.query(&format!(".app.theme-{chosen}")).is_some(),
            "{case}: the root wears the choice, not the outcome"
        );

        // And the icons agree with the palette that choice resolves to.
        let pairs = harness.query_all(".lit").len();
        assert!(pairs > 10, "{case}: the window is full of icons");
        assert_eq!(harness.query_all(".dim").len(), pairs, "{case}: one of each");
        let (lit, dim) = if wanted == "dark" { (0, pairs) } else { (pairs, 0) };
        assert_eq!(shown(&harness, ".lit"), lit, "{case}: light-ink icons shown");
        assert_eq!(shown(&harness, ".dim"), dim, "{case}: dark-ink icons shown");
    }
}

/// **The code on the stage is the code in the file.**
///
/// The preview is markup dropped into the stage rather than a `data:` URL on
/// an `<img>`, so what `usvg` draws is the `<svg>` element Blitz parsed out of
/// that markup and then serialized again — not the bytes `qrnew-core` handed
/// over. This is the test that the round trip changes nothing, and it is what
/// every assertion in this file that goes through [`preview`] rests on.
///
/// The two differences it does allow are the two [`body`] normalizes, and they
/// are checked here rather than taken on trust: the XML declaration is not an
/// element and does not survive being parsed into one, and Blitz writes a
/// space before the slash of an empty element. The second is counted, so a
/// third difference cannot hide inside the substitution.
#[test]
fn the_code_on_the_stage_is_the_code_in_the_file() {
    // A style with something in every field the document can carry: a colour
    // that is not black, curves rather than squares, and a hole in the middle
    // for a picture.
    const TEXT: &str = "https://example.org/a-url-long-enough-to-need-a-few-versions";
    let image = an_image("same-document");
    let mut harness = app_with_picture(TEXT, &image);
    harness.click("[data-look=\"rounded\"]");
    harness.click("[data-swatch=\"#1b3f8f\"]");
    harness.pump();

    let on_screen = preview(&harness).expect("a code is on the stage");
    let in_the_file = qrnew_core::Qr::new(
        TEXT,
        // An inset holds the level at 30% whatever the row says.
        qrnew_core::ErrorCorrection::High,
        &qrnew_core::QrStyle {
            dark: qrnew_core::Rgb::new(0x1b, 0x3f, 0x8f),
            quiet_zone: qrnew::ui::DEFAULT_MARGIN,
            module: qrnew_core::ModuleShape::Rounded,
            finder: qrnew_core::Finder {
                shape: qrnew_core::FinderShape::Rounded,
                ring: None,
                center: None,
            },
            logo: Some(qrnew_core::Logo::new(std::fs::read(&image).unwrap())),
            ..qrnew_core::QrStyle::default()
        },
    )
    .expect("the core draws the same code")
    // The stage carries the document with the hole and without the picture;
    // the picture is the layer over it. What is *saved* is `Qr::svg`, which is
    // this document with the picture in it.
    .svg_without_inset();

    assert_eq!(body(&on_screen), body(&in_the_file), "the same document");

    // And the allowance is exactly the two things named above. The declaration
    // is the head of the file and nothing else went missing; every other byte
    // of difference is a space before a slash, one per empty element.
    assert!(in_the_file.starts_with("<?xml"), "the file is an XML document");
    assert!(!on_screen.contains("?>"), "and the stage is not");
    let empty_elements = body(&on_screen).matches("/>").count();
    assert!(empty_elements > 1, "there are empty elements to count");
    assert_eq!(
        on_screen.trim().len(),
        in_the_file.split_once("?>").unwrap().1.trim().len() + empty_elements,
        "one space per empty element, and nothing else",
    );
}

/// **Changing the theme does not take the code off the stage.**
///
/// It did. Picking a theme left the code — and the picture laid over it — with
/// a box 458 points wide and *nothing high*, so the code vanished and the mat
/// behind it, which is painted in the code's own background colour, was all
/// that was left on the stage. Which looks like the foreground of the code
/// being deleted, and lasts until the next keystroke builds the node again:
/// changing the theme back does not bring it back.
///
/// The cause and the fix are above `.code > .doc` in `ui.css`. What matters
/// here is that the check has to be made on the *box*, not on the document:
/// the markup was right the whole time, so [`preview`] — which every other
/// test in this file goes through — says the app is fine. Both layers are
/// checked, and the picture is an `<img>` with a `data:` URL on it, which is
/// what [`app_drawn`] is for.
///
/// Both directions, too: away from the default and back to it.
#[test]
fn the_theme_does_not_take_the_code_off_the_stage() {
    let image = an_image("theme-stage");
    let mut harness = app_drawn("https://example.org", Some(&image));

    let square = |harness: &Harness<dioxus_native::DioxusDocument>, selector: &str, when: &str| {
        let rect = harness.layout_rect(selector);
        assert!(rect.width > 1.0, "{when}: {selector} has no width");
        assert!(
            (rect.height - rect.width).abs() < 1.0,
            "{when}: {selector} is {}x{}, and the code and its inset are square",
            rect.width,
            rect.height,
        );
        rect.width
    };

    let code = square(&harness, "[data-preview] svg", "before the sheet is opened");
    let inset = square(&harness, "[data-preview-inset]", "before the sheet is opened");

    harness.click(".theme-open");
    harness.pump();
    for pick in ["dark", "light", "system"] {
        harness.click(&format!("[data-theme=\"{pick}\"]"));
        harness.pump();
        let when = format!("with {pick} picked");
        assert_eq!(square(&harness, "[data-preview] svg", &when), code);
        assert_eq!(square(&harness, "[data-preview-inset]", &when), inset);
    }
}

/// **A choice is written down, and nothing else is.**
///
/// The theme is the one thing QRnew keeps between runs, and the writing is a
/// closure handed in as a context rather than a call into `settings` — so this
/// can watch it happen without a file, and so the rest of the suite, which
/// clicks through this sheet several times, cannot edit the settings of
/// whoever runs it.
///
/// The negative half matters as much as the positive one. Opening the sheet is
/// not a decision, and a window seeded by `--theme` or by the saved value
/// itself is not somebody changing their mind; either writing back would turn
/// a screenshot flag into a preference and make the file impossible to reason
/// about.
#[test]
fn the_sheet_writes_a_choice_down_and_nothing_else() {
    let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&written);
    let seen = || written.lock().unwrap().clone();

    let vdom = VirtualDom::new(App)
        .with_root_context(Fill("https://example.org".into()))
        // Seeded, the way a saved theme or `--theme` seeds it.
        .with_root_context(Tone(Theme::Dark))
        .with_root_context(Remember(std::sync::Arc::new(move |theme| {
            sink.lock().unwrap().push(theme);
        })));
    let mut harness = Harness::from_vdom(vdom, HarnessOptions::default());
    harness.set_viewport_size(1280, 860);
    harness.pump();
    assert!(harness.query(".app.theme-dark").is_some(), "the seed took");
    assert_eq!(seen(), vec![], "a seeded window has not been asked anything");

    harness.click(".theme-open");
    harness.pump();
    assert_eq!(seen(), vec![], "and opening the sheet is not an answer");

    harness.click("[data-theme=\"light\"]");
    harness.pump();
    assert_eq!(seen(), vec![Theme::Light]);

    harness.click("[data-theme=\"system\"]");
    harness.pump();
    assert_eq!(
        seen(),
        vec![Theme::Light, Theme::System],
        "going back to the desktop is a choice like any other"
    );

    harness.click(".theme-close");
    harness.pump();
    assert_eq!(seen().len(), 2, "and closing the sheet is not one");
}

/// **The sheet opens, chooses, and closes.**
///
/// The theme is the one setting in the app that is not a control on the face
/// of the window, so the way in, the way out, and what the sheet says when it
/// arrives are worth a test of their own.
#[test]
fn the_theme_sheet_opens_chooses_and_closes() {
    let mut harness = app();
    assert!(harness.query("[data-theme]").is_none(), "it starts closed");

    harness.click(".theme-open");
    harness.pump();
    assert_eq!(
        harness.attr("[data-theme=\"system\"]", "aria-pressed").as_deref(),
        Some("true"),
        "the sheet opens on the answer in force"
    );

    harness.click("[data-theme=\"dark\"]");
    harness.pump();
    assert_eq!(
        harness.attr("[data-theme=\"dark\"]", "aria-pressed").as_deref(),
        Some("true"),
    );
    assert_eq!(
        harness.attr("[data-theme=\"system\"]", "aria-pressed").as_deref(),
        Some("false"),
        "and only one of them at a time"
    );
    // The sheet stays up: picking a theme is a thing somebody may want to do
    // twice, and the window behind it is what they are judging.
    assert!(harness.query("[data-theme]").is_some());

    harness.click(".theme-close");
    harness.pump();
    assert!(harness.query("[data-theme]").is_none());
    assert!(
        harness.query(".app.theme-dark").is_some(),
        "and the choice outlives the sheet"
    );
}
