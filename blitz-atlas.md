# The image atlas: 64 MiB for a picture of any size

An app drawing a single 16-pixel image pays the same 64 MiB of GPU memory as one
drawing a 4096-pixel image, and there is no way to ask for less from a Blitz
app. This is the write-up behind two upstream reports — one against
[`dioxuslabs/anyrender`], one against [`DioxusLabs/blitz`] — and the evidence is
QRnew's own measurements.

Measured on macOS 15 (Apple Silicon), `vmmap --summary`, release build, window
1019×762, read after fourteen seconds.

[`dioxuslabs/anyrender`]: https://github.com/dioxuslabs/anyrender
[`DioxusLabs/blitz`]: https://github.com/DioxusLabs/blitz

## The measurement

`owned unmapped (graphics)`, dirty, is where a wgpu texture lands on this
platform. Sweeping the size of the one image the app draws:

| what is drawn                          | GPU dirty | footprint |
| -------------------------------------- | --------: | --------: |
| a QR code, no raster image anywhere     |   19.0 MB |   60.6 MB |
| the same, with a **16×16** image in it  |   85.2 MB |  127.7 MB |
| the same, with a **256×256** image      |   85.2 MB |  127.8 MB |
| the same, with a 4167×2573 photograph   |   85.2 MB |  182.0 MB |

**+66.2 MB the first time any image is drawn, and not a byte more for an image
sixty-five thousand times the area.** That is the signature of a fixed
allocation, and the size names it: 4096 × 4096 × RGBA8 = 64 MiB.

(The footprint column keeps climbing because decoding a ten-megapixel
photograph is its own cost, unrelated to this. macOS keeps those freed pages
resident — they show up as `MALLOC_LARGE (empty)`, 53.7 MB.)

## Where it comes from

`vello_hybrid` is configurable. `AtlasConfig` has an `atlas_size`, it defaults
to `(4096, 4096)`, and `initial_atlas_count: 0` means the first atlas is
allocated lazily — on the first image, which is exactly what the table shows.

The configuration does not reach it, and it fails to arrive twice.

### 1. `anyrender_vello_hybrid` builds the renderer with the defaults

`VelloHybridRendererOptions` carries a `render_settings: RenderSettings`, and
`RenderSettings::memory_settings::image_atlas_config` is the atlas. So an
application *can* say what it wants. It reaches the scene:

```rust
// window_renderer.rs, in resume()
let render_settings = self.config.render_settings;
self.scene = VelloHybridScene::new_with(width as u16, height as u16, render_settings);
```

and then, a few lines later, the renderer that actually owns the atlas texture
is built without it:

```rust
let resources = Resources::new();
let renderer = VelloHybridRenderer::new(
    render_surface.device(),
    &RenderTargetConfig { format: DEFAULT_TEXTURE_FORMAT, width, height },
);
```

`Renderer::new` is `Self::new_with(device, config, RenderSettings::default())`,
and `new_with` builds its own image cache from what it is handed:

```rust
// vello_hybrid/src/render/wgpu.rs
let image_cache = ImageCache::new_with_config(settings.memory_settings.image_atlas_config);
```

So `render_settings` is honoured for layers and dropped for images. The two-line
fix is `Resources::new_with_config(...)` and `Renderer::new_with(..., render_settings)`.

That `Renderer::new` silently substitutes defaults for a struct the caller also
holds is worth flagging to `vello_hybrid` on its own: the two constructors look
interchangeable at the call site and are not.

### 2. `dioxus-native` has nowhere to put the setting

Even with the above fixed, a Blitz application cannot reach it.
`dioxus_native::RendererOptions` is:

```rust
pub struct RendererOptions {
    pub base_color: Option<Color>,
    pub alpha_mode: Option<CompositeAlphaMode>,
    #[cfg(any(feature = "vello", feature = "vello-hybrid"))] pub features: Option<Features>,
    #[cfg(any(feature = "vello", feature = "vello-hybrid"))] pub limits: Option<Limits>,
}
```

and `with_options` builds the inner options from `anyrender::RendererConfig`,
which holds two fields — `base_color` and `composite_alpha_mode` — then applies
`features` and `limits`. `render_settings` is never mentioned, so it is always
`RenderSettings::default()`.

There is no way around it from outside. `DioxusNativeWindowRenderer`'s only
public constructors take `RendererOptions`; `with_inner_renderer` is private;
`DioxusNativeApplication` is hardcoded to `WindowConfig<DioxusNativeWindowRenderer>`
rather than generic over the renderer, so an application cannot assemble its own
and hand it in either.

The patch is on the `atlas-config` branch of the local blitz clone and is
fifteen lines: a `render_settings` field on `RendererOptions` behind
`#[cfg(feature = "vello-hybrid")]`, forwarded in `with_options`, and read in
`launch_cfg_with_props` by the same `try_read_config!` downcast that already
handles `Features` and `Limits` — so an application passes it in the config
vector it already has, and no existing signature changes.

It compiles, and the value arrives: instrumenting `Resources::new_with_config`
in a vendored `anyrender_vello_hybrid` prints

```
AtlasConfig { initial_atlas_count: 0, max_atlases: 8, atlas_size: (1024, 1024),
              auto_grow: true, allocation_strategy: FirstFit }
```

from an app that asked for 1024.

## What the patch does to the memory

**It follows.** The first attempt at this comparison was thrown away — the
machine's screen locked partway through, and a locked screen defers the GPU
work, so every reading after that point was of a process with no window. Taken
again with `caffeinate -d` holding the display awake, one release binary
throughout, the only difference between rows being a flag:

| inset drawn | atlas asked for | GPU dirty | footprint |
| --- | --- | ---: | ---: |
| none | — | 17.3 MB | 72.3, 72.1 MB |
| 16x16 | *default 4096* | 83.5 | 138.2 |
| 256x256 | *default 4096* | 83.5, 83.9 | 139.0, 139.6 |
| 512x512 | *default 4096* | 83.5 | 142.1 |
| 16x16 | **512** | 18.7 | 74.5 |
| 256x256 | **512** | 18.7, 18.7 | 74.1, 74.3 |
| 256x256 | **1024** | 21.7 | 77.4 |
| 512x512 | **512** | *panics — see below* | |
| 512x512 | **1024** | 25.2, 26.3, 29.8 | 84.3, 88.5, 87.9 |

Read as deltas over the 17.3 MB baseline, the arithmetic is the atlas and
nothing else: **+66.2 MB** for the default, which is 4096 x 4096 x RGBA8 =
64 MiB; **+1.4 MB** at 512, which is 1 MiB; **+4.4 MB** at 1024, which is
4 MiB. An application that asks for 512 pays 1.4 MB where it used to pay 66.2,
and the app's Memory column goes from 139 MB to 74 — back to within two
megabytes of never having drawn a picture at all.

Two notes on reading the table. The absolute footprints are higher than the
first table in this document because that sitting had a 24.0 MB swapchain and
this one has 27.8 MB; the deltas are what carry across, and the +66.2 MB
reproduces exactly. And the 512x512 default row is the one reading that had to
be assembled rather than read: by then the machine was paging, so `dirty` shows
96 KB with 83.4 MB `swapped` beside it, and 83.5 MB is the sum. Two runs of
that configuration agree on it. A third was discarded for having a 14.0 MB
swapchain instead of 27.8 — a different window is not the same measurement,
which is the rule the first table in this document was built on.

### The 512x512 row that is not a number

Asking for a 512 atlas and then drawing a 512-pixel image does not draw it
small. It ends the app:

    thread 'main' panicked at vello_hybrid-0.1.0/src/render/wgpu.rs:544:
    called `Result::unwrap()` on an `Err` value: AtlasLimitReached

`auto_grow` is on and `max_atlases` is 8, and neither helps: an image the size
of the whole atlas never fits in one, so growing produces more atlases it also
does not fit in until the limit is reached, and the refusal is unwrapped. That
is a second thing worth fixing upstream and a separate report — an atlas
allocation that cannot succeed should be a drawing that does not appear, not a
process that goes away — but it is also the shape of the advice below.

## What QRnew would ask for

**1024, and the doubling is not headroom — it is the difference between the app
working and not.** `qrnew_core::MAX_LOGO_SIDE` is 512, so 512 looks like the
exact fit, and the row above is what an exact fit does: an image the size of
the atlas cannot be packed into it, and the app is gone. 1024 costs 4.4 MB for
an ordinary logo and 8 to 12.5 MB for one at the app's own maximum, where the
allocator takes two or three atlases and which it takes varies between runs.
Against 66.2 MB either way.

The inset is the only raster image the app draws, and the constant that bounds
it is already written against this atlas:

> It is also a hard limit rather than a preference. A GPU renderer keeps its
> images in a texture atlas — `vello_hybrid`'s is 4096 pixels square — and an
> image that does not fit in one is not drawn small, it is refused, which in a
> renderer that unwraps the refusal means the window goes away.

Which is the other half of the argument for exposing this. An application that
knows its largest image knows the atlas it needs, in both directions: QRnew
wants a smaller one than the default, and an application that legitimately draws
something larger than 4096 has no way to ask for that either.
