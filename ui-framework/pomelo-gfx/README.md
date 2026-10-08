# pomelo-gfx

A hand-written, dependency-free 2D rasterizer that draws **straight into an RGB565
frame buffer** — the native format of the 16-bit panels this targets (a 480×480 CO5300
AMOLED, driven by an ESP32-S3).

It is **not** a port of Skia, and has no connection to it. The drawing API is *shaped*
after Skia's — `Canvas`, `Paint`, `Path`, `RRect`, `Transform` — because that shape is
familiar, and because it kept the call sites in the framework that uses it small. That
is a design debt repaid in familiarity, not a claim of compatibility.

## Why RGB565 rather than RGBA8888

Because the panel is 16 bits per pixel. A general-purpose RGBA8888 rasterizer has to
draw into a 4-byte-per-pixel surface and then convert the finished frame down to the
panel's format: a second full-size buffer, plus a whole extra pass over the frame every
time anything changes.

Drawing in the panel's own format removes both. On the ESP32-S3 (240 MHz Xtensa LX7,
PSRAM), the framework's own in-tree profiling measured roughly **0.12 µs per pixel** for
a plain rect fill here, against **4.4 µs per pixel** for a general RGBA pipeline's
`draw_pixmap` on the same core — the same order of difference you would expect, since
per-pixel work in a float, premultiplied-RGBA pipeline costs several times an integer
write to the destination format, and at 240 MHz that dominates everything else.

Those are indicative figures from one device session, not a benchmark suite.

## Status

Two crates in Pomelo OS draw through it:

* `iced-pomelo-gfx` — iced's renderer contract, over this rasteriser: the same position in the
  stack as `iced_tiny_skia`
* `iced-pomelo-winit` — the platform layer that runs [iced](https://github.com/iced-rs/iced) on
  this panel (for the frame buffer itself)

**No antialiasing for paths yet**: filled rects, rounded rects and paths are hard-edged.
Coverage-mask blits *are* antialiased, because `Canvas::blit_mask` blends an 8-bit mask —
which is how text and icons get their smooth edges.

## Layout

| file | |
|---|---|
| `src/canvas.rs` | the drawing entry point: clip and transform stacks, filled rects, rounded rects, circles, blits |
| `src/raster.rs` | the scanline rasterizer: path filling, stroking, dithering |
| `src/pixmap.rs` | `Pixmap565` / `Pixmap565Mut`, the frame buffer and its views |
| `src/paint.rs` | paints, strokes, gradient shaders |
| `src/path.rs` | path building and flattening |
| `src/geometry.rs` | points, rects, rounded rects, transforms |
| `src/color.rs` | RGB565 ↔ ARGB conversions, blending, rectangular extraction |

## Provenance

Extracted from [pomelo-os](https://github.com/pomelos-on-sale/pomelo-os) with
`git subtree split`, so the history here is this crate's own. It was added to that
repository at `crates/tiny-flutter/crates/tiny_gfx`; this repository continues from
there unchanged.

## License

GPL-3.0-only — see [`LICENSE`](LICENSE).
