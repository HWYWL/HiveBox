//! The catalogue the grid shows.
//!
//! The launcher decides the identity, order, localized names, icon and accent colour of every app
//! the home screen shows.

use pomelo_material_symbols::Icon;
pub use pomelo_widgets::{AppIcon, AppMeta as Entry};

include!(concat!(env!("OUT_DIR"), "/baked_icons.rs"));

/// Six apps, in the order the grid shows them: the first [`crate::PER_PAGE`] fill the first page,
/// and the rest fall on the second.
///
/// The order is the interface's and not the alphabet's. The four on the first page are the four
/// things this box is *for* — what it plays, what it serves, what it can be told to do, and how it
/// is set up — and the second page holds the two that are about the box itself rather than the jobs
/// it is for: the calculator, which has nothing to do with it at all, and the task viewer, which is
/// about nothing else.
pub const CATALOGUE: &[Entry] = &[
    // Page 1
    Entry {
        name: "Music",
        name_zh: "音乐",
        icon: AppIcon::glyph(Icon::MUSIC_NOTE),
        accent: (60, 40, 44),
    },
    Entry {
        name: "Web Manager",
        name_zh: "网页管理",
        icon: AppIcon::glyph(Icon::PUBLIC),
        accent: (44, 40, 78),
    },
    Entry {
        name: "Terminal",
        name_zh: "终端",
        icon: AppIcon::glyph(Icon::TERMINAL),
        accent: (38, 44, 62),
    },
    Entry {
        name: "Settings",
        name_zh: "设置",
        icon: AppIcon::glyph(Icon::SETTINGS),
        accent: (40, 52, 60),
    },
    // Page 2
    Entry {
        name: "Calculator",
        name_zh: "计算器",
        icon: AppIcon::glyph(Icon::CALCULATE),
        accent: (46, 62, 46),
    },
    Entry {
        name: "Tasks",
        name_zh: "任务查看器",
        // A system monitor rather than a list: the list is what it draws, and this is what it *is*.
        icon: AppIcon::glyph(Icon::MONITOR_HEART),
        accent: (44, 56, 66),
    },
];
