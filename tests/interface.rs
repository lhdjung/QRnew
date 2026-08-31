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
//! Assertions go against the *decoded* preview rather than its data URL, so
//! what is checked is the SVG the app would also save to disk.

use blitz_test_harness::{Harness, HarnessOptions};
use blitz_traits::events::BlitzImeEvent;
use blitz_traits::shell::ColorScheme;
use dioxus::prelude::VirtualDom;
use qrnew::ui::{App, Fill};

/// The preview's SVG, decoded back out of the `data:` URL on the `<img>`.
///
/// `None` when there is no code on screen, which is the placeholder state.
fn preview(harness: &Harness<dioxus_native::DioxusDocument>) -> Option<String> {
    harness.query(".preview img")?;
    let src = harness.attr(".preview img", "src")?;
    let payload = src.strip_prefix("data:image/svg+xml;base64,")?;
    Some(String::from_utf8(decode_base64(payload)).expect("the preview is a UTF-8 document"))
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
        .with_root_context(qrnew::ui::Inlay(image.to_string_lossy().into_owned()));
    let mut harness = Harness::from_vdom(vdom, HarnessOptions::default());
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

/// The picture the code is carrying, decoded back out of the document.
fn inset_of(svg: &str) -> Vec<u8> {
    let payload = svg
        .split_once("href=\"data:image/png;base64,")
        .expect("the code carries a PNG")
        .1;
    decode_base64(payload.split_once('"').unwrap().0)
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
    assert!(svg.starts_with("<?xml"), "{}", &svg[..40.min(svg.len())]);
    assert!(svg.contains("<svg "));
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
    harness.press(keyboard_types::Key::Home);
    for _ in 0.."#ffffff".len() {
        harness.press(keyboard_types::Key::Delete);
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
    assert_eq!((square.width, square.height), (310.0, 170.0));
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
    let hex = svg
        .split_once("<path fill=\"")
        .expect("the modules are painted with a fill")
        .1
        .split_once('"')
        .unwrap()
        .0;
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
    (channel(1), channel(3), channel(5))
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
    assert_eq!(svg, expected);
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
#[test]
fn no_control_is_below_the_fold() {
    for height in [860, 820] {
        for mut harness in [app(), app_with_inset("https://example.org", "fold")] {
            harness.set_viewport_size(1280, height);
            harness.pump();
            for rail in [".rail-main", ".rail-colors"] {
                let box_ = harness.layout_rect(rail);
                let last = harness.layout_rect(&format!("{rail} .card:last-child"));
                assert!(
                    last.y + last.height <= box_.y + box_.height,
                    "{rail} fits without scrolling at {height}: ends at {} against {}",
                    last.y + last.height,
                    box_.y + box_.height
                );
            }
        }
    }
}

/// The controls and the code are side by side, not stacked.
#[test]
fn the_controls_and_the_code_are_in_separate_columns() {
    let mut harness = app();
    harness.click(".field");
    harness.type_text("https://example.org");
    harness.pump();

    let colors = harness.layout_rect(".rail-colors");
    let preview = harness.layout_rect(".preview");
    assert!(
        colors.x + colors.width <= preview.x,
        "the last rail ends before the code begins: {} against {}",
        colors.x + colors.width,
        preview.x
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
    harness.press(keyboard_types::Key::Home);
    for _ in 0..4 {
        harness.press(keyboard_types::Key::Delete);
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
    harness.press(keyboard_types::Key::Home);
    for _ in 0..4 {
        harness.press(keyboard_types::Key::Delete);
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
        harness.press(keyboard_types::Key::Home);
        for _ in 0..4 {
            harness.press(keyboard_types::Key::Delete);
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
/// worked out from the bytes rather than the name, and they come out again
/// inside the document the preview and both exports are made of. The card
/// shows what was chosen, so that a picture that has landed somewhere
/// unexpected in the code can be told from one that was never read.
#[test]
fn an_inset_reaches_the_code_and_the_card() {
    let harness = app_with_inset("https://example.org", "reaches");

    let svg = preview(&harness).expect("a code is drawn with the picture in it");
    assert!(
        svg.contains("<image") && svg.contains("data:image/svg+xml;base64,"),
        "the picture is embedded in the document, declared as what it is"
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
        note.contains("inset"),
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
/// Checked on the document rather than on `shrink_logo`, which `qrnew-core`
/// tests on its own: what is worth proving here is that a picture reaching the
/// app through the one door there is comes out the other side already small.
#[test]
fn a_picture_too_large_to_carry_is_scaled_down_as_it_is_taken_in() {
    let path = a_large_image("large");
    let on_disk = png_size(&std::fs::read(&path).unwrap());
    assert!(
        on_disk.0 > qrnew_core::MAX_LOGO_SIDE,
        "the fixture has to be too large to begin with: {on_disk:?}"
    );

    let harness = app_with_picture("https://example.org", &path);
    let carried = png_size(&inset_of(&preview(&harness).unwrap()));

    assert_eq!(
        carried,
        (qrnew_core::MAX_LOGO_SIDE, qrnew_core::MAX_LOGO_SIDE)
    );

    // And the thumbnail is the same picture, not the file that was picked.
    let thumbnail = harness.attr("[data-inset-thumb]", "src").unwrap();
    assert!(
        thumbnail.starts_with("data:image/png;base64,"),
        "{thumbnail}"
    );
}

/// **Every icon is drawn twice, and the desktop's theme picks one.**
///
/// An icon's colour is a presentation attribute on an SVG that Blitz hands to
/// `usvg` as a document of its own, so no stylesheet can reach it and no media
/// query can change it. `glyph` in `ui.rs` draws the pair instead and `ui.css`
/// hides one of them, which is the whole of QRnew's light and dark support —
/// and it is support that silently becomes half-support if either side of that
/// arrangement is tidied away.
#[test]
fn one_of_each_icon_pair_is_shown_and_it_is_the_theme_that_chooses() {
    let shown = |harness: &Harness<dioxus_native::DioxusDocument>, class: &str| {
        harness
            .query_all(class)
            .into_iter()
            .filter(|node| harness.layout_rect_of(*node).width > 0.0)
            .count()
    };

    for (scheme, lit, dim) in [
        (ColorScheme::Light, 1usize, 0usize),
        (ColorScheme::Dark, 0, 1),
    ] {
        let vdom = VirtualDom::new(App).with_root_context(Fill("https://example.org".into()));
        let mut harness = Harness::from_vdom(
            vdom,
            HarnessOptions {
                color_scheme: scheme,
                ..HarnessOptions::default()
            },
        );
        harness.set_viewport_size(1280, 860);
        harness.pump();

        let pairs = harness.query_all(".lit").len();
        assert!(pairs > 10, "the window is full of icons: {pairs}");
        assert_eq!(harness.query_all(".dim").len(), pairs, "one of each");

        assert_eq!(shown(&harness, ".lit"), pairs * lit, "{scheme:?}");
        assert_eq!(shown(&harness, ".dim"), pairs * dim, "{scheme:?}");
    }
}
