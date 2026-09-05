# Hit-testing a tree that has just lost a node closes the window

Reported 2026-09-04, against `blitz` `c6dec888` (2026-08-30), from QRnew on
macOS 26.5.2. Upstream `main` at `a50cb897` has nothing that touches this.

```
thread 'main' panicked at packages/blitz-dom/src/node/node.rs:1401:37:
called `Option::unwrap()` on a `None` value
```

`panic = "abort"` in a release profile, and a panic in an event handler takes
the process either way: **the window is simply gone.**

## The chain

`1401:37` is `self.with(*child_id)` inside `Node::hit_inner`, and `with` is
`#[track_caller]`, so the reported line is the caller and the `unwrap` is
`self.tree().get(id)`. The id is a node that is no longer in the tree.

```rust
// blitz-dom/src/node/node.rs:1400
for child_id in self.paint_children.borrow().iter().flatten().rev() {
    if let Some(hit) = self.with(*child_id).hit_inner(x, y, scale, scrollbar) {
```

Three facts, and together they are the whole bug:

1. **`remove_node` and `remove_and_drop_node_with` do not touch
   `paint_children`.** They retain out of `parent.children` and mark damage
   (`mutator.rs:531`, `:558`); `paint_children` is rebuilt from
   `layout_children` in `flush_styles_to_layout_impl` (`layout/damage.rs:594`),
   which runs during `resolve`. Between the mutation and the next `resolve`,
   the parent's `paint_children` names a dropped node.
2. **Hit testing does not resolve.** `element_from_point`'s own doc says
   "`resolve` should be called before this method"; `BaseDocument::hit` is
   reached from `EventDriver` with no such call.
3. **The shell resolves only in `redraw`.** `blitz-shell/src/window.rs`
   dispatches `WindowEvent::PointerMoved` straight into `handle_ui_event`
   (`:735`); `inner.resolve(...)` happens in `redraw` (`:407`).

So a pointer event delivered after a removal and before the next redraw walks a
dangling id. Ordinary clicks survive because the redraw wins the race every
time: the mutation lands on a waker-driven wake, `request_redraw` follows it,
and `RedrawRequested` arrives before the user has moved the mouse.

## What loses the race

A modal file dialog. `rfd::AsyncFileDialog::pick_file().await`, then a signal
write that removes a node.

While the panel is up, pointer motion over the app is parked. When the panel
closes, winit delivers the future's wake **and** that whole burst in one batch:
the mutation is applied first, `request_redraw` is queued behind the batch, and
every parked `PointerMoved` then hit-tests a tree with a dropped node in it.
It is not intermittent — in QRnew, choosing a picture for the middle of the code
replaces a button with a thumbnail, and the window closes on the way back from
the dialog.

## The patch

Two lines, mirroring the `children.retain` that is already there. Removing the
id keeps `paint_children` a set of live nodes at every moment, which is what
`hit_inner` already assumes.

```diff
--- a/packages/blitz-dom/src/mutator.rs
+++ b/packages/blitz-dom/src/mutator.rs
@@ fn remove_node(&mut self, node_id: NodeId) {
             parent.insert_damage(ALL_DAMAGE);
             // Mark ancestors dirty so the style traversal visits this subtree.
             parent.mark_ancestors_dirty();
             parent.children.retain(|id| *id != node_id);
+            // Hit-testing walks `paint_children` and does not resolve first, so
+            // a dropped id here is a panic in `Node::hit_inner` on the next
+            // pointer event.
+            if let Some(painted) = parent.paint_children.borrow_mut().as_mut() {
+                painted.retain(|id| *id != node_id);
+            }
             self.maybe_record_node(parent_id);
```

and the same three lines in `remove_and_drop_node_with`, beside its own
`parent.children.retain`. `remove_and_drop_all_children` clears the list.

Belt and braces, and worth having on its own: `hit_inner` should skip an id it
cannot resolve rather than unwrap it. `self.tree().get(*child_id)` in place of
`self.with(*child_id)`, with a `continue` on `None`, turns any future version of
this into a missed hover instead of a closed window.

## What QRnew does until then

Waits 50 ms after a file dialog closes before writing anything to a signal — see
`SETTLE` in `src/ui.rs`. That hands the parked burst an unchanged tree and puts
the mutation on an empty queue, which is the situation an ordinary click is
already in. It is a delay rather than a guarantee, and it goes the day the two
lines above land.
