# pomelo-widgets

The widgets [Pomelo OS](https://github.com/pomelos-on-sale/pomelo-os)'s apps share, in one crate.

Every widget two or more apps draw lives here, so that one table, one palette and one geometry exist
for all of them. Each widget is a module of this crate — today `touch_keyboard` — and an app depends
on the crate and names the module it wants:

```rust
use pomelo_widgets::touch_keyboard::{band, KeyAction, KeyboardMode};
```

## Why one crate rather than one per widget

Two apps drawing the same widget from two copies of its geometry is how a password keyboard drifts
two pixels away from the terminal's: nobody reports that, everybody notices it. There were two
copies before this crate existed — `apps/terminal/src/keys.rs` and a `touch-keyboard` package of its
own — so the move is recorded in `pomelo-os` (`widgets/touch-keyboard` → this crate, in `b7ccc3e`).

Keeping them in *one* crate rather than one crate each is deliberate: they are the same kind of
thing, they are versioned together, and a widget that grows a helper should be able to use another
widget's helper without a new manifest.

## What it depends on, and what it does not

iced's facade, and nothing else: no platform layer, no renderer, no board. A widget builds
`Element`s and never draws them, so which renderer draws is the graph root's business — the apps put
the switch on their own manifests (`desktop = ["iced/tiny-skia"]`) and the board's roots turn it off.

That also means **this crate must never name a renderer**, which is a real trap in the workspace it
is normally built in: there, `iced_winit` *is* `iced-pomelo-winit`, and `iced/tiny-skia` reaches
`iced_tiny_skia::window` — a module behind the `softbuffer` feature, which no edge in that graph
enables. The symptom is `E0433: cannot find window in iced_tiny_skia`.

## Licence

GPL-3.0-only — see `LICENSE`. The same licence as the rest of the crates we own (`pomelo-hal`,
`pomelo-gfx`, the renderer, the platform layer, the icon font, and the apps themselves). It said
`MIT OR Apache-2.0` until 2026-09-30, when the apps moved into a repository of their own and were
given a licence; this crate exists for those apps, so it was settled the same way rather than left
as the odd one out.

Being GPL makes it the same licence as, and compatible with, everything it is built against here.
The keyboard is not a piece anyone takes elsewhere on its own — it is the one our terminal and our
settings app must agree on — so there is nothing this choice costs.
