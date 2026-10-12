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
//! One colour each and no two alike, by hue: the grid is scanned rather than read, and a tile is found by
//! remembering that the music app is the red one. The three apps that are *about* something on the box —
//! the settings, the tasks and the server — sit on the cool half of that spread on purpose, because they
//! are the ones a person opens while looking at the machine rather than at the screen.

use pomelo_material_symbols::Icon;
pub use pomelo_widgets::{AppIcon, AppMeta as Entry};

include!(concat!(env!("OUT_DIR"), "/baked_icons.rs"));

/// Seven apps, in the order the grid shows them: the first [`crate::PER_PAGE`] fill the first page,
/// and the rest fall on the second.
///
/// The order is the interface's and not the alphabet's. The four on the first page are the four
/// things this box is *for* — what it plays, what it serves, what it can be told to do, and how it
/// is set up — and the second page holds the three that are about the box itself and what is beside
/// it rather than the jobs it is for: the calculator, which has nothing to do with it at all, the task
/// viewer, which is about nothing else, and the NAS monitor, which is about the machine next to it.
pub const CATALOGUE: &[Entry] = &[
    // Page 1
    Entry {
        name: "Music",
        name_zh: "音乐",
        icon: AppIcon::glyph(Icon::MUSIC_NOTE),
        accent: (233, 69, 96),
    },
    Entry {
        name: "Web Manager",
        name_zh: "网页管理",
        icon: AppIcon::glyph(Icon::PUBLIC),
        accent: (35, 132, 245),
    },
    Entry {
        name: "Terminal",
        name_zh: "终端",
        icon: AppIcon::glyph(Icon::TERMINAL),
        accent: (26, 158, 98),
    },
    Entry {
        name: "Settings",
        name_zh: "设置",
        icon: AppIcon::glyph(Icon::SETTINGS),
        accent: (124, 132, 146),
    },
    // Page 2
    Entry {
        name: "Calculator",
        name_zh: "计算器",
        icon: AppIcon::glyph(Icon::CALCULATE),
        accent: (246, 158, 34),
    },
    Entry {
        name: "Tasks",
        name_zh: "任务查看器",
        // A system monitor rather than a list: the list is what it draws, and this is what it *is*.
        icon: AppIcon::glyph(Icon::MONITOR_HEART),
        accent: (0, 168, 190),
    },
    Entry {
        name: "Server",
        name_zh: "服务器监控",
        // The machine beside this one rather than anything in it: a server-in-a-box is what it draws,
        // and reaching it is what the app is for. Named for what it is and not for what it usually is:
        // what it watches is anything that answers SSH, and whether that box is a NAS is the owner's
        // business rather than this tile's.
        icon: AppIcon::glyph(Icon::DNS),
        accent: (112, 92, 240),
    },
];
