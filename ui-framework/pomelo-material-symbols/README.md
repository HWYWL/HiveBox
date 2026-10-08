# pomelo-material-symbols

[Google's Material Symbols](https://fonts.google.com/icons), as one Rust `Icon` per icon, for
[Pomelo OS](https://github.com/pomelos-on-sale/pomelo-os).

```rust
use pomelo_material_symbols::{self as icons, Icon};

// once, where the program is built -- the platform installs what the settings carry:
iced::application(..).font(icons::FONT)

// and then, anywhere:
text(Icon::WIFI.glyph()).font(icons::font()).size(20.0)
```

An icon is a **glyph**, so an icon is a character in the Private Use Area of a font that carries
nothing else, and drawing one is drawing text. That is not an aesthetic choice — it is the one road
the renderer already has: `pomelo-gfx` can blit RGB565 images and 8-bit alpha masks, but iced's
renderer exposes neither to an app, and `Canvas::fill_path` only strokes an outline. Glyphs, on the
other hand, are a road with traffic on it: the default font travels it, and so does every string in
every app. `src/lib.rs` has the long version.

```text
pomelo-material-symbols            the font, the pinned axes, and one `const` per icon
← this crate
        ↑   Icon · FONT · font()
apps/app-launcher · pomelo-widgets · any app that draws an icon
```

## The catalogue, and what it costs

**All of it.** Every one of the 4 299 icons upstream ships is in the subset — there is no list to
keep up to date, and using an icon nobody has used before does not need a rebake.

| | |
| :--- | ---: |
| icons | 4 299 (`const`s; 3 999 unique code points, the rest are upstream's aliases) |
| the font | 777 020 B — `.rodata`, **flash only** |
| an unused `Icon` | 0 B: a `const` that nothing references is not emitted |
| RAM | 0 B: the bytes are borrowed out of `.rodata`, not copied |

That last line is not luck. `iced_graphics::text::FontSystem::load_font` used to call
`into_owned()` on the bytes it was handed, which copied a whole font onto the heap; the fork this
ecosystem builds against drops that ([`e7d6194c1`](https://github.com/pomelos-on-sale/iced), and the
measurement behind it was this crate's font: the font database went from 1 798 122 B to 14 698 B).

What is **not** in the font is the 406 code points upstream maps that no icon has: 66 ASCII
placeholder glyphs (upstream's type-the-name ligatures need them) and 339 code points kept for
backwards compatibility with Material Icons. Nothing can name them — there is no `Icon` const for a
code point upstream has no name for — so they would be 381 KB of flash nothing could draw
(1 160 KB → 777 KB). That is what the tool's subsetting step does, and the only thing it does.

## The axes are pinned, and here

iced has no way to set `font-variation-settings` per draw, so a variable font has to be instanced on
the build machine. Upstream's axes are `FILL`, `GRAD`, `opsz` and `wght`; this is the cut:

| axis | value | why |
| --- | ---: | --- |
| `FILL` | 0 | outlined — Material Symbols' own default, and the look Google ships on the web. `--fill 1` rebakes it filled, which reads better very small |
| `GRAD` | 0 | unchanged |
| `opsz` | 24 | upstream's default, and the size these icons are drawn for |
| `wght` | 400 | regular |

Pinning an axis does **not** change the family name — the `name` table is left exactly as upstream
wrote it, so call sites keep working across a rebake. The price is that only one cut can exist at a
time; a second one would have to be renamed.

Two conveniences come from the cut: `unitsPerEm` is 960 and every icon advances exactly one em, so
`size(24.0)` draws a 24×24 icon and `.size()` is the whole of the sizing story; and it is vector, so
one subset serves 16 px and 96 px.

The layout tables (`GSUB`/`GPOS`/`GDEF`) are dropped. Upstream uses them for the type-the-name
ligatures — `wifi` becomes the wifi icon — and icons are addressed by code point here, so the
feature would only be a way for an ordinary `text(..)` to be rewritten behind its author's back.

## What is not here

* **No widgets.** `Icon` is data and `FONT` is bytes: no renderer, no platform, no `Element`. The
  things that build widgets are in `pomelo-widgets`.
* **No default font.** What text looks like when nobody asks is the platform's decision, and this
  font is never it — it registers under its own family name, so nothing draws with it by accident.
* **No `ALL`.** A slice of 4 299 icons would reference every `const` and so emit every one of them,
  and nothing iterates the catalogue: you name the icon you want.

## Regenerating

This crate is self-contained: it reads nothing out of `pomelo-os`, which consumes it as a git
dependency pinned by revision rather than as a checkout, and the only thing its tool needs is
`fontTools`. Download the upstream files into `fonts/source/` (the exact `curl`s are in
[`tools/bake_icons.py`](tools/bake_icons.py)'s header), then:

```bash
python3 tools/bake_icons.py                    # rebake the font and regenerate src/table.rs
python3 tools/bake_icons.py --check            # is what is committed what the tool would produce?
python3 tools/bake_icons.py --search battery   # which names does upstream have?
```

Commit the font, `src/table.rs` and `fonts/MANIFEST.md` together: `table.rs` carries the font's
checksum and the crate's tests recompute it, so a half-regenerated crate is a red build rather than
a panel full of the wrong pictures.

## Licence

The **crate** is GPL-3.0-only — see `LICENSE`.

The **font** is Apache-2.0 (Copyright Google LLC), which is compatible with it: the licence is
`fonts/LICENSE-APACHE-2.0.txt` and the subset's own `name` table carries the family name only, so
that file is the notice. Apache-2.0 permits subsetting, embedding and redistribution, which is what
this crate is.
