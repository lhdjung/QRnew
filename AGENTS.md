## What this is

QRnew is a local-only QR code generator. One binary, three platforms, no
network crate anywhere in the dependency list — that last part is the pitch,
so weigh any new dependency against it.

- `qrnew-core/` encodes and draws a code as one SVG, and reads one back. No GUI
  in it, and it is where the QR rules live.
- `src/ui.rs` + `src/ui.css` are the whole interface: Dioxus rendered by
  **Blitz** — Stylo, Taffy, Parley, `vello_hybrid` — so it is HTML and CSS with
  no webview and no JavaScript. Blitz is pinned to a git revision in
  `Cargo.toml`; expect gaps (no `placeholder`, no `<input type="color">`, CSS
  does not reach inside an SVG) and check `blitz-*.md` before working around
  one — the known faults and their patches are written up there.
- `tests/interface.rs` drives the interface headlessly, no window and no GPU.
  Run `cargo test --workspace` **and again with `--release`**: Dioxus schedules
  effects differently in the two, and a bug has shipped through that gap.

`dioxus-assessment.md` is the long version — why this stack, what it measured,
and every upstream fault met on the way. Read it when the answer is not obvious
from the code.

## Prose: comments, docs, interactive output
Be reasonably concice and mindful of reader's time. Focus on the essential points. Don't produce walls of text. Don't write in overly technical ways. Remember that I'm not your equally technical coworker but your boss who just wants to see results: did the feature land? Was the bug fixed? In interactive output, focus on actionable results and deemphasize *how* you got there.
