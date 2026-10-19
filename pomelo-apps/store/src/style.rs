//! What the store measures and colours with.
//!
//! The updater's page and this one are the same kind of page — a list of rows, a button each, and a
//! sentence at the bottom — so almost every number here is the updater's, restated rather than
//! shared: the two apps are their own crates, and a palette crate would be a third thing to keep in
//! step with both. What differs is the accent, and that difference is deliberate: the store's green
//! is "take this", the updater's indigo is "there is a newer one", and a person who has both open
//! should not have to read a heading to tell which page they are on.

use iced::Color;

use pomelo_widgets::preferences::{FONT_BASE, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the rows.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards and the buttons.
pub const RADIUS: f32 = 12.0;

/// How wide a row's action button is, whatever the rest of the row is doing.
///
/// A reservation rather than "as wide as its label": the sentence to the left of it is a name, a
/// version and a size, none of which this page chooses the length of, and a button that took only
/// what was left would wrap on the row with the longest name.
pub const ACTION: f32 = 92.0;

/// The square the section's glyph or an application's accent sits in, at the left of a row.
pub const BADGE: f32 = 40.0;

/// How wide the prompt is: narrower than [`PANEL`], so the page under it is still visible around it.
pub const PROMPT: f32 = 316.0;

/// Everything the page measures with, at the base font size [`FONT_BASE`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The heading.
    pub title: f32,
    /// A row's name.
    pub text: f32,
    /// A row's second line with the version, the size and the description.
    pub small: f32,
    /// The manifest's own details, and the line at the bottom of the page.
    pub tiny: f32,
    /// The glyph at the left of a row.
    pub glyph: f32,
    /// The first character drawn in an accent square.
    pub badge: f32,
}

impl Sizes {
    /// The measurements the base font size asks for: [`FONT_BASE`], with each field a multiple of it.
    pub fn new() -> Self {
        let base = FONT_BASE;

        Self {
            title: base,
            text: base * 0.9,
            small: base * 0.72,
            tiny: base * 0.58,
            glyph: base * 1.3,
            badge: base * 0.95,
        }
    }
}

/// The ink for the things that are read: the heading, names, versions.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read.
pub fn muted(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x9A, 0x9C, 0xAD),
        ThemeMode::Light => Color::from_rgb8(0x6E, 0x70, 0x7C),
    }
}

/// The page's own surface: a slab *of* the theme's background rather than a colour of its own.
pub fn surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.04),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.03),
    }
}

/// A row's own slab, a step up from the page so that rows read as separate things.
pub fn card(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.07),
        ThemeMode::Light => Color::WHITE,
    }
}

/// A section's selected tab: the page's colour, at low strength, so the three read as one control.
pub fn tab_on(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0x2E, 0x9E, 0x7B, 0.28),
        ThemeMode::Light => Color::from_rgba8(0x2E, 0x9E, 0x7B, 0.16),
    }
}

/// The store's own colour: the heading's glyph, a tab that is chosen, and "install".
pub fn accent() -> Color {
    Color::from_rgb8(0x2E, 0x9E, 0x7B)
}

/// The accent under a finger: the one button drawn filled does not otherwise change its fill.
pub fn accent_pressed() -> Color {
    Color::from_rgb8(0x25, 0x7F, 0x62)
}

/// Something is up to date, or finished: a row that has nothing to offer, an install that landed.
pub fn good() -> Color {
    Color::from_rgb8(0x36, 0xA3, 0x6B)
}

/// Something the page could not do.
pub fn bad() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

/// The wash over the page while the prompt is up.
pub fn backdrop() -> Color {
    Color::from_rgba8(0, 0, 0, 0.45)
}

/// The groove a download's bar runs in.
pub fn track() -> Color {
    Color::from_rgba8(0, 0, 0, 0.28)
}

/// The colour of a row's badge when the package has no icon: its own `accent`, or a grey.
///
/// `#RRGGBB`, and the caller's fallback when the manifest wrote something else. Parsed here and not
/// in the manifest reader because a colour is a *drawing* fact: a store that refused a package over
/// a malformed accent would be refusing an application over a tile.
pub fn accent_of(hex: &str, fallback: Color) -> Color {
    let Some(digits) = hex.strip_prefix('#') else {
        return fallback;
    };

    if digits.len() != 6 {
        return fallback;
    }

    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();

    match (channel(0), channel(2), channel(4)) {
        (Some(r), Some(g), Some(b)) => Color::from_rgb8(r, g, b),
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every size is the expected multiple of the base, and the steps stay in order.
    #[test]
    fn the_sizes_are_multiples_of_the_base_font_size() {
        let size = Sizes::new();

        assert_eq!(size.title, FONT_BASE);
        assert_eq!(size.text, FONT_BASE * 0.9);
        assert_eq!(size.small, FONT_BASE * 0.72);
        assert_eq!(size.tiny, FONT_BASE * 0.58);
        assert_eq!(size.glyph, FONT_BASE * 1.3);

        assert!(size.tiny < size.small);
        assert!(size.small < size.text);
        assert!(size.text < size.title);
    }

    /// An accent is read as the manifest writes it, and anything else is the caller's grey rather
    /// than a colour this page made up.
    #[test]
    fn an_accent_is_read_or_replaced_by_the_fallback() {
        let grey = Color::from_rgb8(0x7C, 0x83, 0x92);

        assert_eq!(accent_of("#F69E22", grey), Color::from_rgb8(0xF6, 0x9E, 0x22));
        assert_eq!(accent_of("F69E22", grey), grey);
        assert_eq!(accent_of("#F69E2", grey), grey);
        assert_eq!(accent_of("#GGGGGG", grey), grey);
        assert_eq!(accent_of("", grey), grey);
    }

    /// The accent's pressed shade is a shade of the accent, so that a press shows on the one button
    /// whose fill does not otherwise change.
    #[test]
    fn the_accent_has_a_pressed_shade_of_its_own() {
        assert_ne!(accent(), accent_pressed());
    }
}
