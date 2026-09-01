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

## What is not yet shown

**That the saving is real.** The delivery of the setting is proven; the memory
that ought to follow is not. The machine's screen locked partway through the
measurements, and a locked screen defers the GPU work — every reading taken
after that point showed a process with no window and is worthless. The numbers
in the table above were all taken with the display awake and are good; the
post-patch comparison has to be retaken.

The prediction is a 1024×1024 atlas at 4 MiB against 64, so 85.2 MB of GPU dirty
should fall to roughly 25 MB, and the 16×16 case should stop costing what the
photograph costs.

## What QRnew would ask for

512, doubled once for headroom. `qrnew_core::MAX_LOGO_SIDE` is 512 and the
inset is the only raster image the app draws — and that constant is already
written against this atlas:

> It is also a hard limit rather than a preference. A GPU renderer keeps its
> images in a texture atlas — `vello_hybrid`'s is 4096 pixels square — and an
> image that does not fit in one is not drawn small, it is refused, which in a
> renderer that unwraps the refusal means the window goes away.

Which is the other half of the argument for exposing this. An application that
knows its largest image knows the atlas it needs, in both directions: QRnew
wants a smaller one than the default, and an application that legitimately draws
something larger than 4096 has no way to ask for that either.
