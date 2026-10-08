# pomelo-apps

The applications of [Pomelo OS](https://github.com/pomelos-on-sale/pomelo-os), as one project.

Each app is a **standard iced program**: a `main.rs` that builds state, an `update` and a `view`,
and nothing that knows what a board is. Every one of them runs on a desktop with a window and the
same source is what the firmware image hosts on the panel — which layer answers is decided by the
graph root that builds it, not by the app.

```bash
cargo run -p calculator            # any of the seven, by name
cargo test --workspace             # every app's own suite
```

On a WSLg box, ask for X11 (`env -u WAYLAND_DISPLAY cargo run -p terminal`): WSLg's Wayland server
resets the connection right after the window exists, for iced's own examples as much as for these.
A window that feels slow is a *debug* build — iced's tiny-skia rasteriser is scalar, so
`--release` is the difference between usable and not.

| app | what it is |
| :--- | :--- |
| `app-launcher` | the home screen: a paged grid of tiles, and the one app that is not a leaf — it hosts the other six as widgets (`view`/`update` called by the launcher, so their state is its state) and merges the subscription of whichever one is on screen |
| `calculator` | keys, a display and the arithmetic |
| `demo-counter` | one button that counts: the smallest complete program here |
| `hello` | a canvas stroke animation, drawn in a `canvas::Program` |
| `music-player` | a playlist, a transport and a readout, over the HAL's audio backend |
| `settings` | a list of sections, one of which is a live Wi-Fi page with a password prompt |
| `terminal` | a shell over LittleFS, with its own on-screen keyboard |

## What it depends on

iced, from crates.io, like any other iced program — there is **no patch table here**, deliberately.
The four things that make these apps ours rather than anyone's arrive as git dependencies pinned by
revision, because this repository has to build on its own:

| crate | who needs it | why it is not in here |
| :--- | :--- | :--- |
| [`pomelo-hal`](https://github.com/pomelos-on-sale/pomelo-hal) | `app-launcher`, `music-player`, `settings` | the board interfaces and their desktop simulator — every app that talks to hardware talks to this |
| [`pomelo-widgets`](https://github.com/pomelos-on-sale/pomelo-widgets) | `settings`, `terminal` | the widgets two or more apps draw: today the on-screen keyboard, so that a password prompt and a shell cannot drift apart |
| [`pomelo-material-symbols`](https://github.com/pomelos-on-sale/pomelo-material-symbols) | `app-launcher` | one font of the whole Material Symbols catalogue, and one `const` per icon |

Being a *member* of this workspace is what lets the seven be one project; it is also what keeps the
firmware from listing them as members in turn (a package cannot be a member of two workspaces), so
`rust_main` and the panel tests over there depend on them by path. Same manifests, a different
graph.

## Licence

GPL-3.0-only — see `LICENSE`.
