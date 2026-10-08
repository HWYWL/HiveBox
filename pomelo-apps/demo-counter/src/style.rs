//! Metrics and colours.
//!
//! The original counter's numbers, so the two apps laid out alike: absolute type, fluid boxes.
//! The one thing that could not be carried over is the card's 2:3:2 split, which that app spelled
//! with `Expanded` and this one with `Length::FillPortion` -- the same arithmetic, said twice.

use iced::Color;

/// The panel the counter is designed for.
pub const SCREEN: u32 = 480;

/// The page: 24 pt top and bottom, 50 pt each side, so the card is 380 px wide inside a 480 px
/// panel.
pub const PAGE_PADDING: [f32; 2] = [24.0, 50.0];

/// The card.
pub const CARD_RADIUS: f32 = 36.0;
pub const CARD_BORDER_WIDTH: f32 = 2.5;
pub const CARD_GAP: f32 = 24.0;

use pomelo_widgets::{FontSizeTier, SystemPreferences};

/// The font size tiers obtained from SystemPreferences: [18.0, 20.0, 24.0, 30.0].
pub const FONT_SIZES: [f32; 4] = SystemPreferences::font_sizes();
pub const FONT_EXTRA_SMALL: f32 = FontSizeTier::ExtraSmall.base_size(); // 18.0 px (Compact tier)
pub const FONT_SMALL: f32 = FontSizeTier::Small.base_size();            // 20.0 px (Small tier)
pub const FONT_STANDARD: f32 = FontSizeTier::Standard.base_size();      // 24.0 px (Standard tier)
pub const FONT_LARGE: f32 = FontSizeTier::Large.base_size();            // 30.0 px (Large tier)

/// The type, sourced from SystemPreferences (no custom font size literals).
pub const TITLE_FONT: f32 = FONT_STANDARD; // 24.0 px
pub const LABEL_FONT: f32 = FONT_STANDARD; // 24.0 px
pub const FOOTER_FONT: f32 = FONT_SMALL;   // 20.0 px
pub const NUMBER_FONT: f32 = 96.0;         // Giant display number (does not scale)

/// The button: its padding, not its size, is what the original set.
pub const BUTTON_PADDING: [f32; 2] = [16.0, 52.0];
pub const BUTTON_RADIUS: f32 = 16.0;

use pomelo_widgets::preferences::ThemeMode;

/// The colours, from the original.
pub const BACKGROUND: Color = Color::from_rgb8(11, 15, 25); // AMOLED deep dark
pub const CARD: Color = Color::from_rgb8(17, 24, 39);
pub const CARD_BORDER: Color = Color::from_rgb8(30, 41, 59);
pub const TITLE: Color = Color::from_rgb8(6, 182, 212);
pub const FOOTER: Color = Color::from_rgb8(100, 116, 139);
pub const BUTTON: Color = Color::from_rgb8(2, 132, 199);
pub const BUTTON_PRESSED: Color = Color::from_rgb8(56, 189, 248);

/// Page background for the given theme mode.
pub fn background_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(243, 244, 246)
    } else {
        BACKGROUND
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

/// Title color for the given theme mode.
pub fn title_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(8, 145, 178)
    } else {
        TITLE
    }
}

/// Number color for the given theme mode.
pub fn number_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(17, 24, 39)
    } else {
        Color::WHITE
    }
}

/// Footer color for the given theme mode.
pub fn footer_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(156, 163, 175)
    } else {
        FOOTER
    }
}

/// Button color for the given theme mode.
pub fn button_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(2, 132, 199)
    } else {
        BUTTON
    }
}

/// Button pressed color for the given theme mode.
pub fn button_pressed_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(14, 165, 233)
    } else {
        BUTTON_PRESSED
    }
}
