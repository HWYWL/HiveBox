//! What the update page measures and colours with.
//!
//! Two rows of the same shape, so almost nothing here is a size of its own: the page is a heading, two
//! cards that are interchangeable, and a line under them saying what happened. The one thing that is
//! *not* a caption is the version — a person opens this page to read a number and compare it with
//! another — so it is drawn at the base size and in the page's own colour, which is the same argument
//! the forecast page makes about its temperature at a different scale.

use iced::Color;

use pomelo_widgets::preferences::{FONT_BASE, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the cards.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards and the buttons.
pub const RADIUS: f32 = 12.0;

/// How wide a row's name column is, so that the two versions line up down the page.
///
/// `固件` against `Firmware`, `字库` against `Glyph table`: the names are not the same length in either
/// language, and two rows whose versions started wherever their name ended would be two rows nobody
/// could compare — which is the one thing this page is for.
pub const NAME: f32 = 132.0;

/// How much room a row keeps for its button, whatever the rest of the row is doing.
///
/// Fixed rather than "as wide as its label", and this is a *reservation*: everything to the left of it
/// is a version, and a version is a string whose length this page does not choose — an English "Update
/// to v0.7.0" is half again as long as the Chinese one. A button that took only what was left would be
/// a button whose label wrapped onto a second line on the row with the longest neighbour, which is
/// exactly what the glyph table's row did before this was here.
pub const ACTION: f32 = 116.0;

/// How wide the prompt is.
///
/// Narrower than [`PANEL`], so that the page it is over is still visible around it — a prompt that
/// covered the panel would be a second page rather than something said about the first one.
pub const PROMPT: f32 = 316.0;

/// Everything the page measures with, at the base font size [`FONT_BASE`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The heading, and the versions themselves.
    pub title: f32,
    /// A row's name, and the sentence under it.
    pub text: f32,
    /// The buttons, and the line at the bottom of the page.
    pub small: f32,
    /// The manifest's own details: a file name, a size.
    pub tiny: f32,
    /// The glyph at the left of a row.
    pub glyph: f32,
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
        }
    }
}

/// The ink for the things that are read: the heading, the names, the versions.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: the captions, the labels.
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

/// A card's own slab, a step up from the page so that two of them read as two things.
pub fn card(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.07),
        ThemeMode::Light => Color::WHITE,
    }
}

/// The page's own colour: the heading's glyph, and the version a row is offering.
///
/// The updater's own indigo — the tile on the grid wears the same one — and not the fault red or the
/// current green: a newer version is not a warning and not a good thing either, it is a thing that is
/// there to be taken or left.
pub fn accent() -> Color {
    Color::from_rgb8(0x3F, 0x51, 0xB5)
}

/// The accent under a finger.
///
/// A shade of its own, because the one button drawn in the accent is *always* drawn in it: every other
/// button on this page changes its fill when it is pressed, and a press has to be visible on a fill that
/// does not change. Darker rather than lighter, which is the direction a press already goes on the two
/// buttons that swap the accent in.
pub fn accent_pressed() -> Color {
    Color::from_rgb8(0x33, 0x42, 0x94)
}

/// Something is up to date: a row that has nothing to offer.
///
/// Green, and drawn with it only when a check has *said* so: a row that draws the same green before
/// anything has been asked would be a row claiming to be current on no evidence, which is what
/// [`Status::Idle`](crate::Status::Idle) exists to avoid.
pub fn good() -> Color {
    Color::from_rgb8(0x36, 0xA3, 0x6B)
}

/// Something the page could not do: a fault line, and the glyph beside it.
pub fn bad() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

/// The wash over the page while the prompt is up.
///
/// Black at less than half, so that the page is dimmed and still *there*: a prompt is something said
/// about the page under it, and the two rows it is about stay legible enough to be the context of the
/// sentence. It is also what makes the prompt modal — a press lands on this, and the rows stop being
/// something that can be pressed twice.
pub fn backdrop() -> Color {
    Color::from_rgba8(0, 0, 0, 0.45)
}

/// The groove a download's bar runs in: a step *below* the prompt it sits on.
///
/// Darker than the prompt rather than lighter, and darker in both themes, because the bar drawn in it
/// is the page's own [`accent`]: a track that was a step up would read as the filled part and leave the
/// accent looking like the hole.
pub fn track() -> Color {
    Color::from_rgba8(0, 0, 0, 0.28)
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

    /// The three colours that mean something mean three different things.
    #[test]
    fn the_meaning_colours_are_three_colours() {
        let colours = [accent(), good(), bad()];

        assert_ne!(accent(), good());
        assert_ne!(good(), bad());
        assert_ne!(bad(), accent());
        assert_eq!(colours.len(), 3);
    }

    /// The accent's pressed shade is a shade of the accent, so that a press shows on the one button
    /// whose fill does not otherwise change.
    #[test]
    fn the_accent_has_a_pressed_shade_of_its_own() {
        assert_ne!(accent(), accent_pressed());
    }
}
