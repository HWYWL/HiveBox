//! Metrics and colors.
//!
//! Absolute, like the rest of this repository's UI code: the panel is 480×480, type must not
//! scale with it, and a box that should adapt says so with `Length::Fill`. The one place that
//! would change on another panel is [`SCREEN`], and even that is only used by the host and the
//! tests.

/// The panel the launcher is designed for.
pub const SCREEN: u32 = 480;

/// The app grid: two columns of two rows, so a page holds four apps, one to a quadrant.
pub const COLUMNS: usize = 2;
pub const ROWS: usize = 2;

/// The apps one page of the grid holds.
pub const PER_PAGE: usize = COLUMNS * ROWS;

use std::time::Duration;

use pomelo_widgets::{Curve, FontSizeTier, SystemPreferences};

/// The font size tiers obtained from SystemPreferences: [18.0, 20.0, 24.0, 30.0].
#[allow(dead_code)]
pub const FONT_SIZES: [f32; 4] = SystemPreferences::font_sizes();
pub const FONT_EXTRA_SMALL: f32 = FontSizeTier::ExtraSmall.base_size(); // 18.0 px (Compact tier)
pub const FONT_SMALL: f32 = FontSizeTier::Small.base_size();            // 20.0 px (Small tier)
pub const FONT_STANDARD: f32 = FontSizeTier::Standard.base_size();      // 24.0 px (Standard tier)
#[allow(dead_code)]
pub const FONT_LARGE: f32 = FontSizeTier::Large.base_size();            // 30.0 px (Large tier)

/// The gap between the cells, and around the page.
pub const GUTTER: f32 = 20.0;
pub const ICON: f32 = 118.0;
pub const ICON_RADIUS: f32 = 33.0;
pub const GLYPH: f32 = 54.0;

/// The app tile container width: wider than the icon, and wider than most names need.
///
/// 160 px, so that the longest name the catalogue has stays on one line beside it. What happens to
/// one that does not is [`LABEL_MAX_WIDTH`]'s: a tile's label is a single line and is cut with an
/// ellipsis, because a label that wrapped would be a row a different height from the row beside it.
pub const TILE_WIDTH: f32 = 160.0;

/// The app grid icon label font size: sourced from the 18px system tier.
pub const LABEL: f32 = FONT_EXTRA_SMALL;
pub const GLYPH_GAP: f32 = 10.0;

/// Maximum width for a tile label before truncation with "...".
pub const LABEL_MAX_WIDTH: f32 = TILE_WIDTH - 4.0;

/// Estimated width of a single character in pixels at given font size.
///
/// In Source Han Sans / general sans-serif fonts:
/// - CJK characters and fullwidth symbols are 1.0 em.
/// - ASCII lowercase typically averages ~0.55 em (with 'm', 'w' ~0.85 em, 'i', 'l', 't' ~0.32 em).
/// - ASCII uppercase typically averages ~0.68 em (with 'M', 'W' ~0.85 em).
/// - Punctuation / symbols: hyphen/bracket ~0.40 em, dot/colon/space ~0.32 em.
pub fn char_width(c: char, font_size: f32) -> f32 {
    if c.is_ascii() {
        match c {
            'm' | 'w' | 'M' | 'W' | '@' | '%' => font_size * 0.85,
            'i' | 'l' | 'j' | 'I' | 't' | '!' | ':' | ';' | '.' | ',' | '\'' | '"' | ' ' | '|' | '`' => {
                font_size * 0.32
            }
            'r' | 'f' => font_size * 0.42,
            '-' | '(' | ')' | '[' | ']' | '{' | '}' => font_size * 0.40,
            'A'..='Z' => font_size * 0.68,
            _ => font_size * 0.55,
        }
    } else {
        font_size * 1.0
    }
}

/// Estimates the total width of a string in pixels at given font size.
pub fn text_width(s: &str, font_size: f32) -> f32 {
    s.chars().map(|c| char_width(c, font_size)).sum()
}

/// Formats and truncates a tile label to ensure it fits in a single line within `max_width`.
///
/// If `text` fits within `max_width`, it is returned as a single line (stripping any newline).
/// If `text` exceeds `max_width`, it is truncated and appended with `...` such that the
/// resulting string (including `...`) fits within `max_width`.
pub fn truncate_label(text: &str, max_width: f32, font_size: f32) -> String {
    let single_line = text.lines().next().unwrap_or(text);
    if text_width(single_line, font_size) <= max_width {
        return single_line.to_string();
    }

    let ellipsis = "...";
    let ellipsis_width = text_width(ellipsis, font_size);
    let budget = (max_width - ellipsis_width).max(0.0);

    let mut current_width = 0.0;
    let mut result = String::new();

    for c in single_line.chars() {
        let w = char_width(c, font_size);
        if current_width + w > budget {
            break;
        }
        current_width += w;
        result.push(c);
    }

    result.push_str(ellipsis);
    result
}

/// The pager.
///
/// How far a finger moves before a press becomes a drag: Flutter's standard `kTouchSlop` is 18.0 px.
/// This acts as a dead zone so that slight finger jitter or roll during a tap does not mistakenly
/// cancel the press or start dragging.
pub const SLOP: f32 = 18.0;

/// How far a finger has to travel without flicking to turn the page: about half the screen (45%).
///
/// Following Flutter's `PageScrollPhysics`, slow dragging requires moving past roughly half the
/// screen (216 px on this 480 px panel), while a quick flick (fling velocity > 450 px/s) turns
/// the page with only a brief swipe.
pub const SWIPE_COMMIT: f32 = 216.0;

/// How far from an edge of the panel a gesture has to *begin* to be that edge's gesture.
///
/// This is the band that tells a "go back" drag apart from a page scrolling, and it is the reason
/// the gesture layer can cover the whole screen at all: the swipe in the middle of a page is not
/// claimed, so it stays the page's. 24 px of a 480 px panel — a thumb's margin, and narrow enough
/// that the back button the settings pages put at the top left still gets its own presses.
pub const EDGE_ZONE: f32 = 24.0;

/// How far an edge swipe has to travel to count, rather than be a slip of the finger.
///
/// Roughly half of [`SWIPE_COMMIT`], and for a reason: turning a page is a deliberate move across
/// the screen, while going back is the shortest gesture a phone has and has to read as one. The
/// detector's own flick escape — 250 px/s — is what covers the quick flick that never travels this
/// far.
pub const EDGE_SWIPE: f32 = 96.0;

/// How long a page takes to settle after the finger leaves, and on what curve.
///
/// 240 ms on `EaseOutCubic`, which is the shape every phone's page turn has: a decisive start and a
/// stop that arrives rather than stops. The same 240 ms at a constant speed reads as slower than it
/// is, and much longer than this turns a turn into a wait — a flick commits the turn on release, so
/// what the finger is watching is only the settle.
pub const PAGE_SETTLE: Duration = Duration::from_millis(240);
pub const PAGE_CURVE: Curve = Curve::EaseOutCubic;

/// The app layer: how long an app takes to arrive over the desktop or leave it, and on what curve.
///
/// The same quarter of a second and the same curve as [`PAGE_SETTLE`], because they are the same
/// gesture family and the same panel — a page turn that started deliberately and an app that slid
/// away over a different curve would read as two different systems.
///
/// The cost of this one is in a different league to a page turn, though, and it is worth knowing
/// before turning it up: an app fills the panel, so *every* frame of this transition redraws the
/// whole screen twice — the app at its new position and the desktop underneath it — where a page
/// turn redraws two pages that were going to be drawn anyway. See
/// [`pomelo_widgets::ScreenTransition::duration`].
pub const TRANSITION: Duration = Duration::from_millis(240);
pub const TRANSITION_CURVE: Curve = Curve::EaseOutCubic;

/// The page dots: one per page, the page that is up lit.
///
/// Exported, like [`SCREEN`], because the panel tests find what is on screen by its colour.
pub const DOT: f32 = 8.0;
pub const DOT_GAP: f32 = 12.0;
pub const DOT_UP: (u8, u8, u8) = (233, 236, 244);
pub const DOT_REST: (u8, u8, u8) = (62, 68, 92);

/// The status bar. Taller than a bar of text: it is the top of a round display, and the row sits
/// inside it rather than against its edge.
pub const STATUS_HEIGHT: f32 = 48.0;

/// The bar's own background: pure white in light mode, pure black in dark mode.
///
/// Exported, like [`DOT_UP`], because the panel tests find what is on screen by its colour.
pub const STATUS_BG: (u8, u8, u8) = (255, 255, 255);
pub const STATUS_BG_DARK: (u8, u8, u8) = (0, 0, 0);
pub const STATUS_BG_LIGHT: (u8, u8, u8) = (255, 255, 255);

pub const STATUS_FONT: f32 = FONT_SMALL;
pub const STATUS_ICON: f32 = FONT_STANDARD;

/// The size of background app icons in the status bar: sourced from the 24px standard tier.
pub const STATUS_BG_APP_ICON: f32 = FONT_STANDARD;

/// Spacing between multiple background app icons in the status bar.
pub const STATUS_BG_APP_GAP: f32 = 6.0;

/// The battery, which is drawn rather than picked out of a set of pictures.
///
/// The reading goes *inside* it, so what the shape has to be is a box with room for three digits —
/// and no glyph in the set is that: the icon set draws a battery filling up, and its interior is
/// the fill. See `status::battery_widget`.
///
/// The numbers are a battery seen from the side: [`BATTERY_W`] by [`BATTERY_H`] of outline, and the
/// little terminal on the right that makes the shape a battery rather than a box.
pub const BATTERY_W: f32 = 40.0;
pub const BATTERY_H: f32 = 22.0;
pub const BATTERY_RADIUS: f32 = 6.0;
pub const BATTERY_BORDER: f32 = 1.5;
pub const BATTERY_NUB_W: f32 = 3.0;
pub const BATTERY_NUB_H: f32 = 9.0;
pub const BATTERY_NUB_RADIUS: f32 = 1.5;
/// The terminal stands off the body by a hair, so the two read as one shape and not as two boxes.
pub const BATTERY_NUB_GAP: f32 = 1.0;

/// The reading inside it: two steps below the bar's own text, and as large as three digits fit.
pub const BATTERY_NUMBER_FONT: f32 = 14.0;

/// The bolt, which stands where the percentage used to and only while the charger is connected.
///
/// A bolt and not a green battery: the number inside is already the colour the bar's text is, and a
/// second meaning for that colour would be a meaning the reading had to share with the charger.
pub const BATTERY_BOLT_FONT: f32 = FONT_EXTRA_SMALL;
pub const BATTERY_BOLT_GAP: f32 = 4.0;

pub const STATUS_GAP: f32 = 14.0;

/// How far the status bar's contents stay away from the screen's left and right edges.
///
/// The panel is a square with rounded corners and the bar is the band that meets them. The cut is
/// only a few pixels deep over the rows the text occupies, but a clock wedged into the corner of a
/// round display reads as a mistake whether or not it is *inside* the cut -- and these are the
/// panel's outermost pixels, the first the bezel takes.
///
/// It is [`GUTTER`], the page's own margin, so the clock sits over the left column of the grid
/// rather than at the edge of the glass: the bar is the only row that spans the full width, and one
/// number is enough to say where its contents start.
pub const STATUS_INSET: f32 = GUTTER + 7.0;

/// The signal scale, in bars.
///
/// Four, because that is the scale the HAL answers on (`ApInfo::signal_bars()`) -- and it is what
/// the launcher accepts, not what it can draw: the icon font has three bars and a crossed-out one,
/// so the fourth bar shows the third's picture. See `status::wifi_icon`.
pub const WIFI_BARS: u8 = 4;

/// The task switcher: the sheet that comes down over the launcher when the top edge is dragged.
///
/// A sheet and not another screen: it is a layer over whatever is up — the desktop, or an app still
/// running behind it — and the wash and the rounded cards are what say so. Its cards wear the height
/// of a finger rather than of their contents, because the cross on each of them has to be pressed
/// while the finger is moving.
pub const RECENTS_MARGIN: f32 = 24.0;
pub const RECENTS_TITLE_FONT: f32 = FONT_SMALL;
pub const RECENTS_CARD_H: f32 = 76.0;
pub const RECENTS_CARD_GAP: f32 = 12.0;
pub const RECENTS_CARD_RADIUS: f32 = 18.0;
pub const RECENTS_PADDING: f32 = 14.0;
pub const RECENTS_ICON: f32 = 48.0;
pub const RECENTS_ICON_RADIUS: f32 = 12.0;
pub const RECENTS_ICON_GLYPH: f32 = 26.0;
pub const RECENTS_NAME_FONT: f32 = FONT_SMALL;
/// The cross on a card, and the glyph beside the words that clear all of them at once.
pub const RECENTS_KILL: f32 = 44.0;
pub const RECENTS_KILL_GLYPH: f32 = 24.0;
/// The room between the sheet's title row and its first card.
pub const RECENTS_HEAD_GAP: f32 = 16.0;

/// The wash over the launcher while the sheet is down.
///
/// The page behind it stays legible, which is what says the sheet is a *layer* over it — but nothing
/// on that page is reachable, and a backdrop that left it looking pressable would be lying about
/// that. Alpha, because what is under it is the wallpaper.
pub const RECENTS_BACKDROP: (u8, u8, u8, u8) = (0, 0, 0, 178);

/// A card in the sheet: a slab of light on the wash, in either theme.
///
/// Translucent rather than a pair of solid colours, so a card is a slab *of whatever it covers*
/// rather than a patch of its own — which is the same trick the press wash on a grid tile uses.
pub const RECENTS_CARD_DARK: (u8, u8, u8, u8) = (255, 255, 255, 41);
pub const RECENTS_CARD_LIGHT: (u8, u8, u8, u8) = (0, 0, 0, 20);
