# Should QRnew become a Dioxus Native app?

Assessment written 2026-08-31, against commit `d00b41a` (branch `core-crate`).
This one answers `tauri-assessment.md`, which asked a different question and
got the answer "no, and here is why the memory instinct was aimed at the wrong
layer." The instinct was right. **Dioxus Native is the layer it was aimed at.**

The proposal is Dioxus rendered by **Blitz**: HTML and CSS parsed by Stylo,
laid out by Taffy, text shaped by Parley, painted by Vello onto the GPU. No
webview, no JavaScript, no second toolchain. Not Dioxus Desktop, which is a
webview and would buy nothing Tauri would not.

**Short version: this works, it is measured, and it is a much better fit for
QRnew than it is for the app the reference documents describe.** A spike
carrying QRnew's actual interface and its actual `qrnew-core` crate runs at
**64 MB against the current 161 MB** in the same size window at **zero** idle
CPU, and its interface is driven by four passing headless tests. The costs are
real and they are about maturity, not about capability.

---

## What was built

`qr-spike`, in this session's scratch directory: QRnew's interface — the title,
the text field, the four error-correction chips, colour swatches, the preview,
the three action buttons — as **129 lines of Rust including 20 lines of CSS**,
against the 532 lines of `src/app.rs` it stands in for. It calls `qrnew-core`
unmodified, by path dependency into this repository.

It is not a port. It answers the three questions that cannot be reasoned about:

1. Does Blitz render the SVG `qrnew-core` already emits?
2. Does the text field take text? That is the app's entire input.
3. Does it cost less than 161 MB?

All three: yes. Blitz is at `0.3.0-beta.2` / `dioxus-native 0.7.0`, from the
clone at `~/rust_projects/blitz` (`64eb2785`) that the HyloPDF experiment
already builds against.

It renders. The title, the field with its text, the chips with Medium
highlighted, the swatches, a real code drawn from `qrnew-core`'s own SVG, and
the three buttons — the whole layout, first build, no workarounds.

## The numbers

One machine, one sitting, macOS 26.5.2 on Apple silicon, release builds, three runs
each, idle with a code on screen. **Both windows are 1019×762 logical points** —
the spike's window is set to the size measured off a screenshot of QRnew,
because a swapchain is sized in pixels and the comparison is worthless
otherwise. That the two IOSurface figures come out at 24.0 MB and 24.4 MB is
the check that the matching worked.

Every memory figure is **physical footprint** — what Activity Monitor shows in
its Memory column and what the kernel charges against a limit. RSS is the wrong
unit for a GPU workload and cost the HyloPDF experiment a wrong conclusion once.

| | QRnew today | the spike | change |
| --- | ---: | ---: | --- |
| Physical footprint, settled | 160.7 MB | **63.8 MB** | **−60%** |
| Physical footprint, peak | 190.3 MB | 184.2 MB | −3% |
| Swapchain (IOSurface) | 24.0 MB | 24.4 MB | *(matched)* |
| Graphics region, resident | 47.0 MB | 14.3 MB | −70% |
| CPU over 10 s idle | 0.03 s | **0.00 s** | — |
| Binary (stripped, `opt-level = "z"`, LTO) | 8.7 MB | 7.4 MB | see below |
| Crates in `Cargo.lock` | 634 | 583 | see below |
| Helper processes | none | none | — |

Run-to-run spread was 160.7–160.8 MB and 63.6–64.0 MB. This is not a noisy
measurement.

**Read the binary and crate rows carefully.** The spike does not carry i18n,
the file reader, the exports or `open`; adding them back is about twenty crates
and some hundreds of kilobytes. Call both rows a wash. The honest claim is that
Dioxus Native costs QRnew **nothing** in binary size or dependency count, which
is itself surprising — the reference document's app pays double for the same
move, because there the trade was against pdf.js and pdfium rather than against
another Rust GUI toolkit.

### The peak does not improve, and that is worth saying

184 MB against 190 MB is not a win. What changes is not the high-water mark but
what the app *holds*: QRnew peaks at 190 MB and settles 30 MB below it; the
spike peaks at 184 MB and settles 120 MB below it. The spike's peak is hit
before its window is on screen — sampled from outside, the peak already reads
164 MB while current reads 9.7 MB — and it is the same with no code drawn at
all, so it belongs to the stack starting up and not to anything QRnew does.

Unattributed, and I did not chase it. If a memory *limit* is what matters, this
migration buys nothing. If the Memory column is what matters, it buys 97 MB.

## Why the memory lands where it does

`tauri-assessment.md` isolated 93 MB of "Metal driver" overhead in the current
build and concluded it was "not fundamental." That was the right call and the
attribution was half wrong. The HyloPDF experiment's layer-by-layer ablation,
on this machine and this Blitz commit, is the evidence:

| stage | footprint |
| --- | ---: |
| the process alone | 1.8 MB |
| + a winit window | 15.7 MB |
| + a wgpu instance, adapter and device | 16.4 MB |
| + `vello`, resumed, one empty frame | **208.0 MB** |
| + `vello_hybrid`, resumed, one empty frame | **18.8 MB** |

A window with a GPU device and a swapchain behind it costs 16 MB. Nothing in
Stylo, Parley, fontique's font enumeration or winit costs anything worth
naming. **What costs money is the renderer's scene-independent scratch, and it
costs the same whether the frame is empty or full.** `vello` allocates 173 MB
of it from constants a comment in its own source says were "hand picked to
accommodate the vello test scenes as well as paris-30k." `vello_hybrid`
allocates none, and is upstream's default.

QRnew's 47 MB resident graphics region is the same shape, and the same
conclusion follows: not the Metal driver being expensive, but a renderer
sizing its buffers for a workload QRnew does not have. `iced_wgpu` draws a few
rectangles, one SVG and some text. *Which* of its allocations account for the
47 MB I did not break down — that would need the same ablation run against
`iced` — so treat the mechanism as inferred and the two region sizes as
measured.

**This is why plain `iced` is not an alternative here.** `tauri-assessment.md`
names dropping `libcosmic` for `iced` as a smaller move that solves a different
subset, and notes it "does nothing for memory, since the renderer is the same."
Confirmed: the 47 MB is `iced_wgpu`'s, and it survives the move.

**And it is why the `tiny-skia` experiment failed the way it did.** That
experiment reached 67 MB — close to what the spike reaches — by dropping to a
software renderer, and paid 23% idle CPU for it because it repainted
continuously. Blitz reaches the same memory *and* draws nothing when nothing
changes: 0.00 seconds of CPU across ten idle seconds, measured the same way.
That combination was not on the table before. It is the whole reason this
document exists.

## Three things that fit QRnew unusually well

### 1. The SVG goes through the same library it already goes through

QRnew's preview is `widget::svg` fed by `Qr::svg()`, and `libcosmic` renders it
through `resvg`. Blitz renders an SVG image through `usvg` — which is `resvg`'s
own parser, the same crate `qrnew-core` already depends on. The generated
document arrives as a `data:` URL on an `<img>` and is parsed by
`blitz_dom::util::parse_svg_image`.

So the preview is not reimplemented, approximated, or rasterized on the way in.
It is the same bytes through the same parser. Blitz's documented SVG gap — that
CSS does not reach inside an SVG, so `stroke: currentColor` paints nothing —
does not touch QRnew at all, because `qrnew-core` already bakes every colour
into presentation attributes. `draw.rs` writes `fill` on the paths because an
exported file has to stand alone. That decision, made for a different reason,
happens to be exactly what Blitz requires.

### 2. The privacy claim survives intact, which I did not expect

`tauri-assessment.md`'s sharpest objection to a webview was that "a statically
linked Rust binary with no network crate in the tree makes that claim
self-evidently," and a webview weakens it to "true, given this CSP config."

Dioxus Native's `net` feature is **on by default** and pulls `blitz-net` →
`reqwest` → `native-tls`. That would have been the same objection in a smaller
font. But `net` is optional, and `blitz-shell` carries a feature whose own
comment reads *"Enables a data-uri-only NetProvider. Only needed if you aren't
using the regular NetProvider."* Built with `default-features = false`, `net`
off and `data-uri` on, the spike's lockfile contains **no `reqwest`, no
`hyper`, no `rustls`, no `native-tls`, no `openssl`** — and still renders the
data-URL preview, because that provider is exactly and only what a data URL
needs.

The claim stays "look at the dependency list."

### 3. `rfd` and `arboard` are already the answer

Blitz's `file-dialog` feature is `rfd`. Its `clipboard` feature is `arboard`.
Those are the two crates QRnew already uses, at the versions it already uses
them. Reading a code from a file and copying a code to the clipboard port by
moving the call, not by finding a replacement. `open` is unaffected. `i18n` is
`i18n-embed` + `rust-embed` + Fluent, which knows nothing about any GUI
toolkit; `fl!()` moves into `rsx!` and the `i18n/` directory does not change.

### And `qrnew-core` does not move at all

1,722 lines — the styled-SVG generator, the shapes, the finder-radius rules,
the logo budget, the reader, and the round-trip tests that hold all of it — are
already behind an API with no GUI in it. The spike depends on it by path and
changed nothing. This is the extraction from `tauri-assessment.md` paying off
exactly as that document said it would: *"the rewrite shrinks to swapping a UI
layer over a stable core."* It does. The UI layer is `src/app.rs`, and it is
532 lines.

## The interface works, and it is tested

The window says the layout is right and the code draws. It cannot say whether
the field takes typing — driving a real window needs accessibility permissions
this machine has not granted to `osascript`. `blitz-test-harness` answers it
without a window, a GPU or a compositor:

```
running 4 tests
test typing_generates_a_code ... ok
test error_correction_changes_the_code ... ok
test a_swatch_recolors_the_code ... ok
test high_correction_makes_a_denser_code ... ok

test result: ok. 4 passed; 0 failed; finished in 0.23s
```

Those click the field, type into it, click the chips and the swatches, and
assert on the `src` of the rendered `<img>` — that a code appears only after
typing, that High produces a different and larger code than Medium and Low, and
that a swatch recolours it. It is the full interaction model of the app, driven
end to end, in a quarter of a second.

**QRnew has no UI tests today.** `qrnew-core` is well covered and `src/app.rs`
is covered by nothing, because testing it means opening a `libcosmic` window.
This is not a consolation prize; it is a capability the current architecture
does not offer and this one does for free.

## What it costs

### There is no colour picker

Blitz has no `<input type="color">` — it has an accessibility role mapping for
one and no widget behind it. QRnew spends two `widget::ColorPickerModel`s, a
hex/RGB entry, a recents list and a copy button on this, and all of it is
`libcosmic`'s. Under Blitz it is hand-built: a swatch grid is twenty lines (the
spike has one), a hex field is a text input and a parser, and a real
saturation/value square with a hue strip is a few hundred lines of CSS
gradients and pointer maths.

This is the single largest piece of UI work in the migration and it is the one
place where the current app is unambiguously ahead.

### The COSMIC identity goes

The header bar with its `header_end` button, the context drawer that shows the
About panel, `widget::about::About`, `cosmic::theme::spacing()`, the tooltip on
the error-correction row, and COSMIC's theming — all of that is `libcosmic`'s
and none of it exists in Blitz. `app.desktop` and `app.metainfo.xml` still
install fine; the app inside them stops looking like a COSMIC app. Same cost
the Tauri option carried, for the same reason.

System dark mode is *not* on this list: Blitz maps winit's theme onto
`prefers-color-scheme`, so a CSS media query handles it. The spike simply did
not use one, which is why its screenshot is light against QRnew's dark.

### Blitz is alpha, and it is a path dependency

`dioxus-native 0.7.0` here is a clone of `main`, not a crates.io release. The
published version predates pieces the HyloPDF experiment depends on. QRnew's
spike does not use the Custom Widget API and might build against a published
release — I did not check, and it is the first thing to check before committing
to this. Until it does, "build QRnew" means "and clone Blitz beside it," which
is a real cost for a project whose README currently says `cargo build`.

The API moves underneath you. The spike hit one instance in an afternoon:
`WindowAttributes::with_inner_size` is `with_surface_size` on
`winit 0.31.0-beta.2`. That is the shape to expect — small, frequent, and paid
in the shell.

Blitz's own status page scores 48% on the WPT `css` subsuite and its
production-readiness estimate is "sometime in 2026."

### The known upstream faults, and which ones QRnew would meet

The HyloPDF experiment found four and worked around all four. Two reach QRnew:

- **A click clears the focus.** Blitz walks up from the click target looking for
  a text input, checkbox, radio, summary, label or link; a plain `<button>` is
  on none of those lists, so the focus goes to `<html>`. And the page cannot
  take it back — `MountedData::set_focus` takes `doc_mut()` from inside a borrow
  that is already held and panics with `RefCell already borrowed`. For QRnew
  this means click "High", then type, and the typing goes nowhere. The fix is
  the one that tree already uses: the element that wants the keyboard says so
  with an attribute and the shell hands it back after a click. QRnew already
  cares about this — it calls `text_input::focus` after reading a file.
- **IME does not exist.** No composition events, so CJK, Vietnamese and accent
  composition cannot be typed into the field. For a PDF reader's search box that
  is a regression for a class of readers. **For QRnew it is worse**, because the
  field is not a search box — it is the entire app. A QR code containing
  Japanese text is an ordinary thing to want, and under Blitz today it could
  only arrive by paste or by reading it out of an image. Pasting works and the
  file reader works, so it is not a wall, but it is the one honest blocker on
  this list and it has no local workaround.

The other two — hit-testing not clipping on `overflow: hidden`, and chords
leaking into a focused field as typed characters — need a scrolling container
and keyboard shortcuts respectively, and QRnew has neither.

### No packaging story

This is where Tauri genuinely beat both options and still does. `tauri build`
produces signed installers and automates notarization. Dioxus Native produces a
binary, and the hand-rolled `bundle-macos` recipe in the `justfile` with its
ad-hoc `codesign --deep --sign -` stays exactly as it is, README warning and
all. Nothing here helps.

## Against the Tauri option

For completeness, since this document answers that one.

| | Tauri | Dioxus Native |
| --- | --- | --- |
| memory vs today | a wash at best (120–200 MB across 3–4 processes) | **−60%, one process** |
| JavaScript | required | none |
| toolchains | Rust + Node | Rust |
| privacy claim | "true, given this CSP" | unchanged: no network crate |
| `qrnew-core` | called over `#[tauri::command]` | called as a function |
| Linux | WebKitGTK, the heaviest webview | same Rust binary |
| styling ecosystem | `qr-code-styling` and friends | none — but the rules are already derived and in `qrnew-core` |
| packaging | signed installers, notarization, updater | nothing |
| maturity | production, large user base | alpha turning beta |
| UI iteration | CSS and DOM | **CSS and DOM** |

The row that decides it is the last one. The strongest argument for Tauri in
`tauri-assessment.md` was frontend iteration speed for a design-heavy styling
studio — "CSS and DOM beat `iced`'s layout model." Blitz *is* CSS and DOM,
through Servo's own style engine. It delivers that argument without the
webview, without JavaScript, and without the memory being a wash.

What Tauri still wins is packaging and maturity. Those are real and they are
not nothing.

## What is not measured

- **Linux and Windows.** Nothing here has run on either. This is the same gap
  the HyloPDF experiment records, and for QRnew it matters more, because Linux
  is the platform the app currently fits best and Vello on common Linux
  hardware is untested. `vello_hybrid` splits work CPU/GPU and `vello_cpu`
  exists as a fallback, but choosing between them per machine is new work.
- **Whether a published Blitz release suffices.** The spike used a `main`
  clone because one was already on this machine.
- **Long inputs.** ~~Measured with one URL.~~ Measured — see section 6. A
  2,255-character input costs 3.3 MB more than a short URL and no CPU at all.
- **Startup time.** Not instrumented on either side.

## Recommendation

**Build it on a branch. Do not merge until Blitz ships a release that carries
what this needs.**

The case is stronger for QRnew than the reference document's case is for
HyloPDF, and for a structural reason: there, the migration is three to five
months because `viewer.ts` is 3,674 lines and the whole test apparatus is
thrown away. Here the core is already extracted, already tested, and already
emits exactly what the new renderer consumes. **The entire migration is
`src/app.rs` — 532 lines, of which the colour picker is the only hard part —
and it comes with a test suite the app does not currently have.** A week or
two, honestly, not a month.

In order:

1. **Check whether a published `dioxus-native` release works**, without the
   `main` clone. If it does, the largest practical objection disappears. If it
   does not, this stays a branch until it does.
2. **Port the interface for real**, on a branch: i18n back in, the file reader
   and the three exports wired to `rfd` and `arboard`, the About panel rebuilt
   as a plain overlay, `prefers-color-scheme` for dark mode.
3. **Build the colour picker.** Budget for this properly; it is most of the
   work and it is the one thing that comes out worse before it comes out better.
4. **Solve the focus handback once**, the way the reference tree does, with a
   test that fails the day upstream fixes it.
5. **Run the harness on Linux and Windows in CI.** `cargo test` needs no GPU
   and no screen, which makes this cheap, and it is the only thing that will
   tell you whether Stylo, Parley and fontique behave on the platform QRnew
   most cares about.
6. **Decide, with the branch in front of you.** A parked branch with its
   reasoning intact is a perfectly good outcome.

**What would kill it:** Vello unusable on ordinary Linux hardware, since that
is the platform QRnew is for. Or IME mattering more than 97 MB does — if QRnew
is meant for people typing CJK directly into the field, this migration takes
something away from them that nothing else here gives back.

**What would not kill it, and used to:** memory. That question is now settled
in the direction the original instinct pointed, and by a route neither
`tauri-assessment.md` nor its `tiny-skia` experiment found.

## Reproducing

The spike is in this session's scratch directory rather than in the repository,
since the ask was an assessment. It is worth keeping — the HyloPDF tree keeps
its equivalent at `experiments/dioxus-spike/` — and moving it in is a `cp`.

```
qr-spike/
  Cargo.toml          # dioxus-native by path; net OFF, data-uri ON
  src/ui.rs           # QRnew's interface, 129 lines including CSS
  src/main.rs         # launch, window sized to QRnew's, --quit and --fill
  tests/interface.rs  # the four tests above, via blitz-test-harness

cargo test                                        # 4 tests, 0.23s
cargo run --release -- --fill "https://…"         # the window
cargo run --release -- --fill "…" --quit 10       # …and what it cost
```

Blitz comes from the clone at `~/rust_projects/blitz` at `64eb2785`, the same
one the HyloPDF experiment builds against.

QRnew's own figures: `cargo build --release`, run `target/release/QRnew`, and
read `vmmap --summary <pid>` after ten seconds. `ps -o rss` disagrees and is
wrong; see the note on RSS in `tauri-assessment.md`.

## Sources

- `tauri-assessment.md` and `macOS-compat.md` in this repository
- `~/rust_projects/HyloPDF/experiments/dioxus-assessment.md` — the plan
- `~/rust_projects/HyloPDF/experiments/PROGRESS.md` — the measurements, and the
  source of the ablation table and every upstream fault named above
- [Blitz status: CSS](https://blitz.is/status/css),
  [elements](https://blitz.is/status/elements),
  [events](https://blitz.is/status/events)
- [the Blitz repository](https://github.com/DioxusLabs/blitz) and
  [roadmap #119](https://github.com/DioxusLabs/blitz/issues/119)
- [vello_hybrid](https://docs.rs/vello_hybrid)

---

# Addendum: the branch exists

Written 2026-08-31, same day, after building it. Branch `dioxus-native`, off
`core-crate`. Blitz at `c6dec888` (2026-08-30), one commit past the revision
this document was written against.

**The port is done and it is smaller than the estimate.** `src/app.rs` is gone;
`src/ui.rs` and `src/ui.css` stand in its place, i18n and all, with the file
reader, both exports, the clipboard, an About panel and a colour picker that
has the saturation/value square this document budgeted most of the work for.
`qrnew-core` was not touched. Fifteen interface tests run headlessly in half a
second, and QRnew had none before.

Six things this document got wrong or could not know.

## 1. IME exists now, and the blocker is gone

This was named "the one honest blocker," and it is void. `blitz-dom` has
`events/ime.rs`, which applies preedit and commit to the focused text input
through Parley; `blitz-shell` enables IME on the window and reports the cursor
area back to the compositor. `composed_text_reaches_the_field` types 日本語
into the field by composition and asserts the code that comes out is the code
`qrnew-core` makes from those three characters.

One sharp edge, and it is winit's contract rather than a fault:
`BlitzImeEvent::Commit` inserts at the selection **without clearing the
composing region first**, because winit sends an empty `Preedit` immediately
before every `Commit`. A test that leaves that line out gets "にほん日本語",
which is what the first draft of this one did.

## 2. A published release is not enough, and it does not matter

Step 1 was "check whether a published `dioxus-native` release works." It does
not, and the reason is not the one expected: `blitz-test-harness` is
`publish = false` in upstream's own manifest, so the test suite this migration
is worth having can never come from crates.io. Everything else is published as
`0.3.0-beta.2`.

**A git dependency pinned to a revision answers it completely**, and it is
better than the clone this document assumed: `cargo build` works on a fresh
checkout with nothing beside it. The clone at `~/rust_projects/blitz` the spike
used is gone from this machine, and nothing missed it.

## 3. The memory is what was measured, and the renderer is why

Same machine, same 1019×762 window, release build, three runs, a code on
screen, `vmmap --summary` at ten seconds:

| | QRnew (libcosmic) | QRnew (this branch) |
| --- | ---: | ---: |
| Physical footprint, settled | 160.7 MB | **65.5 MB** |
| Physical footprint, peak | 190.3 MB | 186.0 MB |
| Swapchain (IOSurface) | 24.0 MB | 24.4 MB |
| Graphics region, resident | 47.0 MB | 1.5 MB |
| CPU over 10 s idle | 0.03 s | **0.00 s** |
| Binary (stripped, `opt-level = "z"`, LTO) | 8.7 MB | 8.4 MB |
| Crates in `Cargo.lock` | 634 | 623 |

Run-to-run spread was 65.5 MB three times over. The libcosmic column is this
document's own, unchanged.

Two rows deserve a second look. The **graphics region** is 1.5 MB against the
spike's 14.3 MB, because this build is on `vello_hybrid` and nothing else.
And the **binary and crate rows are now a genuine win rather than a wash**:
this is the whole app — i18n, `rfd`, `arboard`, `open`, the reader, both
exports — and it is *smaller* than the libcosmic build on both counts. The
spike's caveat about twenty missing crates was paid and there was change left
over.

The peak is still unattributed and still does not improve. `net` is still off,
and `reqwest`, `hyper`, `rustls`, `native-tls` and `openssl` are all absent
from `Cargo.lock`.

## 4. The colour picker was a day, not most of the work

Square, hue strip, sixteen swatches and a hex field, in about 150 lines. The
square is four CSS background layers on one `div`: a hard-stopped
`radial-gradient` for the thumb, black upward, white rightward, the pure hue
underneath. Blitz paints layered backgrounds and linear, radial and conic
gradients, so the browser recipe works unchanged.

**The thumbs are background layers rather than child elements**, which is the
one thing worth carrying to another Blitz app. A child on top of the square is
what the pointer hits, so `element_coordinates()` would come back relative to
the thumb; `pointer-events: none` is not implemented in Blitz. A gradient has
no hit box. Two costs: a background layer is clipped to its element, so the
thumb is held a ring's width inside the box rather than overhanging the way a
browser's does; and the square's size lives twice, in `ui.rs` and in `ui.css`,
because `get_client_rect` panics from inside an event handler. A test asserts
the two agree.

## 5. The focus handback is not needed, and the one place it was is solved

`clicking_a_chip_blurs_the_field` records upstream's behaviour rather than
working around it, because for QRnew it is also what a browser does: click a
button and the field you were typing in blurs. Nobody is surprised.

The one place the libcosmic build reached for `text_input::focus` was after
reading a code out of a file, and that is answered without touching the shell:
the field carries a `key` that is bumped when the *app* changes the text, so
the element is rebuilt rather than updated, and rebuilding re-runs `autofocus`.
No `MountedData::set_focus`, no `RefCell already borrowed`, no custom
`ApplicationHandler` — which matters, because `DioxusNativeApplication` keeps
its `inner` private and wrapping it is not the small job the reference tree's
shell makes it look.

## 6. The finished app costs more than the spike, and a picture costs a lot

The table at the top measures a 129-line spike with a code on screen and
nothing else. The finished app was never measured until now. Same machine,
same method — release build, `--quit`, `vmmap --summary`, physical footprint,
three runs where the spread is quoted and two where it is not.

One difference in the method matters: **the spike was measured at 1019×762 and
the app cannot be.** `--width 1019` asks for a window narrower than the
interface's own minimum, and winit hands back 1160×762 — three columns need the
room. That is 3.4 MB more swapchain (27.8 MB of `IOSurface` against the spike's
24.4), and it is part of every figure below.

| state | settled | peak | idle CPU |
| --- | ---: | ---: | ---: |
| a URL, no picture | **73.6 MB** | 195.6 MB | 0.00 s |
| the largest input a code can hold (2,255 characters) | 76.9 MB | 201.7 MB | 0.00 s |
| a 200×200 picture in the middle | 129.1 MB | 266.7 MB | 0.00 s |
| a 512×512 picture | 144.3 MB | 281.3 MB | 0.00 s |
| a 10.7-megapixel photograph | 196.8 MB | 336.3 MB | 0.00 s |
| *(the same photograph, before the change below)* | *237.4 MB* | *376.8 MB* | *0.00 s* |

Read the first row against the two figures this document already has: **73.6 MB
against the libcosmic build's 160.7 MB.** The case holds. The spike's 63.8 MB
was optimistic by about ten megabytes, which is i18n, the file reader, the
exports, the clipboard, the About panel and a colour picker — and the wider
window. Idle CPU is still zero, at every size, and the largest code a QR
can hold costs three megabytes more than a short URL. Long inputs are no
longer unmeasured.

**The inset is the finding.** A picture in the middle of the code costs between
55 and 123 MB, and `vmmap`'s region table says where it goes:

| region, resident | no picture | a 200×200 picture | a photograph |
| --- | ---: | ---: | ---: |
| `owned unmapped (graphics)` | 17.2 MB | 83.8 MB | 83.8 MB |
| `MALLOC_LARGE (empty)` | 0.0 MB | 0.0 MB | 94.6 MB |

Two separate things, and only one of them is QRnew's.

**The 66.6 MB is `vello_hybrid`'s image atlas**, which is 4096×4096 RGBA —
67.1 MB — and is allocated whole the first time any image is drawn, at the same
size for a 200-pixel thumbnail as for a photograph. It is the same allocation
whose `TextureTooLarge` closed the window before `MAX_LOGO_SIDE` existed. There
is no way to ask for a smaller one through `dioxus-native`, so this is
upstream's number and it is a hard 67 MB on the day a person picks their first
picture. Worth saying plainly: **with an inset in place, this app is no cheaper
than the libcosmic build it replaces.** Without one — which is every code the
app has ever drawn until the inset feature landed — it is less than half.

**The 94.6 MB is the crate's own downscaling, and it came down to 54.**
`shrink_logo` decoded the photograph at its natural size (4167×2573 is 43 MB as
a pixmap) and halved it from there, while `resvg` held a full-size decode of
its own behind the same call. Those pages are freed and macOS keeps them
resident — `malloc_zone_pressure_relief` on the default zone and on all zones
returns zero and releases nothing — so the app carries them for as long as it
runs. The chain now starts at half the natural size whenever it has a halving
to spare, which is `resvg`'s 2:1 filter in place of the first box halving.
Measured on a zone plate against a true box average: **9.2 levels out of 255
for the old chain, 10.2 for the new one, 22.9 starting a quarter of the way
down, and 53.3 going straight to the target in one leap.** One level for three
quarters of the largest allocation the crate makes, and
`a_scaled_photograph_is_averaged_rather_than_sampled` holds the line at 20.

`shrink_logo` on that photograph: 98.3 MB of footprint before, 57.3 MB after.
In the app: 237.4 MB settled before, 196.8 MB after.

## 7. The theme is a class, because it could not be a media query

The assessment above lists `prefers-color-scheme` for dark mode as step two of
the port, and treats system dark mode as a thing Blitz hands you for free. That
is true exactly as far as it goes: Blitz maps winit's window theme onto the
media query and re-evaluates it live, and following the desktop was one block
of CSS.

What it does not cover is an app that wants to let somebody *overrule* the
desktop, which QRnew now does — Theme, beside About, offers System, Light and
Dark, and System is the default. That turns out to be a different problem, and
the difference is worth writing down because nothing in the docs says it:

- **Nothing a Dioxus component can call moves `prefers-color-scheme`.** The
  lever exists — `blitz_shell::View::set_theme_override` — but it belongs to
  the shell, and a component sees the document, not the view. `ShellProvider`,
  which is how a document reaches windowing functionality, has no theme
  method at all.
- **Asking winit to change the window's own theme is not a substitute, and on
  macOS it is specifically not one.** `Window::set_theme` is reachable, via
  `use_window`, and it does change the appearance. But `winit-appkit` watches
  `effectiveAppearance` and returns early — no `ThemeChanged` event — when the
  window's appearance was set by the program rather than by the desktop. So
  the title bar changes and Stylo never hears about it. Any design that
  routes an in-app theme switch through winit and back out through the media
  query is relying on an event that platform deliberately withholds.

So the palette is a class on the app's root element, and `prefers-color-scheme`
is consulted in exactly one place: inside the `.theme-system` branch, which is
the case where the desktop really is the authority and the event really does
arrive. `set_theme` is still called, purely so the title bar matches the window
under it; nothing in the interface depends on whether the platform honours it.

Three consequences fell out, and two of them are Blitz-specific enough to be
worth the next person's time:

- **The palette cannot live on `:root`.** A class is on an element the app
  renders, and the app renders inside `<main>` — it never sees `html` or
  `body`. Custom properties are inherited, so writing them on `.app` reaches
  everything *inside* `.app`, and nothing outside it: the root element's own
  background, and any overlay rendered as a sibling. Both modal sheets moved
  inside `.app` for that reason, and `.app` paints `--bg` itself rather than
  leaving it to `body`.
- **The dark palette is written twice.** It applies to a selector and to a
  media query, and CSS has no way to share one block between the two. The
  duplication is real and the fix is a test —
  `the_dark_palette_says_the_same_thing_twice` compares the copies token by
  token — because a colour edited in one and not the other is a theme that
  differs by how it was arrived at, with nothing on screen to say so.
- **Bare text inside a `<button>` does not repaint when the theme changes.**
  The surface takes the new colour and the word keeps the old one, so a
  segmented row is dark-on-dark until something else makes Blitz rebuild that
  node — clicking a segment, in practice, which corrects the row one segment at
  a time and looks exactly as odd as it sounds. Wrapping the label in a
  `<span>` fixes it; every other button in the app already had one, for its
  icon. The headless harness resolves the same tree correctly, so this is a
  renderer-only fault and no test catches it.
- **An icon still cannot be themed by CSS.** The element is handed to `usvg`
  as a document of its own, so its ink is a presentation attribute that cannot
  read a custom property or a media query. Every icon is therefore drawn twice,
  once per palette, with a rule hiding one — two nodes and two small usvg
  documents per icon on screen, one of each never painted. That is the price of
  a *runtime* theme switch specifically, and it is why the switch is spent on
  the window rather than on anything that appears in a list.

The choice is also the first thing QRnew keeps between runs. One key in one
file, in the directory the platform keeps for it, written only from the sheet's
own click — `--theme` seeds a window without writing itself back, and the
component never touches a path at all: `main.rs` hands it a closure, and the
tests, which click through the sheet repeatedly, are handed nothing. Nothing
else in the window is saved, because nothing else in the window is about the
person rather than about the code being made.

One thing the light window needed that the dark one did not, and that survives
the choice becoming three-way: **the mat under the code is painted in the
code's own background colour**, and nothing stops that being the colour of the
page behind it — `#f5f4f2` is on the palette and the hex field takes anything.
`.preview` is outlined with a dash rather than a rule, so it reads as the app's
boundary rather than as a frame somebody exported, and `mat_line` in `ui.rs`
derives the dash's colour from the mat by pushing it half a palette away from
itself. That derivation is what makes the line theme-independent: a mat light
enough to need a dark line is already lighter than a dark window, and one dark
enough to need a light line is already darker than a light one. It is the one
token with no entry in the dark palette, and
`the_mat_is_outlined_whatever_colour_the_mat_is` walks white, mid-grey and
black under both themes.

## What is still open

- **Linux and Windows — run, and one of them had something to say.** `build.yml`
  only fired on `main`, so a branch could not be checked until after it had
  been merged; it now fires on this branch and on pull requests. Its apt list
  did need widening, and in the direction nobody would have guessed from the
  error: **Parley finds fonts through `fontconfig`, and `yeslogic-fontconfig-sys`
  links it rather than dlopening it**, so the build stops in a `build.rs` with
  `pkg_config::find_library("fontconfig").unwrap()` before a line of QRnew is
  compiled. `libgtk-3-dev` came off the same list — `rfd` asks the desktop
  portal over D-Bus and there is no GTK anywhere in the tree any more.

  Windows builds and passes. Linux builds and failed one test, twice, and it
  is the one this section guessed at: whether Stylo, Parley and fontique lay
  the interface out the same way on a machine whose default font is not San
  Francisco. The argument made here was half right. **Nothing sized by CSS
  moved** — every box in the interface takes its height from an explicit
  `line-height`, unitless and inherited, so a line box is `1.5 x font-size` in
  any face. What moved was where a *sentence* wrapped: the hint at the bottom
  of the Inset card is two lines in San Francisco and three in whatever Ubuntu
  gives `system-ui`, and the colours rail had seventeen points of room for a
  twenty-two point line. `no_control_is_below_the_fold` said so from the
  runner, which is exactly what it is for.

  Fixed in `f2673d4`, and reproduced on this machine first rather than fixed
  blind: pointing the body's `font-family` at Verdana — wider than the Linux
  face and present here — fails the same test with the same numbers. That
  stand-in is worth keeping in mind as the cheapest way to ask a
  font-sensitivity question without waiting ten minutes for a runner.
- **The core's shapes are reachable now, and the rail paid for them.**
  `ModuleShape` and `FinderShape` have been in `qrnew-core` since before the
  rewrite — drawn, decoded and held by `every_combination_of_shapes_scans` —
  and no build of QRnew has ever been able to ask for either. A Shape card at
  the foot of the control rail now does, as **one** choice rather than two: the
  three that are a *look* (Square, Rounded, Dots), with the finders following
  the modules. Six independent pairings is not a question worth putting to
  somebody making one code, and rounded modules inside square finders is the
  one nobody picks on purpose.

  What it cost is the interesting part, and it is the same height budget
  `f2673d4` was about. In the wide face a Linux machine picks for `system-ui` —
  stood in for by Verdana here, which is the trick that section recommends and
  it works — the control rail had about a hundred points to spare and the card
  wanted a hundred and forty. Two points came off every card's padding, one off
  every gap inside a card, and two off the gap between them — and then the same
  again, for the caution below.

  Still not reachable, and deliberately: the finder's own two colours, and the
  logo's padding and clearing. The first is a fourth colour control in the rail
  that has none to spare, on the one part of a code that has to stay findable.
- **The shapes scan, and they do not scan *fast* — which no test here can
  say.** Reported from a phone rather than from a runner: a rounded or dotted
  code has to be held still and focused on, where a square one is read as soon
  as it is pointed at. `every_combination_of_shapes_scans` decodes all three
  with a real reader and is right to; decoding a rendered image is simply not
  the same measurement as a camera hunting for edges in a printed one. **This
  is the class of defect the test suite is structurally unable to find**, and
  the only honest answer is to write it down where the choice is made: the
  Shape card now carries the same caution banner the margin does, as soon as
  anything but square is chosen.

  The card had been left without a hint on the argument that the app keeps its
  promises by only offering answers that hold — which was a good argument about
  the promise it was actually making ("these all scan") and no argument at all
  about the one it was not ("these all scan as well as each other"). Worth
  remembering as a failure mode: a guarantee proved by tests is easy to mistake
  for the whole of what a person needs to know.

  Two cautions in one rail is what the height budget then had to answer, and
  two things paid for it. `.warn` **had never zeroed the user agent's own
  paragraph margin** — thirty points of nothing on every caution in the window,
  which is why the *margin* caution had been overflowing the rail at 820 since
  it was written with no test to say so. And the Margin card now shows its hint
  or its caution rather than both, which is what buys the room the caution
  takes. `no_control_is_below_the_fold` covers that state now, having only ever
  looked at states nobody had touched a control to reach.

  The Shape card is last, under the Margin card, which is the order the two
  read in and the order that was asked for. It is not the order the height
  budget would have picked: the shape caution is then the lower of the two in a
  column that scrolls, and it is the likelier of the two to appear. So the room
  was found instead of borrowed — two more points off every card's padding,
  three off the gap between them — and at 820 in the wide face the caution is
  on screen with the card's own bottom edge five points under the fold.
  `a_caution_is_never_the_thing_that_scrolls` is the promise that holds there.
  Both cautions at once is twenty-five points over, and that state's answer is
  the scrollbar. **Worth noting for the next control that wants this rail:
  there is no third round of this. The padding and the gaps have been shaved
  twice and the saturation square twice, and what is left to give is a card.**
- **The inset has a size now, and the ceiling on it is not a constant.**
  `Logo::size` has been in the core since before the rewrite with no way to
  ask for it. Three sizes — an eighth, the core's sixth, a quarter — and the
  interesting part is that the largest is not always available. A logo has to
  stay `FINDER_CLEARANCE` modules clear of every edge, which is a fixed number
  of *modules* and so a share of the code that grows with it: twenty-one
  modules leave barely a fifth of the width, twenty-five leave a third. A few
  characters plus a picture is a twenty-one-module code, so this is reachable
  rather than theoretical.

  `qrnew_core::largest_logo_size` is new and is what the row asks, rather than
  the app copying two rules that would then drift out of step with the ones
  enforced. A size the code cannot take is dimmed and inert, the way the
  error-correction row is while an inset holds it at 30%. The other half is a
  code that *shrinks* — text deleted out from under a size that fitted — and
  there the app redraws at the size that fits every code and marks the chip
  held. The core refuses rather than shrinking, on the grounds that only the
  caller knows which to give up; the app is the caller, and drawing nothing is
  the one answer that is certainly wrong.

  The row cost the saturation square twelve more points, on top of the thirty
  the Inset card took. That column is where the height in this window comes
  from, and the square is still the largest control in the app.

  Still not reachable, and deliberately: the logo's padding and clearing, and
  any way to refuse the raise to 30% error correction.
- **The hex field crashed the window on a keystroke — closed.** `parse_hex`
  switched on `str::len`, a count of bytes, and then sliced what it assumed
  were characters: three bytes is `abc` and it is also `aé`, and `&text[1..2]`
  inside that second one is not a character boundary. Typing an accented letter
  into the colour field panicked in an event handler, which takes the window
  with it. It reads bytes now and asks whether each one is a hex digit, which
  nothing outside ASCII is. There is a test with eight ways of writing it.
- **Escape closes a sheet, and it takes two handlers.** A modal is the one
  place in this window where the next click has to land somewhere in
  particular, and the scrim and the Close button were the only two ways out.
  What makes it two handlers is where the keyboard is when the key is pressed.
  An `onkeydown` on `.app` catches everything typed while the focus is inside
  the interface, and is the half `tests/interface.rs` can drive — the sheets
  take the keyboard when they open, which is what a modal should do anyway.
  But `clicking_a_chip_blurs_the_field` records the rule that stops that being
  enough: a click matching none of Blitz's known controls *clears* the focus to
  `<html>`, which is above `.app`, so after choosing a theme in the sheet a
  keystroke bubbles away from the app rather than through it. Upstream's
  `use_window_event` catches that one at the winit level, before any of it
  applies. **The window half cannot be tested here** — the harness has no
  window to deliver a `WindowEvent` from — so what the test holds instead is
  the fact that makes the second handler necessary, and it says so.
- **Text fields are painted in a font nobody asked for.** `create_text_editor`
  hands the `parley` editor behind every `<input>` three properties — size,
  line height, brush — and drops the rest of the computed style, so
  `font-family` on a field does nothing at all. QRnew's hex field asks for
  monospace and is painted proportional: `ffffff` measures 22.90 against
  `bbbbbb` at 48.39, where a monospace face would give one number twice.
  `blitz-fonts.md` is the report, with the eight-line patch that fixes it and
  the 52.21 / 52.21 it produces. It also carries the sting in the tail — the
  margin field's hand-rolled centring is arithmetic between Stylo's `ch` and
  what the shaper paints, and the patch that fixes the font moves the second
  of those, so that workaround has to be measured again when this lands.
- **Two copies of `usvg` — closed.** `qrnew-core` is on `resvg 0.48` and the
  whole tree shares one build of `usvg`, `tiny-skia`, `png` and `base64`. It
  cost one behaviour change, and it is a change worth knowing about: **0.45
  decoded an `<image>` while parsing and dropped the node when the bytes were
  not a picture; 0.48 believes the size in the file's header and leaves the
  decoding to the render.** Eight PNG magic bytes followed by nonsense arrive
  as an image half a billion pixels wide. `raster::natural_size` now draws the
  thing into an 8×8 thumbnail with nothing behind it and calls it a picture
  only if something lands in there, which is the one question a size cannot
  answer.
- **Backspace cannot be tested on macOS.** `blitz-dom` routes it through
  AppKit's standard key bindings, which arrive from a window; the headless
  harness cannot produce one. The hex field's test clears with Home and Delete
  and says so.
- **The COSMIC identity — closed, as far as the packaging goes.** The desktop
  entry was `Categories=COSMIC` with `Exec=QRnew`, which is not a freedesktop
  category and not the name of the binary cargo builds; the metainfo listed
  COSMIC as a category and a keyword; and the app had three IDs, one of them
  still the COSMIC template's. One ID now (`dev.lhdjung.QRnew`), one binary
  name (`qrnew`, with `QRnew` kept as the name a person sees), and the recipes
  take the source from one and the destination from the other — which is what
  `just install` and `just bundle-linux` were getting wrong in a way that only
  shows on a case-sensitive filesystem.
- **The atlas, and it is now the largest single number in the app.** 67 MB the
  moment a picture is drawn, for a picture that is at most 512 pixels on its
  longest side. `vello_hybrid` takes an `AtlasConfig`, and nothing between here
  and it — `anyrender_vello_hybrid`, `blitz-paint`, `dioxus-native` — passes one
  through. That is an upstream ask rather than a workaround, and it is the one
  change that would put an inset back inside this document's headline.
  `blitz-atlas.md` is that ask, and it is now measured at both ends: the setting
  arrives, **and the memory follows.** With the display held awake this time,
  one release binary, the flag the only difference between runs — a 256x256
  inset costs 83.5 MB of GPU dirty and 139.0 MB of footprint at the default
  atlas, and 21.7 MB and 77.4 MB at 1024. The app with a picture in it comes
  back to within a few megabytes of the app without one.

  What the measurement also settled is the number to ask for. 512 is the
  obvious one — `MAX_LOGO_SIDE` is 512 — and it is wrong: an image the size of
  the whole atlas cannot be packed into it, `AtlasLimitReached` is unwrapped
  two crates down, and the window goes away. **1024**, then, at 4.4 MB for an
  ordinary logo and 8 to 12.5 MB for one at the app's maximum, against 66.2 MB
  today. Nothing in QRnew can carry it until both halves land upstream: the
  `[patch]` sections and the `--atlas` flag that took these numbers were
  temporary and are reverted.
- **Packaging itself.** Unchanged and unhelped: `codesign --sign -` is still an
  ad-hoc signature, there is still no notarization, and the README still warns
  about the first launch.

  Three things about the release path are worth knowing before this branch
  merges, none of them blocking:

  - **Merging does not produce release assets.** `build.yml` tests and builds
    on all three platforms and uploads the bare executables as workflow
    artifacts, which expire. The `.zip` and `.tar.gz` in Releases come only
    from `release.yml`, which fires on a `v*` tag. So: merge, watch Build go
    green, then tag.
  - **The bundle's version is no longer typed in.** It was written into
    `release.yml`'s plist and into the `justfile`, both saying `0.1.0`, neither
    of them the copy `cargo` builds against. Both read `Cargo.toml` now. A tag
    still has to agree with `Cargo.toml` by hand — nothing checks that.
  - **`macos-latest` is Apple silicon**, so the macOS asset is arm64 only.
    That was already true of `v0.0.1`. A universal binary needs
    `aarch64-apple-darwin` and `x86_64-apple-darwin` built and `lipo`'d, which
    is a real change to the workflow rather than a flag.

  `build.yml` also still lists `dioxus-native` among the branches it fires on.
  That is harmless after the merge — pull requests are covered separately —
  but the comment above it stops being true, and the line can come out with
  the branch.
