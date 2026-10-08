# Pomelo changelog

What this fork changes relative to upstream `iced-rs/iced`.

`pomelo-os` runs iced on an ESP32-S3: Xtensa LX7, 32-bit, ESP-IDF, no window
system and no GPU. iced does not support that target out of the box, and the
nine commits below are the whole of what it took.
They live on the `pomelo/esp32s3` branch; `0.14` (the upstream branch) stays
clean, so following upstream is one `git rebase` and conflicts can only land in
the twelve files listed here.

**Baseline:** upstream `0.14` at `38237dd2` -- *Fix event loop spin on stale
redraw deadlines in `winit` shell*, one commit past the `0.14.1` release.

**Delta:** 9 commits, 12 files, +218 / -37 -- of which `Cargo.lock` accounts
for +86 / -17. The changelog commit that adds this file is not counted, nor are
the three that extend it.

| Commit | Change | Why |
| --- | --- | --- |
| `5b365a543` | id counters use `AtomicUsize` | the target has no 64-bit atomics |
| `7bba303c6` | `softbuffer` is optional in `iced_tiny_skia` | softbuffer cannot build for ESP-IDF |
| `6721ee56e` | new `custom` feature on `iced_renderer` | a platform that brings its own renderer cannot select one of iced's |
| `16f920205` | `custom` names a renderer instead of skipping one | the unit renderer has no impls in release |
| `53d2cd633` | `rustfmt` the `cfg` attributes above | -- |
| `e4fa2373d` | new `pomelo` feature on `iced_renderer` | the renderer an app names has to be the one that draws it |
| `ce26bceb9` | `x11`/`wayland` stop implying softbuffer | the facade demands one of them, and this target is `unix` with neither |
| `7cee9d5b5` | `Instance::state()` | the platform holds the state here, and has to report on it |
| `e7d6194c1` | `load_font` stops copying its argument | the `Arc` it goes into never needed to own the bytes |

Nothing here changes what is drawn. Every change but one is behind a target
capability, behind a new feature, or in the path only a platform without a
window system takes. The exception is the font copy (`e7d6194c1` below): it is
not conditional on anything, because the copy it removes was never a decision --
a desktop program that installs a font paid it too.

## Pointer-width atomics for the id counters

`5b365a543` · `core/src/window/id.rs`, `core/src/image.rs`,
`graphics/src/cache.rs`, `graphics/src/mesh.rs`

Xtensa does not have 64-bit atomics:

```
$ rustc +esp --print cfg --target xtensa-esp32s3-espidf | grep target_has_atomic
target_has_atomic="8"
target_has_atomic="16"
target_has_atomic="32"
target_has_atomic="ptr"
```

`AtomicU64` is therefore not a type that exists there, and `iced_core` and
`iced_graphics` do not compile at all -- std has no runtime fallback of the
libatomic kind. This is an architecture limit rather than a codegen backend
problem: `riscv32imac-esp-espidf` is identical.

All four counters exist to hand out a unique id, so they are `AtomicUsize` now:
the widest *native* atomic the target has. On a target with 64-bit atomics that
is the same 64 bits as before; it narrows only where nothing wider exists. The
counter is widened back to `u64` at its single construction site, so the public
`Id(u64)` types and their `serde` representations are untouched.

(The widening is written `usize as u64` rather than `u64::from`: `core` has no
`From<usize>` for `u64`, only widenings between fixed-width types.)

## The window compositor is optional

`7bba303c6` · `tiny_skia/Cargo.toml`, `tiny_skia/src/lib.rs`

`iced_tiny_skia` depends on `softbuffer` unconditionally, and softbuffer has no
ESP-IDF backend -- its "unsupported platform" fallback does not even compile.

`softbuffer` is an optional dependency behind a `softbuffer` feature now, and
that feature gates `pub mod window`, the `compositor::Default` and
`renderer::Headless` impls, and the `Size` and `compositor` imports those two
are the only users of. `x11` and `wayland` still enable it, so desktop builds
are unchanged.

What remains without it is a pure *renderer*: the platform owns the pixel buffer,
calls `Renderer::draw`, and presents the damaged regions itself -- which is
exactly what `pomelo-iced-backend` does.

## A way for an integrator to bring its own renderer

`6721ee56e`, `16f920205`, `53d2cd633` · `renderer/Cargo.toml`,
`renderer/src/lib.rs`

`iced_renderer` refuses to build in release mode when neither `wgpu` nor
`tiny-skia` is selected, because `Renderer` would be `()`. The guard is right: a
release image whose renderer is a unit type is a mistake worth failing on.

It is a mistake for the wrong reason in one case, though -- an integrator that
supplies the renderer itself and never names these aliases at all, which is what
an embedded platform layer does. `iced_widget` depends on this crate only to
forward features, so such a platform cannot avoid compiling it.

So the guard gains an explicit way out rather than losing its teeth: the new
`custom` feature says "not one of yours", and everyone who has not asked for it
still gets the error.

Silencing the `compile_error!` is not enough to make that configuration build,
which is what `16f920205` corrects. The unit renderer is a
`debug_assertions`-only stub -- `core/src/renderer/null.rs` implements the
renderer traits for `()` and nothing does in release -- so `iced_widget`, whose
widgets default their `Renderer` to this crate's alias, failed with 25
trait-bound errors instead. `custom` now names `iced_tiny_skia`'s renderer and
still supplies no compositor, because the integrator has one. That is the real
reason the guard exists, and the comment on it says so now.

`53d2cd633` is `rustfmt` on the `cfg` attributes the other two added.

## A renderer of your own

`e4fa2373d` · `Cargo.toml`, `renderer/Cargo.toml`, `renderer/src/lib.rs`

`custom` says the integrator brings a surface and a compositor, but it still
leaves the *renderer* to `iced_tiny_skia`, and that turned out to be the end of
the story an app could accept. An app names its renderer exactly once --
`type Renderer = iced::Renderer`, which the facade re-exports from this crate --
so with `custom` it writes its whole widget tree against a renderer nobody
draws with. On a board where the platform layer *is* the renderer, that is the
one name that cannot be left at the default.

The `pomelo` feature names `iced-pomelo-gfx` instead. It wins over `custom`
when both are on, which is the combination a real build has: `custom` says "the
surface is ours", `pomelo` says "and the renderer is". `iced_renderer`'s own
`Compositor` alias stays `()` in both cases -- when the surface belongs to
someone else, neither this crate nor the facade ever instantiates it.

It arrives as a **git dependency**, not a path: the renderer is a repository of
its own (`pomelos-on-sale/iced-pomelo-gfx`), and this fork has no business
knowing where `pomelo-os` checks it out. `pomelo-os` patches that URL back to
its submodule, so a build there uses the checkout that is open and local edits
are live; the `Cargo.lock` here pins a revision for anyone building this fork on
its own.

One thing to know about patching a git source: Cargo fetches it anyway. It has
to see what the source contains before it can substitute anything for it, so
the first resolve on a machine needs access to that repository even though the
compiled code will be the local path. `pomelo-os` sets
`net.git-fetch-with-cli = true` for this and explains the rest in
`vendor/README.md`.

## A display-server feature, on a board with no display server

`ce26bceb9` · `tiny_skia/Cargo.toml`

iced's facade refuses to compile on unix when neither `x11` nor `wayland` is
enabled:

```rust
#[cfg(all(target_family = "unix", not(target_os = "macos"), not(feature = "wayland"), not(feature = "x11")))]
compile_error!("No Unix display server backend has been enabled. ...");
```

An ESP32-S3 is `unix` (`target_family = "unix"`, `target_vendor =
"espressif"`), so a program written for iced has to name one of them to build
here at all -- even though the board has no display server, no window, and
nothing to put in one. That is already odd; what made it impossible is that
naming it *pulled `softbuffer` in*, because `x11 = ["softbuffer/x11", ...]`
enables an optional dependency rather than forwarding a feature to it, and
softbuffer has no ESP-IDF backend.

The end of that road was a second patch, to the guard itself, saying "not on this
OS". `softbuffer?/x11` is better, and is the same class of fix as `7bba303c6`:
the feature is forwarded when the dependency is there, and means nothing when it
is not. `softbuffer` moves into `default = ["x11", "wayland", "softbuffer"]`
so that a desktop build keeps exactly what it had, and an app can now write
`iced = { default-features = false, features = ["thread-pool", "x11"] }` and
have the whole display-server half of the graph be inert on this board.

## Reading the state of a hosted program

`7cee9d5b5` · `program/src/lib.rs`

`Program` is opaque to the platform by design: a program is driven through
`view`, `update` and `subscription`, and nothing outside it is supposed to know
what its state is. iced's own loop never needs to.

Here the loop *is* the platform -- one panel, a status bar the firmware fills
in, test suites that have to assert what an app did -- and a host that cannot
see the state it hosts cannot report on it. `Instance::state()` is one read-only
accessor; nothing else about the contract moves, and no desktop code path uses
it.

## A borrowed font stays borrowed

`e7d6194c1` · `graphics/src/text.rs`

`FontSystem::load_font` takes a `Cow<'static, [u8]>` and called
`into_owned()` on it before handing the bytes to `fontdb`. For an
`include_bytes!` face -- the normal case for a program that carries its own
font, and the only case on this board -- that allocates a copy of the whole
font on the heap, and `fontdb` keeps the `Arc` for the life of the process.

Nothing required it. `Source::Binary` holds an `Arc<dyn AsRef<[u8]>>` and only
ever borrows from it, and `font_system()` in the same file has always passed
the two built-in faces as slices:

```rust
cosmic_text::fontdb::Source::Binary(Arc::new(
    include_bytes!("../fonts/Iced-Icons.ttf").as_slice(),
)),
```

`Cow<'static, [u8]>` implements `AsRef<[u8]>` itself, so it goes in as it is:
the borrowed arm keeps pointing at `.rodata`, and the owned arm was already on
the heap. There was never a type here that needed the bytes to be owned.

Measured on the board's own path, with the suite that counts every byte the Rust
side asks the allocator for (`firmware/panel-tests/tests/font_memory_tests.rs`):

```
font db, 38 face(s) installed   1798122 B  ->  14698 B
```

The 1,798,122 B was this board's font -- the 1.8 MB Source Han Sans SC subset --
copied out of flash into PSRAM and held there for the life of the process. A
font a program installs now costs its size in flash and nothing in RAM.

What this does not change is what is drawn: where a font's bytes live is not
where they are read from.

## Still open

`custom` names `iced_tiny_skia`'s `Renderer`, so a `custom` build still compiles
`iced_tiny_skia`, and with it the tiny-skia rasteriser, even though the platform
draws through its own renderer. `pomelo` does not change that: it only wins the
*name*, and `custom` is still what pulls the crate in for the defaults.
Removing it entirely means the fork stops needing `iced_tiny_skia` -- its
renderer, its rasteriser and its layers -- which is worth doing once the
recorded renderer has replaced tiny-skia for real.

## Updating

```bash
git remote add upstream https://github.com/iced-rs/iced.git   # once
git fetch upstream
git rebase upstream/0.14
git push origin pomelo/esp32s3 --force-with-lease
```

Then record the new commit in `pomelo-os`:

```bash
git -C ../.. add vendor/iced
git -C ../.. commit -m "vendor: bump iced"
```

## Where this is used

`pomelo-os` consumes the fork as a path patch: `[patch.crates-io]` points every
`iced_*` crate at `vendor/iced/*`, and the submodule pins this branch. That
repository has no manifest at its own root, so the table is written three times
-- `vendor/Cargo.toml` (the libraries), `firmware/panel-tests` and
`firmware/components/rust_main` -- because cargo reads `[patch]` only from the
root of a dependency graph. `vendor/README.md` there explains why the forks are
excluded from its workspace and what the update procedure is.
