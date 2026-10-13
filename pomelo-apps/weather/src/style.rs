//! What the forecast page measures and colours with.
//!
//! Every size comes out of one number — the font tier the system is set to — for the reason the other
//! pages have: the temperature is *the* thing on this page, and everything else is a caption under it.
//! A page whose cards kept their size while the numbers in them grew would read as a page of equally
//! important numbers, which is the one thing a forecast is not.

use iced::Color;

use pomelo_widgets::preferences::{FontSizeTier, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// Only the tests read it, and they read it for one thing: whether a day's row — a label, a glyph, a
/// name and two temperatures — still fits across the panel at the largest tier.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the cards.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards and the button.
pub const RADIUS: f32 = 12.0;

/// How wide the label at the left of a day's row is.
///
/// Sized for `今天` and for the longest date the page falls back to, `10-16`, at the largest tier:
/// a gutter that clipped either would clip the only thing saying which day the row is about.
pub const DAY_LABEL: f32 = 62.0;

/// How wide the condition's name is on a day's row, so the two temperatures line up down the card.
///
/// The names are not the same length in either language — `Partly cloudy` against `多云` — and a card
/// whose temperatures wandered with the weather would be a card nobody could read down.
pub const DAY_NAME: f32 = 108.0;

/// Everything the page measures with, for one font tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The temperature, which is the one number on the page read from across a room.
    pub reading: f32,
    /// The condition's glyph beside it.
    pub glyph: f32,
    /// The card titles, the day names and the two temperatures of a day.
    pub text: f32,
    /// The city, the captions, the readings under the temperature.
    pub small: f32,
    /// The units and the service's name, which are there to be found rather than read.
    pub tiny: f32,
}

impl Sizes {
    /// The measurements a font tier asks for.
    ///
    /// The multipliers are the whole of the design: the temperature is twice the base because it is the
    /// page, and the rest steps down from the base so that five day rows, their labels and the card
    /// around them fit the panel.
    pub fn of(tier: FontSizeTier) -> Self {
        let base = tier.base_size();

        Self {
            reading: base * 2.0,
            glyph: base * 1.5,
            text: base * 0.9,
            small: base * 0.72,
            tiny: base * 0.58,
        }
    }
}

/// The ink for the things that are read: the temperature, the city, the day names.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: the captions, the units.
pub fn muted(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x9A, 0x9C, 0xAD),
        ThemeMode::Light => Color::from_rgb8(0x6E, 0x70, 0x7C),
    }
}

/// The page's own surface: a slab *of* the theme's background rather than a colour of its own, so the
/// page and the desktop it came from are the same dark (or the same light).
pub fn surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.04),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.03),
    }
}

/// A card's own slab, a step up from the page so that three of them read as three things.
pub fn card(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.07),
        ThemeMode::Light => Color::WHITE,
    }
}

/// The page's own colour: the temperature, and the glyph beside it.
///
/// A sky blue, and the one colour on the page that is not a meaning: the two temperatures in a day's
/// row have colours of their own ([`high`] and [`low`]), and the reading at the top is neither of them.
pub fn accent() -> Color {
    Color::from_rgb8(0x3D, 0x8B, 0xDE)
}

/// A day's high: warm, and the same warm whatever the number is.
///
/// The colour is not a scale. A gradient from blue to red over the range of a year would make two
/// days look different in kind when they differ by a degree — and a day of `-5` and a day of `-25` are
/// both "cold", which is what a person does with the number.
pub fn high() -> Color {
    Color::from_rgb8(0xE0, 0x7A, 0x45)
}

/// A day's low: cool, by the same argument as [`high`].
pub fn low() -> Color {
    Color::from_rgb8(0x4E, 0x9B, 0xD8)
}

/// Something the page could not do: a fault line, and the glyph beside it.
pub fn bad() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every size a tier asks for grows with the tier, and the temperature stays the biggest thing.
    #[test]
    fn the_sizes_follow_the_tier() {
        let small = Sizes::of(FontSizeTier::Small);
        let large = Sizes::of(FontSizeTier::Large);

        assert!(small.reading < large.reading);
        assert!(small.tiny < small.small, "the smallest step is still a step");
        assert!(small.reading > small.glyph, "and the reading is the biggest");
        assert!(small.glyph > small.text);
    }

    /// The colours that mean something on this page mean four different things.
    ///
    /// A day's high, a day's low, the reading and a fault are four judgements, and two of them sharing
    /// a colour would be two judgements the page draws as one.
    #[test]
    fn the_four_meaning_colours_are_four_colours() {
        let colours = [accent(), high(), low(), bad()];

        for (at, one) in colours.iter().enumerate() {
            for other in &colours[at + 1..] {
                assert_ne!(one, other);
            }
        }
    }
}
