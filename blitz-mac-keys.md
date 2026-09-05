# macOS text input: three findings

Against `c6dec888`, on macOS 15 (Apple Silicon), measured by driving a real
window and reading the field back. Working notes; not filed upstream.

## 1. The text input client is never enabled

`Node::focus` calls `ShellProvider::set_ime_enabled(true)`, but with a text
input focused `Window::ime_capabilities()` is still `None`. So `key_down`
skips `interpretKeyEvents:`, no `doCommandBySelector:` fires, and
`apply_apple_standard_keybinding` — which implements all of these — is never
reached. On `hello world`:

| pressed | expected | observed |
| --- | --- | --- |
| Backspace ×3 | `hello wor` | `hello world` |
| Option+Left | back a word | back a character |
| Ctrl+A | start of line | nothing |

Backspace matters most: `apply_keypress_event` omits it on macOS by design, so
deleting backwards is unreachable entirely.

No patch — the request is made and not honoured, and I did not isolate which
end drops it. An embedder can ask for the client itself, once, which is what
`open_the_text_input_client` in `ui.rs` does.

## 2. Both handlers act, and Cmd gets the wrong binding

With the client on, `winit` delivers the key event as well as the command, so
one press of Left moves the caret twice. Cmd is the exception:
`NSTextInputContext` does not interpret Command chords, so no binding is sent
and `apply_keypress_event` is the only handler — and it reads Cmd as
`ACTION_MOD` and jumps by word. On a Mac, Cmd and an arrow is the start or end
of the line (`StandardKeyBinding.dict`: `@\UF702` is `moveToLeftEndOfLine:`).

`blitz-mac-keys.patch` fixes both. Applies cleanly; `cargo check -p blitz-dom`
passes. With it, `appkit_has_this_key` in `ui.rs` can go.

## 3. Selection colour is a constant

`SELECTION_COLOR` in `blitz-paint/src/lib.rs` is `rgb(180, 213, 255)`, painted
under text drawn in the element's own `color`, with no `::selection`. Any dark
theme gets near-white ink on pale blue — QRnew's dark window cannot show a
readable selection. Fix: honour `::selection`, or derive the selected text
colour from the selection colour.
