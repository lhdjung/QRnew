# macOS Porting Protocol for `qrnew`

## Background

`qrnew` is built on `libcosmic`, the Pop!_OS COSMIC desktop framework. By default, libcosmic depends on Wayland as its windowing system, which does not exist on macOS. libcosmic is built on top of `iced`, which supports macOS via `winit` (a cross-platform windowing library backed by AppKit on macOS). The changes below switch the windowing backend from Wayland to winit and fix one API difference that exists between the two backends.

---

## Change 1 — `Cargo.toml`: Switch libcosmic to the winit backend

**Before:**
```toml
[dependencies.libcosmic]
git = "https://github.com/pop-os/libcosmic.git"
features = [
    "about",
    "single-instance",
    "markdown",
    "qr_code",
    "wgpu",
    "xdg-portal"
]
```

**After:**
```toml
[dependencies.libcosmic]
git = "https://github.com/pop-os/libcosmic.git"
default-features = false
features = [
    "about",
    "markdown",
    "qr_code",
    "wgpu",
    "winit",
]
```

**What changed and why:**

- `default-features = false` — libcosmic's default feature set enables `wayland`. Disabling defaults prevents `wayland-sys` and related crates from being compiled, which fail immediately on macOS because `wayland-client.pc` does not exist.
- `"winit"` added — enables the winit windowing backend, which uses AppKit on macOS. This is what gates `cosmic::app` and the `Application` trait; without it those items are compiled out entirely.
- `"single-instance"` removed — uses D-Bus (`zbus`), which is a Linux IPC mechanism not available on macOS.
- `"xdg-portal"` removed — uses XDG desktop portals, a Linux-only spec for sandboxed file/dialog access.

---

## Change 2 — `src/app.rs:338`: Fix `set_window_title` call signature

**Before:**
```rust
if let Some(id) = self.core.main_window_id() {
    self.set_window_title(window_title, id)
} else {
    Task::none()
}
```

**After:**
```rust
if self.core.main_window_id().is_some() {
    self.set_window_title(window_title)
} else {
    Task::none()
}
```

**What changed and why:**

The Wayland backend's `set_window_title` accepts `(title: String, id: window::Id)` because Wayland can manage multiple top-level surfaces identified by ID. The winit backend's signature is `(title: String)` — the window is implicit since winit manages a single primary window in this context. The `id` is simply dropped; the `is_some()` guard is kept to preserve the original intent of only setting the title when a main window exists.

---

## Limitations on macOS

- **No single-instance enforcement** — removing `single-instance` means multiple instances of the app can be launched simultaneously.
- **No portal dialogs** — removing `xdg-portal` means any file/dialog functionality that went through XDG portals will not work. On macOS this would need to be replaced with native AppKit dialogs if required.
- **Untested upstream** — libcosmic targets Linux/COSMIC and does not officially support macOS. Future upstream changes may introduce new Linux-only dependencies that require additional patching.
