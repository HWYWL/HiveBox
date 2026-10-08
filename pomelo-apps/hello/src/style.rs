//! What the signature is drawn on, and how it is framed.
//!
//! The four numbers are the original's, kept identical on purpose: two ports of the same
//! screensaver that frame it differently are two different pictures.

use iced::Color;

/// The panel the signature is drawn for, and the size the tests simulate.
pub const SCREEN: u32 = 480;

/// How much of the canvas's width the artwork takes.
pub const WIDTH_FRACTION: f32 = 0.86;

/// And how much of its height.
pub const HEIGHT_FRACTION: f32 = 0.56;

/// The narrowest the line may come out, in pixels. A thumbnail-sized canvas would otherwise have a
/// stroke too thin to see.
pub const MIN_STROKE: f32 = 2.0;

/// And the widest, so that a huge canvas does not turn the signature into a blob.
pub const MAX_STROKE: f32 = 60.0;

/// The pale wash behind the signature, on the left.
pub const WASH_START: Color = Color::from_rgb8(218, 244, 236);

/// And on the right. The original dithered between these two; see the canvas module for
/// what happens to the dithering here.
pub const WASH_END: Color = Color::from_rgb8(236, 228, 248);

use pomelo_widgets::preferences::ThemeMode;

/// The wash start color for the given theme.
pub fn wash_start_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(240, 249, 255)
    } else {
        WASH_START
    }
}

/// The wash end color for the given theme.
pub fn wash_end_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(245, 243, 255)
    } else {
        WASH_END
    }
}
