// SPDX-License-Identifier: MPL-2.0

//! QRnew, run.
//!
//! ```text
//! cargo run --release                          # the window
//! cargo run --release -- --fill "https://example.org"
//! cargo run --release -- --fill "…" --width 1019 --height 762 --quit 10
//! ```
//!
//! The size flags and `--quit` exist for one reason: a GPU renderer's
//! swapchain is sized in pixels, so comparing this build's memory against the
//! libcosmic build is only honest at the same window size. `--quit N` ends the
//! run after N seconds so the settled figure can be read off `vmmap` before
//! the process goes away. Neither flag is meant for anybody using the app.

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
    let fill = args
        .iter()
        .position(|arg| arg == "--fill")
        .and_then(|at| args.get(at + 1))
        .cloned()
        .unwrap_or_default();
    let width = flag("--width").unwrap_or(900.0);
    let height = flag("--height").unwrap_or(720.0);

    if let Some(seconds) = flag("--quit") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
            std::process::exit(0);
        });
    }

    let attributes = WindowAttributes::default()
        .with_title(qrnew::fl!("app-title"))
        .with_surface_size(LogicalSize::new(width, height));

    dioxus_native::launch_cfg(
        ui::App,
        vec![Box::new(move || {
            Box::new(ui::Fill(fill.clone())) as Box<dyn std::any::Any>
        })],
        vec![Box::new(attributes)],
    );
}
