//! The catalogue the grid shows.
//!
//! The launcher decides the identity, order, localized names, icon and accent colour of every app
//! the home screen shows.
//!
//! # The accents are the picture, so they are at full strength
//!
//! A tile is a filled rounded square with a white glyph on it and its name under that, and the accent is
//! the whole of what tells one from another — so these are **saturated** colours rather than the near-black
//! shades they used to be, which were dark enough that a tile, a pressed tile and the wallpaper behind it
//! all read as one surface at arm's length. The white glyph is what the darkness was for; a colour that
//! already carries the contrast does not need to be dark to keep a glyph legible.
//!
//! # What is left here is the shell, and only the shell
//!
//! Four tiles, because every one of the ten applications a person installs is a package now: the music
//! player, the web manager, the server, the file manager, the weather, the games, the task viewer, the
//! power log, the channel map and the cloud all left the grid as they were published — see
//! `tools/build_apps.py`, which is what draws their tiles now. An application that can be installed is
//! one the image has no business also carrying, and an image that carried both would be one where a
//! person could uninstall something and still find it on the desktop.
//!
//! What stays is what the box cannot do without: the store, the settings (and the Wi-Fi page inside
//! it, which is how the box gets back on a network), the terminal, and the update tile. Those four are
//! the machine's own panel.
//!
//! # The store is the first tile
//!
//! Because an image that carries no applications is an image whose first question is "where do they
//! come from" — and the answer is one tap away, in the corner a thumb reaches without looking. It used
//! to sit seventh, behind the apps it was the shop for; now those apps arrive *from* it, so it opens
//! the grid the way a shop opens a street. The settings follow, because the second question of a box
//! with nothing on it is "is it on a network".
//!
//! The updater is the one tile here that is on its way out: the firmware and the glyph table are two of
//! the three things a repository publishes, and the third — the application packages — is the store's.
//! When the settings page carries the system update, this tile goes and the grid is three.

use pomelo_material_symbols::Icon;
pub use pomelo_widgets::{AppIcon, AppMeta as Entry};

include!(concat!(env!("OUT_DIR"), "/baked_icons.rs"));

/// The shell's tiles, in the order the grid shows them.
///
/// These are the apps compiled into this image, which cannot be uninstalled. The tiles after them are
/// the installed packages, in the catalog's order — see the crate's module docs on why those are not
/// here.
pub const CATALOGUE: &[Entry] = &[
    Entry {
        name: "App Store",
        name_zh: "应用商店",
        // A shop front. Not `system_update`: that is the updater's tile and means "a new version of
        // something this box already has", where this page is "something this box does not have yet".
        icon: AppIcon::glyph(Icon::STORE),
        // The store's own green, shared with the page it opens. A tile is found by remembering its
        // colour, and the colour that opens this page should be the one inside it.
        accent: (46, 158, 123),
    },
    Entry {
        name: "Settings",
        name_zh: "设置",
        icon: AppIcon::glyph(Icon::SETTINGS),
        accent: (124, 132, 146),
    },
    Entry {
        name: "Terminal",
        name_zh: "终端",
        icon: AppIcon::glyph(Icon::TERMINAL),
        accent: (26, 158, 98),
    },
    Entry {
        name: "Update",
        name_zh: "更新",
        // An arrow coming down onto a bar, which is what `system_update` draws and what the page's two
        // rows do. Plain `UPDATE` is the pair of circular arrows that means "look again" — which is
        // what the button *inside* the page wears, on the row that checks for a new version — and a
        // tile that wore it too would be the same picture for two different things.
        icon: AppIcon::glyph(Icon::SYSTEM_UPDATE),
        accent: (63, 81, 181),
    },
];
