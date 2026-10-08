//! The catalogue the grid shows.
//!
//! The launcher decides the identity, order, localized names, icon (glyph or baked bitmap),
//! and accent color of all apps displayed on the home screen.

use pomelo_material_symbols::Icon;
pub use pomelo_widgets::{AppIcon, AppMeta as Entry};

include!(concat!(env!("OUT_DIR"), "/baked_icons.rs"));

/// Six apps, in the order the grid shows them.
pub const CATALOGUE: &[Entry] = &[
    // Page 1
    Entry {
        name: "Terminal",
        name_zh: "终端",
        icon: AppIcon::glyph(Icon::TERMINAL),
        accent: (38, 44, 62),
    },
    Entry {
        name: "Calculator",
        name_zh: "计算器",
        icon: AppIcon::glyph(Icon::CALCULATE),
        accent: (46, 62, 46),
    },
    Entry {
        name: "demo-counter",
        name_zh: "demo-counter",
        icon: AppIcon::glyph(Icon::COUNTER_0),
        accent: (62, 48, 36),
    },
    Entry {
        name: "Hello",
        name_zh: "你好",
        icon: AppIcon::Bitmap(baked::HELLO_ICON),
        accent: (58, 38, 58),
    },
    // Page 2
    Entry {
        name: "Settings",
        name_zh: "设置",
        icon: AppIcon::glyph(Icon::SETTINGS),
        accent: (40, 52, 60),
    },
    Entry {
        name: "Music",
        name_zh: "音乐",
        icon: AppIcon::glyph(Icon::MUSIC_NOTE),
        accent: (60, 40, 44),
    },
];

