# QRnew

**Local-only QR code generator**

Simple, flexible, and offline-only, so user data remain private. Create a QR code immediately, then save it as PNG or SVG.

Also supported:
-   Customize colors and shapes
-   Insert image in the center
-   Save theme for later: colors plus image
-   Read text from QR codes
-   Dark mode editor

![demo](resources/qrnew-demo.png)

## Installation

Go to [Releases][releases] and download the most recent asset for your system, then double-click to unpack.

> **macOS first launch:** macOS will block the app because it is not notarized. After the warning appears, open *System Settings → Privacy & Security*, scroll down to the *Security* section, and click *Open Anyway*.

> **Windows first launch:** SmartScreen will block the app. Click *More info* on the warning, then *Run anyway*.

## Dev build

Building from source needs the [Rust] toolchain, plus the development headers for `fontconfig`, `libwayland` and `libxkbcommon` on Linux or the Xcode CLI tools on macOS. [just] is optional and only for the packaging recipes.

```sh
git clone https://github.com/lhdjung/QRnew
cd QRnew
cargo run     # debug build, quickest to iterate on
just run      # release build, what a user would get
```

Packaging is `just bundle-macos`, `just bundle-linux` or `just bundle-windows`. The macOS and Windows recipes need ImageMagick (`magick`) on `PATH` to convert the icon.

## AI usage

While the code was written by Claude Opus 5.0, most design decisions were made by me. I tested the app continuously throughout development and raised many issues with it.

## Acknowledgements

QRnew is built with [Dioxus] and [Blitz], a HTML and CSS renderer in pure Rust. It is based on the [qrcode] crate and was inspired by [qrrs], a CLI frontend for qrcode.

[releases]: https://github.com/lhdjung/QRnew/releases
[Rust]: https://rustup.rs
[just]: https://github.com/casey/just
[Dioxus]: https://dioxuslabs.com
[Blitz]: https://github.com/DioxusLabs/blitz
[qrcode]: https://crates.io/crates/qrcode
[qrrs]: https://github.com/Lenivaya/qrrs
