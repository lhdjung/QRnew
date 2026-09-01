// SPDX-License-Identifier: MPL-2.0

//! QRnew, run.
//!
//! ```text
//! cargo run --release                          # the window
//! cargo run --release -- --fill "https://example.org"
//! cargo run --release -- --fill "…" --inset logo.png
//! cargo run --release -- --theme dark
//! cargo run --release -- --fill "…" --width 1019 --height 762 --quit 10
//! ```
//!
//! The size flags and `--quit` exist for one reason: a GPU renderer's
//! swapchain is sized in pixels, so comparing this build's memory against the
//! libcosmic build is only honest at the same window size. `--quit N` ends the
//! run after N seconds so the settled figure can be read off `vmmap` before
//! the process goes away. Neither flag is meant for anybody using the app —
//! and passing either of them is also what turns the maximized default off,
//! since a window the compositor sized is not a window you can compare.
//!
//! `--fill` and `--inset` are the two states the app cannot be started in from
//! the outside otherwise — one needs somebody to type, the other needs
//! somebody to work a file dialog — and both of them cost the renderer
//! something worth measuring.
//!
//! `--theme system|light|dark` is the third of those, and the cheapest: the
//! theme is behind a button and a sheet, so a window in any theme but the
//! desktop's needs somebody to click twice. It seeds what the sheet would have
//! set, which is what makes the two themes photographable.

use dioxus_native::{LogicalSize, WindowAttributes};
use qrnew::{i18n, ui};

fn main() {
    // Get the system's preferred languages, and apply the localizations.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| -> Option<f64> {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .and_then(|value| value.parse().ok())
    };
    let text = |name: &str| -> Option<String> {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };
    let fill = text("--fill").unwrap_or_default();
    let tone = text("--theme").and_then(|name| ui::Theme::named(&name));
    let inset = text("--inset");
    let measured = flag("--width").or_else(|| flag("--height"));

    if let Some(seconds) = flag("--quit") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
            std::process::exit(0);
        });
    }

    // The interface is three columns wide and wants the room: it opens
    // maximized, and the surface size below is only what the window falls back
    // to when it is un-maximized. The minimum is the width at which the two
    // 356-pixel control rails still leave the code a square worth looking at.
    //
    // No theme is asked for here, and that is the point: the window opens
    // under whatever the desktop is set to, which is what `Theme::System` in
    // `ui.rs` means and what the app defaults to. Somebody who picks Light or
    // Dark from the sheet has `ui.rs` call `set_theme` on this same window, so
    // the title bar follows them; setting one *here* would only be a fourth
    // answer nobody chose.
    let attributes = WindowAttributes::default()
        .with_title(qrnew::fl!("app-title"))
        .with_min_surface_size(LogicalSize::new(1160.0, 700.0))
        .with_surface_size(LogicalSize::new(
            flag("--width").unwrap_or(1280.0),
            flag("--height").unwrap_or(860.0),
        ))
        .with_maximized(measured.is_none());

    // A context per seed, and the inset's is left out entirely when no path
    // was given: `App` asks for it with `try_consume_context`, so a context
    // that is not there is the same as no picture.
    type Context = Box<dyn Fn() -> Box<dyn std::any::Any> + Send + Sync>;
    let mut contexts: Vec<Context> = vec![Box::new(move || {
        Box::new(ui::Fill(fill.clone())) as Box<dyn std::any::Any>
    })];
    if let Some(path) = inset {
        contexts.push(Box::new(move || {
            Box::new(ui::Inlay(path.clone())) as Box<dyn std::any::Any>
        }));
    }
    if let Some(theme) = tone {
        contexts.push(Box::new(move || {
            Box::new(ui::Tone(theme)) as Box<dyn std::any::Any>
        }));
    }

    dioxus_native::launch_cfg(ui::App, contexts, vec![Box::new(attributes)]);
}
