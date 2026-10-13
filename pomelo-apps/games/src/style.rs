//! What the games pages measure and colour with.
//!
//! # The one page in this project whose layout is not a font tier
//!
//! Every other page scales its sizes from the font tier and lets the cards be as tall as they turn
//! out. This one cannot: a 2048 board is a square, and a square on a 480 px panel is 328 px whatever
//! somebody's text size is. So the board's own measurements are constants here, and the *chrome*
//! around it — the title, the caption, the buttons — comes out of the tier like everything else. The
//! panel has room for both, which is the arithmetic in [`BOARD`]'s comment.
//!
//! # The tiles are not themed
//!
//! The sixteen colours below are 2048's own, unchanged, in both the light theme and the dark one. A
//! board is *recognisable*: somebody who has played this game once knows that 128 is gold and that the
//! dark slab is a tile nobody has made yet, and a theme that recoloured them would be a theme that
//! made the game harder to read rather than easier. What follows the theme is the frame under them.

use iced::Color;

use pomelo_widgets::preferences::{FontSizeTier, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the page's own rows.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards, and the buttons.
pub const RADIUS: f32 = 12.0;

/// The side of one tile, and the room between two of them.
///
/// The whole board is `4 * TILE + 3 * TILE_GAP` = 304 px, and that number is what the rest of this
/// file is fitted to: 24 px of margin, the header at its tallest, two gaps and the pad's 48 come to
/// 465 of the panel's 480 — see [`the board fits the panel`](tests) for the arithmetic done properly.
/// The tile cannot be bigger without a header that grew, and a board that did not fit the panel would
/// be a board with a row cut off.
pub const TILE: f32 = 70.0;
pub const TILE_GAP: f32 = 8.0;

/// The whole board: four tiles and the three gaps between them.
pub const BOARD: f32 = TILE * 4.0 + TILE_GAP * 3.0;

/// The height of the pad's buttons. Four of them, each as wide as a tile, and their gaps are the
/// tiles' — so the pad is exactly as wide as the board and each arrow stands under the column or the
/// row it moves.
///
/// Shorter than a tile, because the pad is a control and not a cell: 48 px is above the 44 px a finger
/// wants, and the height it did not take is height the board kept.
pub const PAD: f32 = 48.0;

/// How tall iced draws a line of text of a given size.
///
/// Named because the page's arithmetic depends on it and because it is iced's number and not this
/// project's: the board's size is fitted to a header of two lines, and if iced ever changed this the
/// board would have to be re-fitted. It is here so that the change starts in one place.
pub const LINE: f32 = 1.2;

/// Everything the page's chrome measures with, for one font tier.
///
/// The board is not in here. See the module's own note: the tiles are the game's size and the text on
/// this page is the reader's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The title of a page, and the name of a game.
    pub title: f32,
    /// A caption, a button's label, a row's name in the list.
    pub text: f32,
    /// A line under a name in the list, and the score's label.
    pub small: f32,
}

impl Sizes {
    /// The measurements a font tier asks for.
    pub fn of(tier: FontSizeTier) -> Self {
        let base = tier.base_size();

        Self {
            title: base * 1.15,
            text: base * 0.9,
            small: base * 0.72,
        }
    }
}

/// How big the number in a tile is drawn.
///
/// Sized from the tile rather than from the font tier, and it is the one piece of text in this project
/// that is: the number has to fit the cell, and the cell is 76 px because the board is 328 px because
/// the panel is 480. A tile of `2048` at the size of a tile of `2` would touch both of its walls, so
/// the number steps down as it gains digits — which is what the real game does, and for this reason.
pub fn tile_text(value: u32) -> f32 {
    match value {
        0..=99 => TILE * 0.42,
        100..=999 => TILE * 0.36,
        _ => TILE * 0.28,
    }
}

/// The frame the tiles sit in.
pub fn board(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x15, 0x16, 0x1B),
        ThemeMode::Light => Color::from_rgb8(0xBB, 0xAD, 0xA0),
    }
}

/// An empty cell: a hole in the board rather than a tile, so it is the frame's own colour a step
/// towards the ink of the page.
pub fn empty(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.07),
        ThemeMode::Light => Color::from_rgb8(0xCD, 0xC1, 0xB4),
    }
}

/// A tile's colour and the ink on it, by the number it holds.
///
/// The empty board and the two smallest tiles are pale with dark ink on them; from 8 up the tiles are
/// saturated and the ink turns white, which is where the game's own palette turns it. Every value
/// past the last rung shares that last tile's dark slab, because there is no rung after the one the
/// game is named for — and a board that went on getting louder past 2048 would be a board whose
/// biggest tile was the least readable thing on it.
pub fn tile(value: u32, theme: ThemeMode) -> (Color, Color) {
    // The ink on a tile, in the game's own two colours: `0x776E65` on the pale ones, and the pale
    // cream of the board on the rest.
    let on_pale = Color::from_rgb8(0x77, 0x6E, 0x65);
    let on_saturated = Color::from_rgb8(0xF9, 0xF6, 0xF2);

    match value {
        0 => (empty(theme), on_pale),
        2 => (Color::from_rgb8(0xEE, 0xE4, 0xDA), on_pale),
        4 => (Color::from_rgb8(0xED, 0xE0, 0xC8), on_pale),
        8 => (Color::from_rgb8(0xF2, 0xB1, 0x79), on_saturated),
        16 => (Color::from_rgb8(0xF5, 0x95, 0x63), on_saturated),
        32 => (Color::from_rgb8(0xF6, 0x7C, 0x5F), on_saturated),
        64 => (Color::from_rgb8(0xF6, 0x5E, 0x3B), on_saturated),
        128 => (Color::from_rgb8(0xED, 0xCF, 0x72), on_saturated),
        256 => (Color::from_rgb8(0xED, 0xCC, 0x61), on_saturated),
        512 => (Color::from_rgb8(0xED, 0xC8, 0x50), on_saturated),
        1024 => (Color::from_rgb8(0xED, 0xC5, 0x3F), on_saturated),
        2048 => (Color::from_rgb8(0xED, 0xC2, 0x2E), on_saturated),
        _ => (Color::from_rgb8(0x3C, 0x3A, 0x32), on_saturated),
    }
}

/// The colour of a game that has been won: the gold of the tile the game is named for.
///
/// Named for [`tile`]'s reason: a line saying "you made 2048" should be the colour of the tile that
/// says it, and there is exactly one place in this project that decides what that colour is.
pub fn won() -> Color {
    Color::from_rgb8(0xED, 0xC2, 0x2E)
}

/// The ink for the things that are read: the title, a game's name, the arrows.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: the captions, the score's
/// label, the line under a game's name in the list.
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

/// A card's own slab: the header's buttons, and the list's rows.
pub fn card(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.07),
        ThemeMode::Light => Color::WHITE,
    }
}

/// The colour of a pad's arrow while a finger is on it.
pub fn pressed() -> Color {
    Color::from_rgb8(0x8F, 0x7A, 0x66)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board fits the panel, at the largest font tier, with the pad and both margins.
    ///
    /// The arithmetic in [`TILE`]'s comment, as a test rather than as a comment, because the way this
    /// breaks is somebody making the tiles a little bigger for a desktop window: on the panel it is
    /// the difference between a board and a board with its last row cut off. The largest tier is the
    /// one that matters — the board cannot grow with it, so everything else has to fit around it.
    #[test]
    fn the_board_fits_the_panel() {
        let sizes = Sizes::of(FontSizeTier::Large);

        // The header's height is the heading's two lines: a page's title and the line under it. The
        // buttons beside it are shorter than that at every tier, so they are not what the row measures.
        let header = sizes.title * LINE + 2.0 + sizes.small * LINE;
        let used = MARGIN * 2.0 + header + GAP * 2.0 + BOARD + PAD;

        assert!(
            used <= PANEL,
            "{used} px of chrome and board in {PANEL} px of panel"
        );
    }

    /// Four tiles and three gaps are the board, and four pad buttons are as wide as the board is.
    #[test]
    fn the_pad_lines_up_with_the_board() {
        assert_eq!(BOARD, TILE * 4.0 + TILE_GAP * 3.0);

        // The pad's four buttons, each `(BOARD - 3 * TILE_GAP) / 4` wide and the whole of it with the
        // same gaps as the tiles: the arrows stand under the columns they move.
        let button = (BOARD - TILE_GAP * 3.0) / 4.0;
        assert_eq!(button * 4.0 + TILE_GAP * 3.0, BOARD);
    }

    /// Every rung of the palette is its own colour, and past the last rung they stop.
    ///
    /// The second half is the rule: `4096` is a tile the game never promised to draw, and it is drawn
    /// as the darkest slab rather than as a fourteenth colour somebody invented.
    #[test]
    fn the_palette_is_a_rung_per_tile() {
        let theme = ThemeMode::Dark;
        let rungs = [2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048];

        for (at, value) in rungs.iter().enumerate() {
            for other in &rungs[at + 1..] {
                assert_ne!(
                    tile(*value, theme).0,
                    tile(*other, theme).0,
                    "{value} and {other} are drawn the same"
                );
            }
        }

        assert_eq!(tile(4096, theme).0, tile(8192, theme).0);
        assert_eq!(tile(4096, theme).0, tile(16384, theme).0);
    }

    /// An empty cell is not a tile: it is the board's own colour, in every theme.
    #[test]
    fn an_empty_cell_is_the_board_and_not_a_tile() {
        for theme in [ThemeMode::Dark, ThemeMode::Light] {
            assert_eq!(tile(0, theme).0, empty(theme));

            for value in [2, 4, 8, 2048] {
                assert_ne!(tile(0, theme).0, tile(value, theme).0);
            }
        }
    }

    /// The numbers on the tiles get smaller as they get longer, so that they fit the tile.
    #[test]
    fn a_longer_number_is_drawn_smaller() {
        assert!(tile_text(2) > tile_text(128));
        assert!(tile_text(128) > tile_text(2048));

        // And even the longest number the game can make fits its tile: five digits at the smallest
        // step, in a font whose digits are a little over half their size wide.
        assert!(tile_text(16_384) * 0.6 * 5.0 < TILE);
    }

    /// The two inks are two, and the win's gold is the tile it is named for.
    #[test]
    fn the_colours_that_mean_something_are_distinct() {
        let theme = ThemeMode::Dark;

        assert_ne!(ink(theme), muted(theme));
        assert_ne!(ink(theme), board(theme));
        assert_ne!(won(), ink(theme));
        assert_eq!(won(), tile(2048, theme).0, "the game is named for it");
    }
}
