//! What the file manager measures and colours with.
//!
//! Every size here comes out of one number — the base font size, `FONT_BASE` — for the reason the
//! other pages have: a row and the name in it are one drawing, and a row that stayed the same height
//! while the type grew would clip the name it exists to show.

use iced::Color;

use pomelo_widgets::preferences::{ThemeMode, FONT_BASE};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// The layout never reads it — an app fills whatever space it is given — with one exception: how many
/// characters of a name fit a row is a fact about the panel, and it is read in the tests so that "a
/// name runs off the edge of the screen" is a failing assertion rather than something noticed on a
/// photograph.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the rows.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 6.0;

/// The corner of everything on this page that has one: the rows, the toolbar and the prompt.
pub const RADIUS: f32 = 12.0;

/// Everything the page measures with, from one base font size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The height of one row in the list; tall enough to be a finger's target.
    pub row: f32,
    /// The names in the list and the buttons on the toolbar.
    pub text: f32,
    /// The sizes, the units and the path along the top.
    pub small: f32,
    /// The glyphs on the toolbar.
    pub icon: f32,
    /// The heading on the toolbar and the prompt.
    pub heading: f32,
}

impl Sizes {
    /// The measurements the base font size asks for.
    ///
    /// Every field is a multiple of `FONT_BASE`. A row is a little under twice the base — a finger's
    /// worth of height rather than a line's — and the secondary lines step down from the base so that
    /// a screenful of names fits the panel without the page having to scroll to say everything.
    pub fn new() -> Self {
        let base = FONT_BASE;

        Self {
            row: base * 1.85,
            text: base * 0.85,
            small: base * 0.66,
            icon: base * 1.05,
            heading: base * 1.05,
        }
    }
}

/// The ink the names are written in.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: the sizes and the path.
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

/// A row's slab, so that a screenful of names reads as a list rather than as a wall.
pub fn row(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.05),
        ThemeMode::Light => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.9),
    }
}

/// The row a finger has just picked out.
pub fn selected(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0x4A, 0x86, 0xE8, 0.28),
        ThemeMode::Light => Color::from_rgba8(0x4A, 0x86, 0xE8, 0.20),
    }
}

/// The page's own colour for something that is neither right nor wrong: a folder, the up button.
///
/// A blue rather than the green the meters use, because the only thing green on this page would be a
/// measurement, and a folder is not one.
pub fn accent() -> Color {
    Color::from_rgb8(0x4A, 0x86, 0xE8)
}

/// A folder's glyph: the accent, warmed a little so that a list of directories is not a wall of one
/// blue.
pub fn folder() -> Color {
    Color::from_rgb8(0xE8, 0xA3, 0x3D)
}

/// The colour of something that destroys: the delete button, and the prompt that asks about it.
pub fn danger() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row is a finger's target before it is a line of type.
    #[test]
    fn a_row_is_taller_than_the_type_in_it() {
        let sizes = Sizes::new();

        assert!(sizes.row > sizes.text, "a row has to be bigger than its name");
        assert!(sizes.row >= 30.0, "and big enough to hit");
    }

    /// Every field is its own multiple of the one base size, and the things meant to be small really
    /// are smaller than the things meant to be big.
    #[test]
    fn the_sizes_are_the_expected_multiples_of_the_base() {
        let sizes = Sizes::new();

        assert_eq!(sizes.row, FONT_BASE * 1.85);
        assert_eq!(sizes.text, FONT_BASE * 0.85);
        assert_eq!(sizes.small, FONT_BASE * 0.66);
        assert_eq!(sizes.icon, FONT_BASE * 1.05);
        assert_eq!(sizes.heading, FONT_BASE * 1.05);

        assert!(sizes.small < sizes.text);
        assert!(sizes.text < sizes.row);
    }

    /// A name has to fit beside the glyph and the size, or the row clips the one thing it is there to
    /// show.
    #[test]
    fn a_name_fits_the_panel() {
        let sizes = Sizes::new();

        // The glyph, its padding, and the size on the right take roughly a third of the row;
        // the rest is the name. Twenty-five glyphs is short of the longest names this board
        // holds, and short is the side to err on.
        let name = sizes.text * 0.55 * 25.0;

        assert!(
            name <= PANEL - 2.0 * MARGIN,
            "a name is {name} px wide, and the panel has {} px of room",
            PANEL - 2.0 * MARGIN
        );
    }
}
