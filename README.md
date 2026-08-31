# QRnew

Local-only QR code generator

Small, simple, and offline-only, so user data remain private. Create a QR code immediately, then save it as PNG or SVG. You can also copy it, or scan it directly from screen.

QRnew reads QR codes as well. Open an image file — PNG, JPEG, GIF, WebP or SVG — and its contents land in the input box, ready to be edited, restyled and saved again.

![demo](resources/qrnew-demo.png)

## Installation

Go to [Releases][releases] and download the asset for your operating system, then double-click to unpack.

> **macOS first launch:** macOS will block the app because it is not notarized. After the warning appears, open *System Settings → Privacy & Security*, scroll down to the *Security* section, and click *Open Anyway*.

## Dev build

Building from source needs the [Rust] toolchain, plus the development headers for `libxkbcommon` on Linux or the Xcode command line tools on macOS. [just] is optional, and only carries the packaging recipes.

```sh
git clone https://github.com/lhdjung/QRnew
cd QRnew
cargo run     # debug build, quickest to iterate on
just run      # release build, what a user would get
```

The QR pipeline lives in the `qrnew-core` crate and is tested on its own, without a window:

```sh
cargo test -p qrnew-core
```

Those tests render each style and read it back with a real decoder, so a change that quietly breaks scannability fails rather than merely looking odd.

The interface is tested too, and also without a window — `tests/interface.rs` builds the real component, lays it out, and clicks and types into it through upstream's headless harness:

```sh
cargo test --test interface
```

To look at the styles instead, this writes one PNG per combination:

```sh
cargo run -p qrnew-core --example gallery -- some/directory
```

Packaging is `just bundle-macos`, `just bundle-linux` or `just bundle-windows`, each run on the platform it names. The macOS and Windows recipes need ImageMagick (`magick`) on `PATH` to convert the icon.

## Acknowledgements

QRnew is built with [Dioxus] rendered by [Blitz] — HTML and CSS through Servo's style engine, painted straight onto the GPU, with no webview and no JavaScript — and the [qrcode] crate. It was inspired by [qrrs], a CLI frontend for qrcode. Earlier versions were built with [libcosmic].

[releases]: https://github.com/lhdjung/QRnew/releases
[Rust]: https://rustup.rs
[just]: https://github.com/casey/just
[Dioxus]: https://dioxuslabs.com
[Blitz]: https://github.com/DioxusLabs/blitz
[libcosmic]: https://github.com/pop-os/libcosmic
[qrcode]: https://crates.io/crates/qrcode
[qrrs]: https://github.com/Lenivaya/qrrs
