//! Metrics and colours.
//!
//! The panel is 480×480 and type must not scale with it, so the metrics are absolute and the one
//! number that would move on another panel is [`SCREEN`] — and only the host and the tests read
//! that. Anything that should adapt says so with `Length::Fill` at the call site instead.

use iced::Color;

/// The panel the web manager is designed for.
pub const SCREEN: u32 = 480;

/// The page: a gutter all round, so the card and the switch do not touch the glass.
pub const PAGE_PADDING: f32 = 28.0;

/// The status card.
pub const CARD_RADIUS: f32 = 16.0;
pub const CARD_BORDER_WIDTH: f32 = 1.0;
pub const CARD_PADDING: f32 = 20.0;

/// The switch: its padding, not its size, is what makes it a finger's target.
pub const BUTTON_PADDING: [f32; 2] = [18.0, 24.0];
pub const BUTTON_RADIUS: f32 = 14.0;

/// The gap between the blocks of the page.
pub const SECTION_GAP: f32 = 26.0;

use pomelo_widgets::FONT_BASE;

/// The font size the interface draws at, and the only one there is.
///
/// The four-tier scale is gone, and with it the 18/20/30 px baked glyph tables: [`FONT_BASE`] is the
/// size of the one table that ships.
pub const FONT: f32 = FONT_BASE;

/// The type, all of it one size: a title, an address and a footnote are written in it now, where the
/// tiers used to step them apart.
pub const TITLE_FONT: f32 = FONT;
pub const SUBTITLE_FONT: f32 = FONT;
pub const STATUS_FONT: f32 = FONT;
pub const ADDRESS_FONT: f32 = FONT;
pub const BUTTON_FONT: f32 = FONT;
pub const FOOTNOTE_FONT: f32 = FONT;

use pomelo_widgets::preferences::ThemeMode;

pub const CARD: Color = Color::from_rgb8(17, 24, 39);
pub const CARD_BORDER: Color = Color::from_rgb8(30, 41, 59);
pub const TITLE: Color = Color::from_rgb8(167, 139, 250);
pub const BODY: Color = Color::from_rgb8(226, 232, 240);
pub const MUTED: Color = Color::from_rgb8(148, 163, 184);
pub const RUNNING: Color = Color::from_rgb8(52, 211, 153);
pub const ERROR: Color = Color::from_rgb8(248, 113, 113);
pub const BUTTON: Color = Color::from_rgb8(79, 70, 229);
pub const BUTTON_PRESSED: Color = Color::from_rgb8(99, 102, 241);
pub const STOP: Color = Color::from_rgb8(220, 38, 38);
pub const STOP_PRESSED: Color = Color::from_rgb8(239, 68, 68);

/// Page background for the given theme mode.
///
/// The dark one is the box's own black ([`ThemeMode::background`]) rather than a near-black of this app's
/// own: the panel sits inside a black bezel, and only an exact black leaves that bezel's edge undrawn.
pub fn background_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(243, 244, 246)
    } else {
        theme.background()
    }
}

/// Card background for the given theme mode.
pub fn card_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(255, 255, 255)
    } else {
        CARD
    }
}

/// Card border for the given theme mode.
pub fn card_border_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(229, 231, 235)
    } else {
        CARD_BORDER
    }
}

/// Heading colour for the given theme mode.
pub fn title_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(109, 40, 217)
    } else {
        TITLE
    }
}

/// Body text colour for the given theme mode.
pub fn body_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(31, 41, 55)
    } else {
        BODY
    }
}

/// Secondary text colour for the given theme mode.
pub fn muted_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(107, 114, 128)
    } else {
        MUTED
    }
}

/// The colour that says "the server is up", for the given theme mode.
pub fn running_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(5, 150, 105)
    } else {
        RUNNING
    }
}

/// The colour that says "something went wrong", for the given theme mode.
pub fn error_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(220, 38, 38)
    } else {
        ERROR
    }
}

/// The start switch's fill for the given theme mode.
pub fn button_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(79, 70, 229)
    } else {
        BUTTON
    }
}

/// The start switch's pressed fill for the given theme mode.
pub fn button_pressed_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(99, 102, 241)
    } else {
        BUTTON_PRESSED
    }
}

/// The stop switch's fill for the given theme mode.
pub fn stop_button_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(220, 38, 38)
    } else {
        STOP
    }
}

/// The stop switch's pressed fill for the given theme mode.
pub fn stop_button_pressed_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(239, 68, 68)
    } else {
        STOP_PRESSED
    }
}
