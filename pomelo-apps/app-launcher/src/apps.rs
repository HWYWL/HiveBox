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
/// fall on the one after — eight and seven.
///
/// The order is the interface's and not the alphabet's, and the two pages are told apart by *when* a
/// person wants what is on them rather than by what it is.
///
/// The first page is the box doing its job, read left to right and then down. The first row is the
/// four things this box is *for*: what it plays, the page it serves, the machine next door it watches,
/// and the files on either of them — the two of those that reach *off* the box sit together at the
/// end of the row for that reason. The second row is what is left of a box in use: the two tiles
/// whose subject is not the box at all — the sky outside it, and the person holding it — and then the
/// two that are about its own software rather than about anything that software does, the settings and
/// the updater, which is where a firmware or a glyph table is replaced. Those two close the page
/// because they are the two a person opens *last*: one when something is wrong, the other when
/// something new exists.
///
/// The second page is the box *itself*: the terminal, where the box is asked directly; the task
/// viewer, which is about nothing but what the box is running; the clock, which is about nothing but
/// the time the box thinks it is; and the power log, which is about the battery and is the only tile
/// on the grid that is about *time* — the status bar shows what the battery is, and this one shows
/// what it has been doing. The two after those are the ones a person only ever opens to play with:
/// the calculator, which is about neither the box nor anything beside it, and the channel map, which
/// is about the air around the box and belongs to neither.
///
/// The cloud closes the page, and it is the one tile on it whose subject is *off* the panel: the
/// machines in an account rather than the box in the hand. It is on the second page and not beside
/// the server tile for a reason that is only about room — the first page holds [`crate::PER_PAGE`]
/// and its two rows are the box's own jobs, which a cloud account is not one of — and it is last
/// rather than first for the same reason: a page is read in order, and a tile that pushed the six
/// above it along would be a grid every finger has to learn again.
///
/// Fourteen and not fifteen: the overview tile was the one tile whose subject was *more than one
/// thing* — the dashboard that drew the battery, the radio, the disks, the memory and the server at
/// once — and every reading on it is drawn somewhere better on its own page. It was the tile whose
/// own page could be done without, so it is the tile that went when the grid was rearranged.
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
        name: "Server",
        // One word, for the same reason the task viewer's is four characters: `服务器监控` was a
        // character too long for the tile and read as `服务器…`.
        name_zh: "服务器",
        // The machine beside this one rather than anything in it: a server-in-a-box is what it draws,
        // and reaching it is what the app is for. Named for what it is and not for what it usually is:
        // what it watches is anything that answers SSH, and whether that box is a NAS is the owner's
        // business rather than this tile's.
        icon: AppIcon::glyph(Icon::DNS),
        accent: (112, 92, 240),
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
    // Page 2
    Entry {
        name: "Terminal",
        name_zh: "终端",
        icon: AppIcon::glyph(Icon::TERMINAL),
        accent: (26, 158, 98),
    },
    Entry {
        name: "Tasks",
        // Four characters, which is all a tile's name has room for: the label is one line under the
        // icon and is cut with an ellipsis when it does not fit, and five of them drew `任务查…` on the
        // panel. See `style::LABEL_MAX_WIDTH`, which a test holds every name in here to.
        name_zh: "任务详情",
        // A system monitor rather than a list: the list is what it draws, and this is what it *is*.
        icon: AppIcon::glyph(Icon::MONITOR_HEART),
        accent: (0, 168, 190),
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
    Entry {
        name: "Power Log",
        name_zh: "电量日志",
        // A trend line rather than a battery: the status bar's icon is already a battery, and what this
        // one adds is the shape of it over time. Two battery pictures side by side would be two a
        // person has to read to tell apart, and this one is meant to be found by its colour alone.
        icon: AppIcon::glyph(Icon::INSIGHTS),
        // A violet, in the widest hue gap left: between the server's blue-violet and the clock's plum
        // there is a stretch of purple that nothing else on the grid is using.
        accent: (170, 96, 214),
    },
    Entry {
        name: "Calculator",
        name_zh: "计算器",
        icon: AppIcon::glyph(Icon::CALCULATE),
        accent: (246, 158, 34),
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
    Entry {
        name: "Cloud",
        name_zh: "云服务",
        // A cloud with two arrows around it rather than a plain one: the plain cloud is the weather
        // page's overcast — see its own comment above — and what this tile is about is a cloud somebody
        // *acts on*, which is what the arrows say. It is the glyph the page's own header wears too, and
        // here that is the point rather than a repetition: the tile and the page are one subject.
        icon: AppIcon::glyph(Icon::CLOUD_SYNC),
        // The colour of a cloud seen from below, and the one hue on the grid that is a *desaturated*
        // blue: the web manager's and the weather page's are both at full strength beside it, so the
        // three blues are told apart by saturation rather than by a person reading the labels.
        accent: (91, 143, 176),
    },
    // Page 2, after the cloud: the same kind of page about something that is not the box.
    Entry {
        name: "WorkBuddy",
        // Four characters, because the label is one line of a tile and the product's own name is
        // two characters too long for it: what the tile says in Chinese is what the page is about.
        name_zh: "工作助手",
        // A circle with a tick in it: what the page is about is whether today has been checked in.
        // The page's own header wears it too, so the tile and the page are one subject.
        icon: AppIcon::glyph(Icon::CHECK_CIRCLE),
        // A green of its own, and the only green on the grid: the check-in is the one tile here
        // that is about a thing going *well*.
        accent: (66, 165, 110),
    },
];
