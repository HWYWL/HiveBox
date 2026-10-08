//! Metrics and colours.
//!
//! The original settings app's numbers, so that the two apps laid out alike:
//! absolute type, fluid boxes. Only [`SCREEN`] is about the panel; everything else is a
//! measurement of the design, and a wider screen gets wider boxes, never bigger text.

use iced::Color;
use pomelo_hal::FlashRegionKind;

use crate::SettingsSection;

/// The panel the app is designed for.
pub const SCREEN: u32 = 480;

/// The page: its margins, and the gap between two cards.
pub const PAGE_MARGIN: f32 = 20.0;

/// The gap between two cards.
///
/// One number for both kinds of page: the main list's groups and a sub-page's blocks are cards, and
/// the space between two of them is the only thing that says where one card ends.
pub const CARD_GAP: f32 = 30.0;

/// The space above a list's title.
///
/// That title is the first thing in the list rather than a bar over it, so it needs its own room:
/// the panel's edge is too close for a heading to sit on, and the heading is bigger than the rows
/// it heads.
pub const TITLE_TOP_GAP: f32 = 40.0;

/// The space between a list's title and its first card.
///
/// Its own number and not `CARD_GAP`: a title is not a card, and a heading pushed as far from what
/// it heads as two cards are from each other reads as separate from it.
pub const TITLE_GAP: f32 = 15.0;

/// A sub-page's own top.
///
/// Small, and not `TITLE_TOP_GAP`: what follows is a row of controls rather than a heading.
pub const NAV_TOP_GAP: f32 = 24.0;

/// The gap between a page's navigation row and the head card under it.
pub const NAV_GAP: f32 = 32.0;

/// The back button in the navigation row: a capsule, its glyph, and the shape those two make.
///
/// Height sets the shape — a radius of half of it turns each end into a semicircle — and the width
/// adds the rectangle between them. A circle's target is its diameter and nothing else, so a finger
/// a few pixels to the side of a 48 px circle misses it; the same height carried out sideways does
/// not, and a control this much wider is a control this much easier to hit.
pub const NAV_BUTTON_W: f32 = 72.0;
pub const NAV_BUTTON_H: f32 = 48.0;
pub const NAV_GLYPH: f32 = 26.0;

/// The size of a title: the large tier, so a page is headed in the size the system preference asks
/// for rather than in a measurement of the design.
pub const TITLE_FONT: f32 = FontSizeTier::Large.base_size();

/// Under the last card of a page, so a list does not end flush with the panel's edge.
pub const BOTTOM_GAP: f32 = 30.0;

use pomelo_widgets::{FontSizeTier, SystemPreferences};

/// The font size tiers obtained from SystemPreferences: [18.0, 20.0, 24.0, 30.0].
pub const FONT_SIZES: [f32; 4] = SystemPreferences::font_sizes();
pub const FONT_EXTRA_SMALL: f32 = FontSizeTier::ExtraSmall.base_size(); // 18.0 px (Compact tier)
pub const FONT_SMALL: f32 = FontSizeTier::Small.base_size();            // 20.0 px (Small tier)
pub const FONT_STANDARD: f32 = FontSizeTier::Standard.base_size();      // 24.0 px (Standard tier)
pub const FONT_LARGE: f32 = FontSizeTier::Large.base_size();            // 30.0 px (Large tier)

/// A title drawn on a band rather than in the list: the password prompt's, which is a modal over the
/// page and so has no list to head.
pub const NAV_FONT: f32 = FONT_STANDARD;

/// A grouped card.
pub const CARD_RADIUS: f32 = 20.0;
pub const CARD_BORDER_WIDTH: f32 = 1.0;

/// A row of the main list.
///
/// `ROW_TEXT_INSET` is the one measurement here that is a *sum*: the row's own padding, then the
/// badge, then the gap after it — which is where the label begins. The divider under a row starts
/// there too, so the line and the text are one expression rather than the same arithmetic written
/// out twice, and changing the badge moves both.
pub const ROW_PADDING_V: f32 = 16.0;
pub const ROW_PADDING_H: f32 = 14.0;
pub const ROW_TEXT_INSET: f32 = ROW_PADDING_H + Badge::ROW.side + BADGE_GAP;
pub const ROW_SEPARATOR: f32 = 1.0;

/// A row of a detail card.
pub const DETAIL_PADDING_V: f32 = 14.0;
pub const DETAIL_PADDING_H: f32 = 16.0;
pub const DETAIL_FONT: f32 = FONT_SMALL;

/// A switch row's horizontal room.
///
/// The vertical is [`ROW_PADDING_V`] and not a second number: a row is a row, and whether it holds
/// a toggle or a badge does not make it two heights. That is the whole of why a switch row and a
/// tile sat four pixels apart on the same screen — see [`ROW_PADDING_V`].
pub const SWITCH_PADDING_H: f32 = 16.0;

/// The icon square: a glyph on a solid, at one of two sizes.
///
/// The three numbers travel together because together they are one measurement: a glyph is only
/// legible on a solid of a certain size, and a corner only reads as a corner in proportion to it.
/// A list row takes [`Badge::ROW`], a sub-page's head [`Badge::HEAD`].
#[derive(Debug, Clone, Copy)]
pub struct Badge {
    /// The square's side.
    pub side: f32,
    /// The glyph inside it.
    pub glyph: f32,
    /// Its corner.
    pub radius: f32,
}

impl Badge {
    /// A list row's, beside a [`LABEL_FONT`] label.
    pub const ROW: Self = Self {
        side: 28.0,
        glyph: FONT_SMALL,
        radius: 6.0,
    };

    /// A sub-page head's, beside a [`TITLE_FONT`] title.
    pub const HEAD: Self = Self {
        side: 64.0,
        glyph: 40.0,
        radius: 16.0,
    };
}

/// The gap between a row's badge and its label.
pub const BADGE_GAP: f32 = 12.0;

/// A sub-page's head card: the room inside it, and the gap between the badge and the title.
pub const HEAD_PADDING: f32 = 20.0;
pub const HEAD_GAP: f32 = 16.0;

/// The main list's typography sourced from SystemPreferences tiers.
pub const LABEL_FONT: f32 = FONT_STANDARD;
pub const VALUE_FONT: f32 = FONT_SMALL;
pub const VALUE_GAP: f32 = 8.0;
pub const CHEVRON_FONT: f32 = 28.0;

/// The switch: a 52x28 track with a 24px knob, inset 2px from the edge it rests against.
pub const TOGGLE_W: f32 = 52.0;
pub const TOGGLE_H: f32 = 28.0;
pub const TOGGLE_RADIUS: f32 = 14.0;
pub const TOGGLE_INSET: f32 = 2.0;
pub const KNOB: f32 = 24.0;
pub const KNOB_RADIUS: f32 = 12.0;

/// A usage bar.
pub const USAGE_PADDING: f32 = 16.0;
pub const BAR_GAP: f32 = 10.0;
pub const BAR_HEIGHT: f32 = 10.0;
pub const BAR_RADIUS: f32 = 5.0;

/// The flash map's key: the square beside a region's name, and how far the name stands from it.
///
/// The same height as the bar it is a key to, so a row and its segment are recognisably the same
/// thing — and four pixels of corner, because a square at this size with a sharper one reads as a
/// bullet rather than as a swatch of colour.
pub const FLASH_CHIP: f32 = 10.0;
pub const FLASH_CHIP_RADIUS: f32 = 3.0;
pub const FLASH_CHIP_GAP: f32 = 10.0;

/// The palette chips of the theme page.
pub const CHIP_W: f32 = 60.0;
pub const CHIP_H: f32 = 40.0;
pub const CHIP_RADIUS: f32 = 10.0;
pub const CHIP_GAP: f32 = 10.0;
pub const PALETTE_PADDING: f32 = 16.0;

/// A network's row in the Wi-Fi list, and the four bars that stand for its signal.
///
/// Four, not three: `ApInfo::signal_bars()` in the HAL answers on a `0..=4` scale, and a scale
/// drawn with one bar fewer than it is measured in is a bar that can never light.
pub const SIGNAL_BARS: u8 = 4;
pub const SIGNAL_BAR_W: f32 = 3.0;
pub const SIGNAL_BAR_GAP: f32 = 2.0;
pub const SIGNAL_BAR_H: f32 = 14.0;
pub const SIGNAL_BAR_MIN_H: f32 = 4.0;
pub const SIGNAL_GAP: f32 = 10.0;

/// The password prompt: a sheet at the foot of the panel.
///
/// It reaches the panel's left, right and bottom edges and rounds only its top two corners, because
/// that is what it is: a keyboard with a question above it, standing on the floor of the screen. A
/// sheet that floated free of three edges would be a dialog, and this one is not asking to be
/// centred.
///
/// The gap at the top is what is left of the page: it is the only part of the dimmed list still
/// visible, and it is what says the sheet is a layer *over* the page rather than the page itself.
///
/// Small, because there is not much left to give it. The sheet is the panel less this gap, the band
/// takes [`keyboard_height`] off the bottom of it, and the question above the band needs the rest —
/// and on a 480 px panel those three come to very nearly all of it. See [`PROMPT_GAP`].
pub const PROMPT_TOP_GAP: f32 = 12.0;
pub const PROMPT_PADDING: f32 = 16.0;

/// The gap between the question's own rows.
///
/// Tight, and tighter than when the keys were flexible: with the band's height fixed, every pixel
/// between two rows here is a pixel the band is not allowed to have.
pub const PROMPT_GAP: f32 = 8.0;
pub const PROMPT_RADIUS: f32 = 16.0;

/// The password sheet's keyboard: the band's height for the panel this app is designed for.
///
/// The formula lives with the keyboard — `pomelo_widgets::touch_keyboard::band_height` — so this
/// band and the terminal's are the same band on the same screen. That is the whole point of asking
/// the crate: a password prompt that sized its own keyboard would be a second opinion on how big a
/// keyboard is.
///
/// It is a *height*, not a share of what is left over. A keyboard that shrank and grew with the
/// length of the question above it would change size when a connection failed.
pub fn keyboard_height() -> f32 {
    pomelo_widgets::touch_keyboard::band_height(SCREEN as f32)
}

/// The typed password: one dot per character, and a caret after them. The font is a Chinese and
/// Latin subset with no `•` in it, so the dots are painted rather than typed — the same reason the
/// terminal's cursor is a block.
pub const PASSWORD_DOT: f32 = 9.0;
pub const PASSWORD_DOT_GAP: f32 = 7.0;

/// The caret: a bar, how far it drops onto the baseline, and how close it stands to the character
/// before it.
///
/// The gap is its own and not `PASSWORD_DOT_GAP`: a caret touching the character it follows is where
/// the next one will go, and the space between two dots is not that.
pub const CARET_W: f32 = 2.0;
pub const CARET_H: f32 = 20.0;
pub const CARET_DROP: f32 = 5.0;
pub const CARET_GAP: f32 = 3.0;

/// The height of one line of a field.
///
/// `DETAIL_FONT` is 20 and this font's line box is 1.3 of the size. That ratio is not a guess: a test
/// that found the prompt's label by its text printed `height: 31.2` for a 24 px font, and iced takes
/// the ratio from the font, so it holds at any size. The caret and the dots are laid out against
/// this, because they are drawn rather than set and have no line box of their own to inherit.
pub const LINE_H: f32 = DETAIL_FONT * 1.3;

/// The box a value is typed into: its corner.
pub const FIELD_RADIUS: f32 = 12.0;

use pomelo_widgets::ThemeMode;

/// The page background: pure AMOLED black, so an unlit pixel costs nothing.
pub fn black() -> Color {
    rgb((0, 0, 0))
}

pub fn background_for(theme: ThemeMode) -> Color {
    theme.background()
}

/// The navigation bar, a shade above the black page.
pub fn nav() -> Color {
    nav_for(ThemeMode::Dark)
}

pub fn nav_for(theme: ThemeMode) -> Color {
    theme.nav_bar()
}

/// A grouped card.
pub fn card() -> Color {
    card_for(ThemeMode::Dark)
}

pub fn card_for(theme: ThemeMode) -> Color {
    theme.card()
}

/// The hairline around a card.
pub fn card_border() -> Color {
    card_border_for(ThemeMode::Dark)
}

pub fn card_border_for(theme: ThemeMode) -> Color {
    theme.border()
}

/// The hairline between two rows of a card.
pub fn separator() -> Color {
    separator_for(ThemeMode::Dark)
}

pub fn separator_for(theme: ThemeMode) -> Color {
    theme.separator()
}

/// The accent, used by the back button -- and by a test to find it.
pub fn accent() -> Color {
    rgb((10, 132, 255))
}

/// The accent under a finger.
pub fn accent_pressed() -> Color {
    rgb((64, 156, 255))
}

/// Primary text label colour.
pub fn label_for(theme: ThemeMode) -> Color {
    theme.text_primary()
}

/// A value on the right of a row.
pub fn muted() -> Color {
    muted_for(ThemeMode::Dark)
}

pub fn muted_for(theme: ThemeMode) -> Color {
    theme.text_secondary()
}

/// The chevron that says a row opens.
pub fn chevron() -> Color {
    chevron_for(ThemeMode::Dark)
}

pub fn chevron_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((100, 100, 105)),
        ThemeMode::Light => rgb((160, 160, 165)),
    }
}

/// The key of a detail row.
pub fn detail_key() -> Color {
    detail_key_for(ThemeMode::Dark)
}

pub fn detail_key_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((180, 180, 185)),
        ThemeMode::Light => rgb((100, 100, 105)),
    }
}

/// The main list's footnote.
pub fn footnote() -> Color {
    footnote_for(ThemeMode::Dark)
}

pub fn footnote_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((120, 120, 128)),
        ThemeMode::Light => rgb((142, 142, 147)),
    }
}

/// iOS green: the `on` track of a switch, and the battery badge.
pub fn green() -> Color {
    rgb((52, 199, 89))
}

/// The `on` track under a finger.
pub fn green_pressed() -> Color {
    rgb((92, 214, 125))
}

/// The `off` track of a switch.
pub fn track_off() -> Color {
    track_off_for(ThemeMode::Dark)
}

pub fn track_off_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((60, 60, 65)),
        ThemeMode::Light => rgb((229, 229, 234)),
    }
}

/// The `off` track under a finger.
pub fn track_off_pressed() -> Color {
    track_off_pressed_for(ThemeMode::Dark)
}

pub fn track_off_pressed_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((74, 74, 80)),
        ThemeMode::Light => rgb((209, 209, 214)),
    }
}

/// The empty part of a usage bar.
pub fn bar_track() -> Color {
    bar_track_for(ThemeMode::Dark)
}

pub fn bar_track_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((50, 50, 55)),
        ThemeMode::Light => rgb((229, 229, 234)),
    }
}

/// The memory bar.
pub fn memory_bar() -> Color {
    rgb((88, 86, 214))
}

/// The storage bar, and its badge.
pub fn storage_bar() -> Color {
    rgb((255, 149, 0))
}

/// The colour a region of the flash map is drawn in: its segment of the bar, and its key below.
///
/// One function rather than four colours at the call site, because the two must agree: a row whose
/// square and segment disagreed is a legend for a chart that is not the one on screen.
///
/// A region's kind and not its label: the four colours say what a stretch of flash *is*, which is
/// the whole point of the bar. One colour per partition would be one colour per row, and a picture
/// that is a list is not a picture.
pub fn flash_region_for(kind: FlashRegionKind, theme: ThemeMode) -> Color {
    match kind {
        FlashRegionKind::System => match theme {
            ThemeMode::Dark => rgb((110, 110, 118)),
            ThemeMode::Light => rgb((174, 174, 178)),
        },
        // The memory bar's purple: firmware is space spoken for and not the user's to fill, which is
        // the same thing that bar says about the heap.
        FlashRegionKind::Firmware => memory_bar(),
        // The orange the volumes above are drawn in: this region *is* that filesystem.
        FlashRegionKind::Data => storage_bar(),
        // The empty part of a bar, because that is what it is.
        FlashRegionKind::Unallocated => bar_track_for(theme),
    }
}

/// The hairline around a palette chip.
pub fn chip_border() -> Color {
    chip_border_for(ThemeMode::Dark)
}

pub fn chip_border_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgba((255, 255, 255), 60.0 / 255.0),
        ThemeMode::Light => rgba((0, 0, 0), 40.0 / 255.0),
    }
}

/// A lit signal bar of a network's row.
pub fn signal_on() -> Color {
    signal_on_for(ThemeMode::Dark)
}

pub fn signal_on_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgba((255, 255, 255), 0.9),
        ThemeMode::Light => rgba((0, 0, 0), 0.85),
    }
}

/// The bars a network's signal does not reach.
pub fn signal_off() -> Color {
    signal_off_for(ThemeMode::Dark)
}

pub fn signal_off_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgba((255, 255, 255), 0.22),
        ThemeMode::Light => rgba((0, 0, 0), 0.18),
    }
}

/// The page behind the password prompt, dimmed: the prompt has the finger, not the list.
pub fn backdrop() -> Color {
    rgba((0, 0, 0), 0.6)
}

/// A row that names a state instead of offering an action -- "scanning…", "no networks found".
pub fn notice() -> Color {
    notice_for(ThemeMode::Dark)
}

pub fn notice_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((120, 120, 128)),
        ThemeMode::Light => rgb((142, 142, 147)),
    }
}

/// A failed connection attempt.
pub fn error() -> Color {
    rgb((255, 69, 58))
}

/// The colours an entry's icon may be filled with.
///
/// An enum and not a [`Color`]: the palette is the whole set, so "add the colour here first" is
/// something the type says rather than something a comment asks for. A new colour is a new variant,
/// and every `match` over this stops compiling until it is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconColor {
    Blue,
    Green,
    Orange,
    Red,
    Purple,
    Grey,
    Black,
    White,
}

impl IconColor {
    /// The fill.
    pub fn color(self) -> Color {
        match self {
            Self::Blue => rgb((0, 122, 255)),
            Self::Green => rgb((52, 199, 89)),
            Self::Orange => rgb((255, 149, 0)),
            Self::Red => rgb((255, 69, 58)),
            Self::Purple => rgb((175, 82, 222)),
            Self::Grey => rgb((142, 142, 147)),
            Self::Black => rgb((0, 0, 0)),
            Self::White => Color::WHITE,
        }
    }

    /// The fill of a section's icon.
    ///
    /// One hue per entry, so a row is recognisable before its label is read. The palette repeats —
    /// eight colours and eight rows — and an entry that wants a colour the palette has not got adds
    /// a variant above rather than reaching for a [`Color`].
    pub fn for_section(section: SettingsSection) -> Self {
        match section {
            SettingsSection::Wifi => Self::Blue,
            SettingsSection::Memory => Self::Purple,
            SettingsSection::Storage => Self::Orange,
            SettingsSection::Battery => Self::Green,
            SettingsSection::SystemInfo => Self::Grey,
            SettingsSection::Theme => Self::Purple,
            SettingsSection::Time => Self::Red,
            SettingsSection::Main => Self::Blue,
        }
    }
}

/// The eight presets the theme page shows, in two rows of four.
pub fn palette() -> [Color; 8] {
    [
        rgb((0, 0, 0)),
        rgb((30, 58, 138)),
        rgb((6, 182, 212)),
        rgb((16, 185, 129)),
        rgb((245, 158, 11)),
        rgb((244, 63, 94)),
        rgb((168, 85, 247)),
        Color::WHITE,
    ]
}

fn rgb(color: (u8, u8, u8)) -> Color {
    Color::from_rgb8(color.0, color.1, color.2)
}

fn rgba(color: (u8, u8, u8), alpha: f32) -> Color {
    Color::from_rgba8(color.0, color.1, color.2, alpha)
}
