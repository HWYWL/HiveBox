//! The keyboard's colours, by key role.
//!
//! The iOS dark keyboard palette, copied from the original keyboard's `theme.rs`: a letter key, a
//! modifier key, the return key, and the shift key while caps is on.

use iced::Color;

use super::keys::KeyKind;
use crate::preferences::ThemeMode;

/// The keyboard band's own background (#1C1C1E). The keys sit on it; the page behind them does not
/// show through.
pub fn background() -> Color {
    background_for(ThemeMode::Dark)
}

/// The keyboard band's background for `theme`.
pub fn background_for(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => rgb((28, 28, 30)),
        ThemeMode::Light => rgb((209, 213, 219)),
    }
}

/// The keyboard's key radius.
pub const KEY_RADIUS: f32 = 5.0;

/// The three colours one key paints with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyPalette {
    /// The key at rest.
    pub fill: Color,
    /// The key under a finger.
    pub pressed: Color,
    /// The key's label.
    pub text: Color,
}

/// The iOS dark keyboard palette, by key role.
pub fn palette(kind: KeyKind) -> KeyPalette {
    palette_for(kind, ThemeMode::Dark)
}

/// The iOS keyboard palette, by key role and theme mode.
pub fn palette_for(kind: KeyKind, theme: ThemeMode) -> KeyPalette {
    let (fill, pressed, text) = match (kind, theme) {
        // Dark theme:
        (KeyKind::Character, ThemeMode::Dark) => ((72, 72, 74), (110, 110, 115), (255, 255, 255)),
        (KeyKind::Special, ThemeMode::Dark) => ((44, 44, 46), (72, 72, 74), (255, 255, 255)),
        (KeyKind::Return, ThemeMode::Dark) => ((0, 122, 255), (60, 150, 255), (255, 255, 255)),
        (KeyKind::ShiftActive, ThemeMode::Dark) => ((255, 255, 255), (110, 110, 115), (0, 0, 0)),

        // Light theme (iOS style light keyboard):
        (KeyKind::Character, ThemeMode::Light) => ((255, 255, 255), (230, 230, 230), (0, 0, 0)),
        (KeyKind::Special, ThemeMode::Light) => ((174, 179, 190), (195, 200, 210), (0, 0, 0)),
        (KeyKind::Return, ThemeMode::Light) => ((0, 122, 255), (60, 150, 255), (255, 255, 255)),
        (KeyKind::ShiftActive, ThemeMode::Light) => ((0, 0, 0), (60, 60, 60), (255, 255, 255)),
    };

    KeyPalette {
        fill: rgb(fill),
        pressed: rgb(pressed),
        text: rgb(text),
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}
