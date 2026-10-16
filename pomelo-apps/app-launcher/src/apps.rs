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

/// Fifteen apps, in the order the grid shows them: each [`crate::PER_PAGE`] fill a page, and the rest
/// fall on the one after — six, six and three.
///
/// The order is the interface's and not the alphabet's. The four the first page opened with are still
/// the first four, because they are still the four things this box is *for* — what it plays, what it
/// serves, what it can be told to do, and how it is set up. The fifth is the one tile that is about the
/// box's own software rather than about anything that software does: the updater, where a firmware or a
/// glyph table is replaced, and it sits beside the settings for that reason — both are pages about the
/// box itself rather than pages about a job. The calculator closes the page: it is the one app here
/// that has nothing to do with the box at all, and it was the first tile of the second page until the
/// page had room for six.
///
/// The second page holds the ones that are about the box and what is beside it rather than the jobs it
/// is for: the task viewer, which is about nothing else, the NAS monitor, which is about the machine
/// next to it, the clock, which is about nothing but the time this box thinks it is, the overview,
/// which is about the box itself and is the one tile that is about more than one thing, the file
/// manager, which is about the two filesystems this box can mount and is the way to move anything in
/// either, and the power log, which is about the battery and is the only tile on the grid that is about
/// *time* — the overview shows what the battery is, and this one shows what it has been doing.
///
/// The third page is the three things the box has no say in. The channel map opens it: it is about the
/// air around the box, which belongs to neither the box nor the machine beside it. The weather is the
/// channel map's subject one layer up — the sky rather than the air — and it is read the same way, over
/// the network, which is why the two are neighbours. The games app is last because it is the only tile
/// on the grid whose subject is not the box at all: it is the person holding it.
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
    Entry {
        name: "Update",
        name_zh: "更新",
        // An arrow coming down onto a bar, which is what `system_update` draws and what the page's two
        // rows do. Plain `UPDATE` is the pair of circular arrows that means "look again" — which is
        // what the button *inside* the page wears, on the row that checks for a new version — and a
        // tile that wore it too would be the same picture for two different things.
        icon: AppIcon::glyph(Icon::SYSTEM_UPDATE),
        // An indigo, in the stretch between the server's blue-violet and the settings page's grey: the
        // one hue the spread has left that is neither a job's colour nor a reading's own.
        accent: (63, 81, 181),
    },
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
    Entry {
        name: "Clock",
        name_zh: "时钟",
        // The face rather than a bell: this tile is the dial, the pomodoro and the alarms in one app,
        // and the dial is the page it opens on. A clock glyph is what the *whole* of it is, and the
        // time is what it is about.
        icon: AppIcon::glyph(Icon::SCHEDULE),
        // The plum end of the spread: the two hues either side of it are the music app's rose and the
        // server's violet, and this is the one place left between them that is not a grey.
        accent: (214, 51, 132),
    },
    // Page 2
    Entry {
        name: "Overview",
        name_zh: "概览",
        // The box's own dashboard rather than any one of its readings: this is the page that draws the
        // battery, the radio, the disks, the memory and the server at once, so its glyph is the
        // dashboard and not the battery or the disk that a single-reading tile would use.
        icon: AppIcon::glyph(Icon::SPACE_DASHBOARD),
        // The hue left in the middle: the widest gap in the spread is between the calculator's orange
        // and the terminal's green, and a lime is the one colour that is close to neither.
        accent: (104, 159, 56),
    },
    Entry {
        name: "Files",
        name_zh: "文件管理",
        // An open folder rather than a shut one: browsing is what the app is for, and the page it opens
        // on is a listing, so the picture is the thing that has been gone into rather than the thing
        // sitting there. The rows inside it draw the shut glyph, so the two are told apart by shape.
        icon: AppIcon::glyph(Icon::FOLDER_OPEN),
        // A warm brown, the stretch between the music app's red and the calculator's orange: paper and
        // cardboard, which is what a file manager is about, and the one warm hue the spread had left.
        accent: (166, 111, 74),
    },
    Entry {
        name: "Power Log",
        name_zh: "电量日志",
        // A trend line rather than a battery: the overview's tile is already the battery, and what this
        // one adds is the shape of it over time. Two battery tiles side by side would be two tiles a
        // person has to read to tell apart, and this one is meant to be found by its colour alone.
        icon: AppIcon::glyph(Icon::INSIGHTS),
        // A violet, in the widest hue gap left: between the server's blue-violet and the clock's plum
        // there is a stretch of purple that nothing else on the grid is using.
        accent: (170, 96, 214),
    },
    Entry {
        name: "Wi-Fi Map",
        name_zh: "信道地图",
        // A radio being searched rather than a network being joined: this app is about the air, not about
        // getting onto it, and the settings' Wi-Fi page is where joining lives. The plain `WIFI` glyph
        // would be that other thing, and it is the one the list inside this app already uses per network.
        icon: AppIcon::glyph(Icon::WIFI_FIND),
        // A teal, between the tasks app's cyan and the terminal's green: the one stretch of hue left that
        // is neither a signal-meter blue nor a success green.
        accent: (0, 137, 123),
    },
    // Page 3
    Entry {
        name: "Weather",
        name_zh: "天气",
        // A sun behind a cloud, which is the app's own glyph for its most ordinary unsettled sky: the
        // page inside picks one glyph per condition, so the tile's cannot be any single one of them —
        // and of the set this is the one that reads as a *forecast* rather than as weather that has
        // already happened. A plain cloud would be its overcast and a plain sun its clear.
        icon: AppIcon::glyph(Icon::PARTLY_CLOUDY_DAY),
        // The sky's own blue, a step from the channel map's teal and a step from the web manager's: the
        // one hue left in the spread that is unmistakably a forecast and unmistakably not a radio.
        accent: (2, 136, 209),
    },
    Entry {
        name: "Games",
        name_zh: "游戏",
        // A gamepad, and the one glyph here that names a *category* rather than an app: this tile opens
        // a list of games, so what it has to say is "games". The square grid the one game is played on
        // (`GRID_VIEW`) is the glyph its row wears inside, exactly as the channel map's tile is not the
        // per-network glyph its own list uses.
        icon: AppIcon::glyph(Icon::SPORTS_ESPORTS),
        // An arcade gold, in the one warm stretch the spread had left: it is the yellow end of the run
        // that holds the calculator's orange and the music app's red, and it is a colour no reading of
        // the box would ever claim for itself.
        accent: (251, 192, 45),
    },
];
