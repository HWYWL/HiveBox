//! Metrics and colours.
//!
//! The same numbers and the same named colours as the original player's `ui/theme.rs`,
//! `ui/title.rs`, `ui/vinyl.rs`, `ui/progress.rs` and `ui/controls.rs`, so that the two apps lay
//! out alike: absolute type, fluid bands. The page's four bands are the original's
//! `Expanded(flex:)` shares, 4 : 15 : 3 : 3, of what is left after the page's padding.

use iced::Color;

/// The panel the player is designed for.
pub const SCREEN: u32 = 480;

/// The page: the original's `EdgeInsets::ltrb(0, 20, 0, 30)`.
pub const PAGE_TOP: f32 = 20.0;
pub const PAGE_BOTTOM: f32 = 30.0;

/// The four bands: title, disc, progress, controls.
pub const TITLE_FLEX: u16 = 4;
pub const DISC_FLEX: u16 = 14;
pub const PROGRESS_FLEX: u16 = 3;
pub const CONTROLS_FLEX: u16 = 4;

/// The title band.
pub const TITLE_FONT: f32 = 32.0;
/// The disc: the original's 128 px album-art box, its inner label and the spindle at the centre.
pub const DISC_SIZE: f32 = 128.0;
pub const LABEL_SIZE: f32 = 64.0;
pub const SPINDLE_SIZE: f32 = 10.0;

/// The groove marker that makes the rotation visible, and how far out from the centre it orbits.
///
/// It replaces the baked album-art bitmap the original player blitted: that is an image, and
/// iced's `image` feature would pull the `image` crate into the firmware for one picture.
pub const MARKER_SIZE: f32 = 8.0;
pub const MARKER_ORBIT: f32 = 46.0;

/// The progress bar: the original's 380 px of a 480 px panel, 6 px tall with a 3 px radius, 8 px
/// above the timestamps, and never thinner than 4 px so an empty track still shows where it starts.
pub const BAR_WIDTH: f32 = 380.0;
/// The horizontal margin around the progress bar on the canonical panel (50.0 px).
pub const BAR_MARGIN_H: f32 = (SCREEN as f32 - BAR_WIDTH) / 2.0;
pub const BAR_HEIGHT: f32 = 6.0;
pub const BAR_RADIUS: f32 = 3.0;
pub const BAR_GAP: f32 = 8.0;
pub const BAR_MIN_FILL: f32 = 4.0;
pub const TIME_FONT: f32 = 14.0;

/// The controls: the original's 48 / 60 / 48 px round buttons, the gap between them, and the
/// volume pair with its readout.
pub const BUTTON_SMALL: f32 = 48.0;
pub const BUTTON_PLAY: f32 = 60.0;
pub const ICON_SMALL: f32 = 26.0;
pub const ICON_PLAY: f32 = 32.0;
pub const BUTTON_FONT: f32 = 15.0;
pub const BUTTON_GAP: f32 = 20.0;
pub const VOLUME_BUTTON: f32 = 36.0;
pub const VOLUME_GAP: f32 = 6.0;
pub const VOLUME_READOUT: f32 = 52.0;
// 15 and not 16: the platform's baked sizes are 14 / 15 / 18, and a readout one pixel off one of
// them pays a glyph rasterisation per character the first time it is drawn.
pub const VOLUME_FONT: f32 = 15.0;

/// A radius no box this app draws is half as wide as.
///
/// The renderer clamps a border radius to half the box (`iced_tiny_skia::engine::draw_quad`), so
/// this means "as round as the box allows" — which is how a container becomes a disc, a label or a
/// button without a drawing primitive of our own.
pub const ROUND: f32 = 1000.0;

use pomelo_widgets::preferences::ThemeMode;

/// The page background: the theme's `BG_COLOR`.
pub fn background() -> Color {
    background_for(ThemeMode::Light)
}

/// The page background for the given theme mode.
pub fn background_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((255, 255, 255))
    } else {
        rgb((18, 18, 20))
    }
}

/// The title: the theme's `TITLE_COLOR`.
pub fn title() -> Color {
    title_for(ThemeMode::Light)
}

/// The title for the given theme mode.
pub fn title_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((17, 24, 39))
    } else {
        Color::WHITE
    }
}

/// Secondary text: the theme's `TEXT_GRAY`.
pub fn text_gray() -> Color {
    text_gray_for(ThemeMode::Light)
}

/// Secondary text for the given theme mode.
pub fn text_gray_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((107, 114, 128))
    } else {
        rgb((156, 163, 175))
    }
}

/// The disk face: the theme's `VINYL_OUTER`.
pub fn vinyl_outer() -> Color {
    rgb((26, 26, 30))
}

/// One groove on it: the theme's `VINYL_GROOVE`.
pub fn vinyl_groove() -> Color {
    rgb((42, 42, 48))
}

/// The label at the centre, while a track plays: the theme's `VINYL_LABEL_PLAYING`.
pub fn label_playing() -> Color {
    primary()
}

/// The label at the centre, otherwise: the theme's `VINYL_LABEL_PAUSED`.
pub fn label_paused() -> Color {
    rgb((156, 163, 175))
}

/// The spindle: the theme's `VINYL_SPINDLE`.
pub fn spindle() -> Color {
    rgb((255, 255, 255))
}

/// The playback progress, and the play/pause button: the theme's `PRIMARY_PURPLE`.
///
/// The original painted all three playback buttons the same grey and drew a play/pause *icon*
/// inside the middle one. There is no icon font here, and three identical grey discs are also
/// three indistinguishable buttons on the panel, so the primary action takes the theme's primary
/// colour and its neighbours stay grey.
pub fn primary() -> Color {
    rgb((168, 40, 255))
}

/// The play/pause button while a finger is on it.
pub fn primary_pressed() -> Color {
    rgb((196, 116, 255))
}

/// The grey of the previous and next buttons: the theme's `BTN_BG_GRAY`.
pub fn button_bg() -> Color {
    button_bg_for(ThemeMode::Light)
}

/// The grey of the previous and next buttons for the given theme mode.
pub fn button_bg_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((243, 244, 246))
    } else {
        rgb((38, 38, 42))
    }
}

/// And while a finger is on one.
pub fn button_pressed() -> Color {
    button_pressed_for(ThemeMode::Light)
}

/// And while a finger is on one for the given theme mode.
pub fn button_pressed_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((229, 231, 235))
    } else {
        rgb((58, 58, 62))
    }
}

/// The volume buttons: the theme's `PROGRESS_TRACK_BG`, deliberately a different grey from the
/// playback buttons so that "the grey discs" means the three playback controls and nothing else.
pub fn volume_bg() -> Color {
    rgb((229, 231, 235))
}

/// And while a finger is on one.
pub fn volume_pressed() -> Color {
    rgb((156, 163, 175))
}

/// An icon or label on a button: the theme's `BTN_ICON_GRAY`.
pub fn button_icon() -> Color {
    button_icon_for(ThemeMode::Light)
}

/// An icon or label on a button for the given theme mode.
pub fn button_icon_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((75, 85, 99))
    } else {
        rgb((220, 220, 225))
    }
}

/// The unfilled part of the progress bar: the theme's `PROGRESS_TRACK_BG`.
pub fn track() -> Color {
    track_for(ThemeMode::Light)
}

/// The unfilled part of the progress bar for the given theme mode.
pub fn track_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((229, 231, 235))
    } else {
        rgb((44, 44, 48))
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}
