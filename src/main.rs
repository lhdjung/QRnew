// SPDX-License-Identifier: MPL-2.0

//! QRnew, run.
//!
//! ```text
//! cargo run --release                          # the window
//! cargo run --release -- --fill "https://example.org"
//! cargo run --release -- --fill "…" --inset logo.png
//! cargo run --release -- --appearance dark
//! cargo run --release -- --fill "…" --width 1019 --height 762 --quit 10
//! ```
//!
//! `--fill`, `--inset` and `--appearance` seed states the app otherwise needs
//! somebody to type, click or work a file dialog to reach — they exist to make
//! a window photographable and measurable. `--appearance` does not write itself to
//! `settings`.
//!
//! `--width`/`--height`/`--quit` are for memory measurement, not for users: a
//! GPU swapchain is sized in pixels, so two builds only compare at the same
//! window size, and `--quit N` exits after N seconds so `vmmap` can read the
//! settled figure. Passing a size flag also turns the maximized default off.

use std::sync::Arc;

use dioxus_native::{LogicalSize, WindowAttributes};
use qrnew::{i18n, settings, themes, ui};

/// The key the appearance is filed under in `settings`.
const APPEARANCE: &str = "appearance";

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
    // The flag beats the file and does not become the file: a screenshot is
    // not somebody changing their mind. Only the sheet writes.
    let tone = text("--appearance")
        .and_then(|name| ui::Appearance::named(&name))
        .or_else(|| settings::read(APPEARANCE).and_then(|name| ui::Appearance::named(&name)));
    let inset = text("--inset");
    let measured = flag("--width").or_else(|| flag("--height"));

    if let Some(seconds) = flag("--quit") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
            std::process::exit(0);
        });
    }

    // Three columns wide and wanting the room: it opens maximized, and the
    // surface size below is only the un-maximized fallback. The minimum is the
    // width at which the two 356-pixel rails still leave the code a square
    // worth looking at.
    //
    // No appearance is asked for here: the window opens under whatever the desktop
    // is set to, which is `Appearance::System`. Picking Light or Dark has `ui.rs`
    // call `set_theme` on this same window.
    let attributes = WindowAttributes::default()
        .with_title(qrnew::fl!("app-title"))
        .with_min_surface_size(LogicalSize::new(1160.0, 700.0))
        .with_surface_size(LogicalSize::new(
            flag("--width").unwrap_or(1280.0),
            flag("--height").unwrap_or(860.0),
        ))
        .with_maximized(measured.is_none());

    // A context per seed. `App` asks with `try_consume_context`, so a context
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
    if let Some(appearance) = tone {
        contexts.push(Box::new(move || {
            Box::new(ui::Tone(appearance)) as Box<dyn std::any::Any>
        }));
    }
    // Same reason, for the folder of saved looks: the tests save and delete
    // themes, and a component that knew its own path would edit the themes of
    // whoever ran them. No writable home means no button.
    if let Some(dir) = themes::dir() {
        contexts.push(Box::new(move || {
            Box::new(ui::Themes(dir.clone())) as Box<dyn std::any::Any>
        }));
    }
    // Handed in rather than reached for, so the tests — which click through
    // the sheet — drive a window that cannot touch anybody's settings.
    contexts.push(Box::new(|| {
        Box::new(ui::Remember(Arc::new(|appearance: ui::Appearance| {
            settings::write(APPEARANCE, appearance.slug());
        }))) as Box<dyn std::any::Any>
    }));

    dioxus_native::launch_cfg(ui::App, contexts, vec![Box::new(attributes)]);
}
