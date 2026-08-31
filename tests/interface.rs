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
    assert_eq!((square.width, square.height), (310.0, 200.0));
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
#[test]
fn no_control_is_below_the_fold() {
    let harness = app();
    for rail in [".rail-main", ".rail-colors"] {
        let box_ = harness.layout_rect(rail);
        let last = harness.layout_rect(&format!("{rail} .card:last-child"));
        assert!(
            last.y + last.height <= box_.y + box_.height,
            "{rail} fits without scrolling: ends at {} against {}",
            last.y + last.height,
            box_.y + box_.height
        );
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
/// for. The same line then reports how much has been typed, which is what lets
/// it hold its height without leaving a gap.
#[test]
fn the_field_has_a_prompt_and_then_a_count() {
    let mut harness = app();
    let prompt = harness.text_content(".note");
    assert!(
        !prompt.trim().is_empty(),
        "an empty field is prompted for something to do"
    );

    harness.click(".field");
    harness.type_text("hello");
    harness.pump();

    let counted = harness.text_content(".note");
    assert!(
        counted.contains('5'),
        "the line counts what was typed: {counted:?}"
    );
    assert!(harness.query(".note.bad").is_none());
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
