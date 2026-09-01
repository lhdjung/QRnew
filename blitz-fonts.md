# A text field is painted in a font nobody asked for

`font-family` on an `<input>` does nothing in Blitz. Nor does `font-weight`,
`font-style`, `letter-spacing` or anything else the CSS says about the type in
a field: the editor that paints it is handed a size, a line height and a
colour, and the rest of the computed style is dropped on the floor. This is the
write-up behind an upstream report against [`DioxusLabs/blitz`], and the
evidence is QRnew's own hex field, which asks for monospace and does not get it.

Measured on macOS 15 (Apple Silicon) through `blitz-test-harness`, against
`c6dec888`, the revision QRnew pins.

[`DioxusLabs/blitz`]: https://github.com/DioxusLabs/blitz

## The measurement

QRnew's colour picker has a hex field, and `ui.css` asks for monospace in it:

```css
.hex {
  font-family: ui-monospace, "SF Mono", "Cascadia Mono", monospace;
  font-size: 14.5px;
}
```

Type `ffffff` into it, then `bbbbbb`, and read the width straight off the
`parley` layout that paints the glyphs. In a monospace face those two strings
are the same width by definition — six advances of the same size. The number
beside each is the app's own prose field, `.field`, which asks for nothing and
should be proportional:

| | `ffffff` | `bbbbbb` | |
| --- | ---: | ---: | --- |
| `.hex`, as shipped | 22.90 | 48.39 | **2.1x — proportional** |
| `.hex`, with the patch below | 52.21 | 52.21 | monospace |
| `.field`, either way | ~25 | ~49 | proportional, as asked |

The field is not monospace, it is not the font the stylesheet names, and
nothing anywhere reports that the declaration was ignored.

## Where it comes from

`create_text_editor` in `blitz-dom/src/layout/construct.rs` builds the
`parley::PlainEditor` behind every `<input>` and `<textarea>`. It computes the
full style first — `stylo_to_parley::style` returns a `TextStyle` carrying the
resolved family list, weight, width, slant, variations, features, letter and
word spacing — and then keeps three of them:

```rust
let styles = editor.edit_styles();
styles.retain(|_| false);
styles.insert(StyleProperty::FontSize(parley_style.font_size));
styles.insert(StyleProperty::LineHeight(parley_style.line_height));
styles.insert(StyleProperty::Brush(parley_style.brush));
```

`retain(|_| false)` empties the set, three properties go back in, and every
other property returns to `parley`'s default. The work of resolving the family
has already been done one line above; it is simply not passed on.

The patch is eight lines, all of them the same line:

```rust
styles.insert(StyleProperty::FontFamily(parley_style.font_family));
styles.insert(StyleProperty::FontSize(parley_style.font_size));
styles.insert(StyleProperty::FontWidth(parley_style.font_width));
styles.insert(StyleProperty::FontStyle(parley_style.font_style));
styles.insert(StyleProperty::FontWeight(parley_style.font_weight));
styles.insert(StyleProperty::FontVariations(parley_style.font_variations));
styles.insert(StyleProperty::FontFeatures(parley_style.font_features));
styles.insert(StyleProperty::LineHeight(parley_style.line_height));
styles.insert(StyleProperty::WordSpacing(parley_style.word_spacing));
styles.insert(StyleProperty::LetterSpacing(parley_style.letter_spacing));
styles.insert(StyleProperty::Brush(parley_style.brush));
```

Every value is already in hand and every one is `'static` — the family list,
the variations and the features are `Cow::Owned` — so `StyleSet` takes them
without a lifetime anywhere. The table above is that patch measured, and
QRnew's forty-one interface tests pass with it in place bar one, which is the
second half of this document.

## And a second answer to "how wide is a digit"

QRnew centres the number in its margin field by hand, because Blitz ignores
`text-align` inside an `<input>` for the same reason it ignores `font-family`:
the glyphs belong to an editor with no box to align in. The workaround is
`padding-left: calc(30px - 1ch)` — half the field, less half the text, in the
field's own digit width — and a test measures the painted result against the
box so the arithmetic cannot drift.

That test is how a third font turned up. `.count` is 17px of
`system-ui, -apple-system, …`, and there are three answers to how wide `0` is
in it:

| | digit advance | |
| --- | ---: | --- |
| Stylo's `1ch`, which the app does its arithmetic in | 10.000 | 0.588 em |
| what `parley` paints today | 9.455 | 0.556 em |
| what `parley` paints with the patch | 8.583 | 0.505 em |

The middle row is not the CSS font — 0.556 em is Helvetica's digit, not San
Francisco's — which is the bug above, seen from the other side. The top row is
neither, so `zero_advance_measure` and the shaper do not agree even once they
are pointed at the same stack.

The app has been living on the gap between the first two rows being small: the
number sits about half a point left of centre, and the test's 1.5-point
tolerance covers it. With the patch the gap becomes 2.8 points and the test
fails — **which is the patch being right and the workaround being calibrated
against the bug.** Anybody centring text in a field the way QRnew does is
calibrated the same way, and will have to recalibrate when this lands, so it is
worth saying out loud in the change rather than leaving it to be discovered.

The honest fix for the second row is the patch. The honest fix for the third is
`text-align` on an input actually working, which is a larger ask and not this
one.

## What QRnew does meanwhile

Nothing, and the stylesheet keeps asking. `.hex` still names its monospace
stack: the declaration is correct CSS and the renderer is what is wrong, so
deleting it would be writing the bug into the app. The comments in `ui.css` and
`ui.rs` that describe the editor as being "handed a font and nothing else" have
been corrected to say what it is actually handed, with the numbers above
beside them.
