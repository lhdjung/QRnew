# Should QRnew become a Tauri app?

Assessment written 2026-08-27, against commit `988edbf` (branch `colors`).

Short version: the two stated motivations don't survive contact with the numbers.
Memory would probably get *worse* under Tauri on macOS, and insets plus custom
module shapes don't need a webview — they need about 250 lines of SVG generation
that the current codebase is already 80% set up for. There are real reasons to
want Tauri, but they're different reasons than the ones driving the idea.

## Measured baseline

Taken on this Mac, release binary from `target/release/QRnew`, idle with an
empty input:

| Metric | Value |
| --- | --- |
| Binary size (stripped, `opt-level = "z"`, LTO) | 8.3 MB |
| RSS after warm-up | 80 MB |
| Physical footprint (what Activity Monitor reports as "Memory") | 158 MB |
| Peak physical footprint | 171 MB |
| Helper processes spawned | none |
| Crates in `Cargo.lock` | 631 |

The 415 GB virtual size that shows up in `ps` is Metal reserving address space.
It is not real memory and can be ignored.

## Claim 1: "macOS needs a bridge that eats a lot of memory"

There is no bridge process. `macOS-compat.md` describes the actual situation:
`libcosmic` is swapped from the Wayland backend to `winit`, which talks to
AppKit, and rendering goes through `wgpu` onto Metal. Both live inside the single
QRnew process. Nothing is translating or proxying anything at runtime.

The 158 MB footprint is mostly the Metal driver: shader compilation, pipeline
caches, and GPU heaps allocated by `iced_wgpu` for a window that draws a few
rectangles. That is a genuinely silly amount of machinery for this app, so the
instinct that something is wasteful is correct. The proposed fix is aimed at the
wrong layer.

**Tauri would not improve this.** On macOS, Tauri renders through `WKWebView`,
which spawns three XPC services per app: `WebKit.WebContent`,
`WebKit.Networking`, and `WebKit.GPU`. You trade one process holding a Metal
context for a host process plus a WebKit content process that has its own JS
heap, layout engine, font stack, and compositor. A Tauri hello-world on macOS
typically lands in the 120–200 MB range across those processes. That is at best
a wash against the measured 158 MB, and it is spread over more processes, which
makes it look worse in Activity Monitor, not better.

Windows is similar (WebView2, multi-process, plus a runtime dependency that has
to be present). Linux is where it clearly regresses: Tauri uses WebKitGTK, the
heaviest and least consistent of the three webviews, on the one platform where
QRnew currently runs best.

### The one-line experiment: run, and the result is mixed

`wgpu` is an optional `libcosmic` feature (`wgpu = ["iced/wgpu", "iced_wgpu"]`).
Dropping it from the feature list makes `iced` fall back to the `tiny-skia`
software renderer. I built both and measured them back to back on the same
machine, same conditions, idle with an empty input.

| Metric | `wgpu` (current) | `tiny-skia` | Change |
| --- | --- | --- | --- |
| Physical footprint | 160.5 MB | 67.0 MB | **-58%** |
| Peak footprint | 192.2 MB | 79.1 MB | **-59%** |
| Dirty (written) memory | 124.7 MB | 51.3 MB | **-59%** |
| Total resident | 466.5 MB | 391.5 MB | -16% |
| Binary size | 7.93 MB | 6.77 MB | -15% |
| Crates in lockfile | 631 | 598 | -33 |
| **Idle CPU** | **0.7%** | **23%** | **~33x worse** |

The memory result is exactly the predicted mechanism. `vmmap` on the `wgpu`
build shows a region called `owned unmapped (graphics)` at 99.3 MB total /
92 MB resident, which is the Metal driver's allocation. In the `tiny-skia` build
that region does not exist at all. Everything else in the two memory maps is
near-identical.

The CPU result kills it. Measuring accumulated CPU time across a 10-second idle
window, after startup had settled: the `wgpu` build used 0.07 s, the `tiny-skia`
build used 2.33 s. The software renderer is repainting continuously while
nothing is happening, at roughly a quarter of a core. For a utility that sits
open in the background, that is fan noise and battery drain in exchange for
memory that was never actually hurting anything.

**Verdict: don't ship this as-is.** I reverted `Cargo.toml` and `Cargo.lock`;
the working tree is back to `988edbf`. To reproduce, delete the `"wgpu"` line
from the `libcosmic` feature list and build with `--offline` rather than
`--locked`, since dropping the feature changes the lockfile.

It stays interesting for one reason: the memory the Metal path costs is now a
known, isolated 93 MB, and it is not fundamental. If the idle repaint has a
cause that can be fixed — `iced`'s `tiny-skia` renderer supports damage regions,
so continuous full repaints may be a `softbuffer` presentation issue rather than
something inherent — then a 67 MB footprint at 0.7% idle CPU is available. That
would be a better outcome than either build currently offers, and better than
anything Tauri can do here. Worth a look upstream before it's worth a rewrite.

### A note on RSS

`ps` reports RSS going *up* under `tiny-skia` (80 MB to 123 MB), which
contradicts every other measurement. Ignore it. RSS counts clean file-backed
pages from the dyld shared cache that are shared with every other process on the
system and cost nothing extra. Physical footprint is the figure Activity Monitor
shows in its Memory column and the one Apple treats as authoritative, and both
it and the dirty-memory number move the same direction as the graphics-region
evidence.

## Claim 2: "insets and custom shapes need a JS frontend"

They don't. The hard part of styled QR codes is geometry and error-correction
budgeting, not drawing primitives, and none of that gets easier in a browser.

What a logo inset and rounded modules actually require:

1. **The module matrix.** `qrcode::QrCode::to_colors()` hands you a
   `Vec<Color>` of the modules. You already depend on the crate.
2. **Custom SVG emission.** Instead of `code.render::<svg::Color>()`, walk the
   matrix yourself and emit paths: rounded rects, dots, connected blobs,
   separately styled finder patterns. This is `format!` into a `String`. Call it
   150–300 lines with zero new dependencies.
3. **PNG output.** Rasterize that SVG with `resvg`, or draw the same paths
   directly with `tiny-skia` — already in the tree either way.
4. **Logo insets.** Composite with the `image` crate, which is already a
   dependency. The real work is the rules, not the compositing: force
   `ErrorCorrection::High`, cap the occluded area at roughly 25–30% of modules,
   keep the quiet zone and all three finder patterns clear, and ideally
   round-trip the result through a decoder to verify it still scans.

Point 4 is the entire difficulty of this feature, and it is identical in Rust
and JavaScript.

The honest advantage on the JS side is that libraries like `qr-code-styling`
have already made those decisions for you. That is a real time saving. It is
also the kind of thing you can port the *rules* from without adopting the
runtime.

### The rendering split you should fix regardless

Right now QRnew has two independent QR renderers:

- Preview: `libcosmic`'s `widget::qr_code` at `cell_size(8)` (`src/app.rs:186`)
- Export: `qrcode`'s own renderer at `module_dimensions(10, 10)` (`src/app.rs:295`, `src/app.rs:329`)

They already disagree on scale and quiet-zone handling. Add shapes and insets
and you would have to implement every style twice, keep them pixel-identical,
and debug divergence between what the user sees and what they save.

The fix works in either architecture, and it points the same way: **generate the
SVG once, then display that same SVG.** `libcosmic` has `widget::svg`, which
renders through `resvg`. One styled-SVG function feeds both the preview and the
export, and WYSIWYG becomes structural rather than something you maintain by
hand. This collapses most of the "flexible QR output" argument for a webview,
because you'd be rendering SVG in the preview either way.

## What Tauri would genuinely buy you

Setting the stated reasons aside, these are real:

- **Frontend iteration speed.** CSS and DOM beat `iced`'s layout model for a
  design-heavy UI with live style previews, shape pickers, and drag-to-position
  logo placement. If the app's future is "a styling studio," this matters and it
  compounds.
- **Styling ecosystem.** `qr-code-styling` and friends solve the scannability
  heuristics you'd otherwise derive yourself.
- **Packaging and distribution.** `tauri build` produces signed `.dmg` and
  `.msi`/NSIS installers, automates notarization, and ships an updater. Compare
  the hand-rolled `bundle-macos` recipe in the `justfile` with its ad-hoc
  `codesign --deep --sign -`, and the README's "macOS will block the app"
  warning. Tauri won't buy you an Apple Developer ID, but it removes the
  scaffolding around it.
- **Non-Linux as a first-class target.** `macOS-compat.md` ends with "untested
  upstream… future upstream changes may introduce new Linux-only dependencies."
  That is an accurate description of ongoing maintenance risk. `libcosmic` does
  not owe you a macOS build, and one day it may take one away.

## What it would cost

- **The COSMIC-native identity on Linux.** The app is built from the
  `cosmic-app-template`, ships `app.desktop` and `app.metainfo.xml`, and inherits
  COSMIC theming. Under Tauri it becomes a generic webview app on the platform
  where it currently fits in best.
- **WebKitGTK on Linux.** A packaging and distro-variance headache, and the
  slowest of the three webviews.
- **WebView2 on Windows.** Preinstalled on Windows 11, an extra runtime
  dependency before that.
- **The privacy story gets harder to argue.** The pitch is "local-only,
  offline-only, so user data remain private." A statically linked Rust binary
  with no network crate in the tree makes that claim self-evidently. A webview
  is a full browser engine with a network stack, and an npm dependency tree is a
  supply-chain surface. You can lock this down with a strict CSP and no remote
  origins, and it would still be true — but "true, given this CSP config" is a
  weaker claim than "true, look at the dependency list."
- **Startup latency.** Webview init costs a few hundred milliseconds that the
  current app does not pay. Minor, but "runs fast" is in the README.
- **Two toolchains.** Rust plus Node, two build systems, `node_modules`, and a
  second language's dependency updates.
- **The rewrite itself.** 550 lines of Rust, of which `src/app.rs` is nearly all
  UI. Plus reworking i18n: `i18n-embed`/Fluent would either move to a JS i18n
  library or stay in Rust and get exposed over commands. A weekend or two,
  realistically, not a month.

## Recommendation

**Extract a core crate, and defer the shell decision.**

Pull the QR pipeline into a `qrnew-core` module or crate with roughly this shape:

```rust
fn render_svg(
    data: &str,
    ec: ErrorCorrection,
    style: &QrStyle,   // colors, module shape, finder style, logo
) -> Result<String, QrError>;

fn render_png(/* same inputs */, scale: u32) -> Result<Vec<u8>, QrError>;
```

This is worth doing on its own merits: it kills the dual-renderer split, it's
where insets and shapes have to live in either architecture, and it makes the
scannability rules testable without a GUI. It also happens to de-risk the whole
question, because a Tauri frontend would call exactly this API through
`#[tauri::command]`. Build the core now, decide the shell when you have more
information, and the rewrite shrinks to swapping a UI layer over a stable core.

Concretely, in order:

1. Extract `qrnew-core` with a styled-SVG generator. Switch the preview to
   `widget::svg` fed by the same function as the export.
2. Build insets and shapes in that core. This is where the actual difficulty is,
   and you'll learn more about the problem here than from any framework choice.
3. Separately and at whatever pace suits you, chase the `tiny-skia` idle-repaint
   issue upstream. A 67 MB footprint is sitting there behind one bug.
4. Revisit Tauri once the core exists.

**What would change this recommendation:** if the styling UI grows past what
`iced` handles comfortably (multi-panel layout, live gradient editing,
drag-positioned logos), *and* Linux stops being the priority platform, then
Tauri is a reasonable call. Note that memory has dropped off this list: the
experiment showed the 93 MB of Metal overhead is isolated and removable in
principle, which is more than a webview rewrite would have offered.

One more option worth naming: dropping `libcosmic` for plain `iced` removes the
Linux-only feature baggage and the upstream-breakage risk from
`macOS-compat.md`, while keeping the language, the binary size, and the startup
time. It costs the COSMIC desktop integration and does nothing for memory,
since the renderer is the same. It's a smaller move than Tauri and solves a
different subset of the problem.
