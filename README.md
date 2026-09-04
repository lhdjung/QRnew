# QRnew

Local-only QR code generator

Small, simple, and offline-only, so user data remain private. Create a QR code immediately, then save it as PNG or SVG. You can also copy it, or scan it directly from screen.

QRnew reads QR codes as well. Open an image file — PNG, JPEG, GIF, WebP or SVG — and the text inside it appears next to the button, with one click to copy it. A code drawn light on dark reads too: the standard says dark on light and most decoding libraries stop there, but every phone manages it, so QRnew tries the image the other way round before giving up.

The two colours are yours to pick, and the card says so when they get too close for a scanner to tell apart — which is easier to do than it sounds, because a scanner sees brightness and not hue. Two colours that look nothing alike can be the same colour to a camera.

The code does not have to be drawn in squares. *Shape* offers rounded modules or dots, and the three corner squares a scanner looks for soften along with them. Every one of them still scans — that is what `qrnew-core`'s tests check, with a real decoder, for each shape in turn. Scanning *quickly* is a different question, and one no test can answer: a camera has fewer clean edges to work with, so it takes a moment longer to focus and lock on. The card says so as soon as you choose anything but square, because square is what to use when the code has to be read fast and every time.

A picture can sit in the middle of a code, too. Choose one and it is drawn into the middle of the modules; error correction rises to 30% while it is there, which is what pays for the part it covers. Three sizes, and the largest is not always on offer: a picture has to stay clear of the corner squares, which sit a fixed number of modules in from each edge — so a code holding a few characters has barely room for the middle size, and one holding a web address has room for all three. The row only offers what the code in front of it can carry.

The window follows the desktop's light or dark setting, and *Appearance* in the corner overrules that when you want it to — and remembers: a code looks like a different code matted on graphite than it does on paper, and paper is usually what it ends up printed on. The dashed line around the preview is where the code ends, so the blank border that gets saved with it stays visible even when you give it the same colour as the window behind it.

A set of colours and a logo is usually a house style rather than a one-off, so *Themes* keeps them: name what is on screen, and it comes back in two clicks over any text you like. A theme is everything about the code but the text: the two colours, the picture in the middle and how big it is drawn, the margin, the shape and the error-correction level. Never the text, which is the thing you are changing when you reach for one. Two of those are preferences rather than instructions, and the card says so when either gives way — a picture has to clear the corner squares, so the large size over a very short code becomes the largest that fits, and a picture in the middle needs 30% error correction whatever the theme asked for.

Each theme is a folder in QRnew's config directory, named `qrnew-theme-…` so it still says what it is once it leaves, holding a `settings.toml` and, if it has one, the picture itself — so a theme can be written, edited, or copied to another machine by hand, and two themes can both have a `logo.png`. The file holds only what the theme changes: a key it leaves out is whatever QRnew would have done anyway, and the only one it cannot leave out is `name`. *Import theme…* takes such a folder from anywhere and files it with the rest.

Those two — the appearance and the themes — are the only things QRnew writes down anywhere.

![demo](resources/qrnew-demo.png)

## Installation

Go to [Releases][releases] and download the asset for your operating system, then double-click to unpack.

The topmost entry there is *Development build*: whatever is on `main` right now, rebuilt on every push and replaced by the next one. The numbered releases below it are the ones that stay put.

> **macOS first launch:** macOS will block the app because it is not notarized. After the warning appears, open *System Settings → Privacy & Security*, scroll down to the *Security* section, and click *Open Anyway*.

> **Windows first launch:** SmartScreen will block the app for the same reason — nobody has paid for a certificate it recognizes. Click *More info* on the warning, then *Run anyway*.

## Dev build

Building from source needs the [Rust] toolchain, plus the development headers for `fontconfig`, `libwayland` and `libxkbcommon` on Linux — `fontconfig` is linked rather than loaded at run time, so its absence stops the build rather than the app — or the Xcode command line tools on macOS. [just] is optional, and only carries the packaging recipes.

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
