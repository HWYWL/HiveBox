//! Metrics and colours.
//!
//! The same numbers and the same named colours as the original player's `ui/theme.rs`,
//! `ui/title.rs`, `ui/vinyl.rs`, `ui/progress.rs` and `ui/controls.rs`, so that the two apps lay
//! out alike: absolute type, fluid bands. The playing screen's bands are the original's
//! `Expanded(flex:)` shares, 4 : 15 : 3 : 3, of what is left after the page's padding — with one
//! share moved: the volume row, which the original's controls band held, gets a band of its own and
//! the disc stands two short to pay for it. The disc has more room than it needs and the volume row
//! has nowhere else to fit. The list page has no bands at all: a head, and rows that scroll.

use iced::widget::slider;
use iced::{Border, Color};

/// The panel the player is designed for.
pub const SCREEN: u32 = 480;

/// The page: the original's `EdgeInsets::ltrb(0, 20, 0, 30)`.
pub const PAGE_TOP: f32 = 20.0;
pub const PAGE_BOTTOM: f32 = 30.0;

/// The playing screen's five bands: title, disc, progress, controls, volume.
///
/// The original's 4 : 15 : 3 : 3, with the volume row given a share of its own. The disc pays for it
/// and can afford to: it needs 128 of the 430 px the page has, and 11 of 25 is 172.
pub const TITLE_FLEX: u16 = 4;
pub const DISC_FLEX: u16 = 11;
pub const PROGRESS_FLEX: u16 = 3;
pub const CONTROLS_FLEX: u16 = 4;
pub const VOLUME_FLEX: u16 = 3;

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
/// above the timestamps.
///
/// The rail's measurements are what both draggable bars are drawn with — see [`BAR_TOUCH`] — and the
/// 4 px minimum an empty track used to show with is gone with the containers that drew it: the handle
/// at the left end of the rail says where a track starts, and does it by being there.
pub const BAR_WIDTH: f32 = 380.0;
/// The horizontal margin around the progress bar on the canonical panel (50.0 px).
pub const BAR_MARGIN_H: f32 = (SCREEN as f32 - BAR_WIDTH) / 2.0;
pub const BAR_HEIGHT: f32 = 6.0;
pub const BAR_RADIUS: f32 = 3.0;
pub const BAR_GAP: f32 = 8.0;
pub const TIME_FONT: f32 = 14.0;

/// How tall the *band* a bar is dragged in is, as opposed to the rail it draws.
///
/// The rail stays the original's 6 px, because that is what the bar looks like; 6 px is a mark to look
/// at, though, and not a thing to put a finger on. A slider is one widget with one box, so the box is
/// the finger's size and the rail is drawn inside it.
pub const BAR_TOUCH: f32 = 28.0;
/// The handle on a rail — round, and half again as wide as the rail is tall, so that a bar that can be
/// dragged looks like one.
pub const HANDLE_RADIUS: f32 = 9.0;
/// The ring around the handle, in the page's colour under the theme. Without it the handle is a purple
/// bump on a purple rail, and the only part of it that shows is the half hanging over the grey track.
pub const HANDLE_BORDER: f32 = 2.0;
/// One step of a seek, in seconds: finer than a finger can aim at on a 380 px rail — four minutes is
/// 240 s of bar — and coarse enough that the readout under it changes by a number a person can read.
pub const SEEK_STEP: f32 = 1.0;
/// One step of the level, in percent, on a shorter rail and for the same reason.
pub const VOLUME_STEP: u8 = 1;

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
/// The level bar in the volume row: what is left of the progress bar's width once the two buttons,
/// the readout and the three gaps between them are taken out of it. The row is that same 380 px,
/// which is what makes the two bars line up rather than nearly line up.
pub const VOLUME_BAR_WIDTH: f32 =
    BAR_WIDTH - VOLUME_BUTTON * 2.0 - VOLUME_READOUT - VOLUME_GAP * 3.0;

/// The back button on the playing screen: a small transport button, because that is what it is.
pub const BACK_BUTTON: f32 = 40.0;
pub const ICON_BACK: f32 = 22.0;
/// The glyph in each of the volume pair — smaller than the transport's, because the button is.
pub const ICON_VOLUME: f32 = 20.0;

/// The list page's head: tall enough for its type and no taller, so that the rows get the rest.
pub const HEADER_HEIGHT: f32 = 64.0;
/// One step down from the playing screen's 32 px title: the page a person is on is not the track.
pub const HEADER_FONT: f32 = 24.0;
/// A row's title: 18, the largest of the platform's baked sizes below the page titles, so that a long
/// name fits and a rasterisation is not paid per character.
pub const ROW_FONT: f32 = 18.0;

/// A track's row: 56 px is a finger's height, and the gap is the controls' 8 px, because the two are
/// both "this much apart".
pub const ROW_HEIGHT: f32 = 56.0;
pub const ROW_GAP: f32 = 8.0;
/// Rounded, and not `ROUND`: a row is a card, and the renderer would happily make it a pill.
pub const ROW_RADIUS: f32 = 12.0;
/// The row's own inside margin — enough to keep the words off the rounded edge.
pub const ROW_PADDING: f32 = 14.0;
/// The column the track's number sits in, which is also the width the playing mark takes over.
/// Fixed, so that every title starts at the same x whether it is preceded by `7` or by a glyph.
pub const ROW_INDEX_WIDTH: f32 = 28.0;
/// The playing mark in that column.
pub const ROW_MARK: f32 = 18.0;

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

/// The row of the track that is playing: the primary colour diluted.
///
/// The theme has no token for it and there is no border to draw with, so the mark has to be the
/// fill: the same purple as the progress bar and the play button, mixed most of the way into
/// whichever background it sits on. Pale on the light theme, deep on the dark one — marked, and not
/// shouted, in both.
pub fn row_playing_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((243, 232, 255))
    } else {
        rgb((60, 32, 84))
    }
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

/// The look of a bar that can be dragged: the same rail as the one this app used to paint by hand,
/// plus a round handle to take hold of.
///
/// The rail keeps the theme's two colours and the original's measurements — [`BAR_HEIGHT`] tall with
/// [`BAR_RADIUS`] — so that a glance at the playing screen still finds the same instrument in the same
/// place, and the theme still says what colour it is. What is new is that there is something on it:
/// a slider with nothing to grab is a bar that gives no sign it can be moved.
pub fn slider_style(theme: ThemeMode) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (primary().into(), track_for(theme).into()),
            width: BAR_HEIGHT,
            border: Border {
                radius: BAR_RADIUS.into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: HANDLE_RADIUS,
            },
            background: primary().into(),
            border_width: HANDLE_BORDER,
            border_color: background_for(theme),
        },
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}
