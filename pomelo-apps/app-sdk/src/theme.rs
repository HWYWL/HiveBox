//! The interface's colours, and the one operation on them an application needs.
//!
//! These are the values the launcher draws with: a pure black page, cards a step above it, white
//! text and a muted grey for the second line. An application that uses them looks like the rest of
//! the box, and a package that wants its own palette has its accent in its metadata.

use crate::ui::{rgb, Color};

/// The page behind everything, which is the colour of the panel's own border.
pub const BLACK: Color = 0x0000_0000;

/// A card or a row.
pub const SURFACE: Color = 0x0016_161A;

/// A card a step above the page: pressed, selected, or a row inside a card.
pub const SURFACE_HIGH: Color = 0x0024_242A;

/// The hairline that separates two groups.
pub const DIVIDER: Color = 0x002E_2E36;

/// Primary text.
pub const TEXT: Color = 0x00FF_FFFF;

/// Secondary text: labels, units, the line under a title.
pub const MUTED: Color = 0x009A_A0_AA;

/// The colour of a thing that is not right.
pub const DANGER: Color = 0x00E5_484D;

/// The colour of a thing that is.
pub const OK: Color = 0x0030_A4_6C;

/// The store's and the settings' accent, when a package or a page has no colour of its own.
pub const ACCENT: Color = 0x003F_51B5;

/// A colour mixed towards black, for a pressed state.
///
/// `amount` is 0–255, where 0 leaves the colour alone and 255 is black. Integer arithmetic on
/// purpose: this runs in an interpreter and a float multiply per pixel of a rectangle is a cost an
/// application should not pay for a shade.
pub fn shade(color: Color, amount: u8) -> Color {
    let keep = 255 - amount as u32;
    let red = ((color >> 16) & 0xFF) * keep / 255;
    let green = ((color >> 8) & 0xFF) * keep / 255;
    let blue = (color & 0xFF) * keep / 255;

    rgb(red as u8, green as u8, blue as u8)
}

/// A colour mixed towards white, for a highlight.
pub fn tint(color: Color, amount: u8) -> Color {
    let add = amount as u32;
    let red = (((color >> 16) & 0xFF) * (255 - add) / 255 + add).min(255);
    let green = (((color >> 8) & 0xFF) * (255 - add) / 255 + add).min(255);
    let blue = ((color & 0xFF) * (255 - add) / 255 + add).min(255);

    rgb(red as u8, green as u8, blue as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shade_moves_towards_black_and_a_tint_towards_white() {
        assert_eq!(shade(0x00FF_FFFF, 0), 0x00FF_FFFF);
        assert_eq!(shade(0x00FF_FFFF, 255), 0);
        assert_eq!(tint(0, 255), 0x00FF_FFFF);
        assert!(shade(0x0080_8080, 128) < 0x0080_8080);
        assert!(tint(0x0080_8080, 128) > 0x0080_8080);
    }
}
