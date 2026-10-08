//! Metrics and colours.
//!
//! Two kinds of number live here, and the difference matters. It is the rule the original
//! calculator was laid out with, and this port had let it slip: a key's height was frozen at the
//! value the 480×480 panel was tuned with, so a taller window did not give taller keys — it gave
//! a band of empty page at the bottom.
//!
//! * **Shares of the page** (`*_SHARE`) — the *layout*. Every box takes its height from a flex
//!   weight whose value is the pixel value the design panel was tuned with, so the design is
//!   reproduced *exactly* at 480×480 and scales proportionally everywhere else: a taller window
//!   gives taller keys, a wider one wider keys. The page's weights are `20 : 130 : 10 : 300 : 20`,
//!   which is the panel — see `the_page_shares_are_the_design_panel`.
//! * **Absolute pixels** — the *type and the insets*: an 18 pt label, an 18 px card inset, a 16 px
//!   radius. Flutter's `fontSize` means the same thing on every window and so do these; resizing
//!   the window must not change them.

use iced::Color;

use crate::keys::{Kind, LAYOUT};

/// The panel the calculator is designed for.
pub const SCREEN: u32 = 480;

/// The band the design leaves above and below its content. Without it a shorter window would
/// stretch its content to the window and stop matching the design.
pub const BAND_SHARE: u16 = 20;

/// The display card.
///
/// 130 and not the 84 the keypad was tuned against: 20 + 130 + 10 + 300 + 20 is the 480 px panel
/// exactly, and a card that does not take its share is a page whose keys come out 59 px instead of
/// the 52 they were measured at.
pub const CARD_SHARE: u16 = 130;

/// The gap between the card and the keypad.
pub const CARD_GAP_SHARE: u16 = 10;

/// One key row.
pub const KEY_SHARE: u16 = 52;

/// The gap between two key rows.
pub const KEY_GAP_SHARE: u16 = 10;

/// The share of the page the whole keypad takes: one [`KEY_SHARE`] per row plus one
/// [`KEY_GAP_SHARE`] between each pair — 300 for the five rows of the design panel.
pub const KEYPAD_SHARE: u16 = keypad_share(LAYOUT.len());

/// [`KEYPAD_SHARE`] for a keypad of `rows` rows. A function so that the share cannot drift from the
/// table it describes.
const fn keypad_share(rows: usize) -> u16 {
    rows as u16 * KEY_SHARE + rows.saturating_sub(1) as u16 * KEY_GAP_SHARE
}

/// The outer padding of the page on all four sides (left, right, top, bottom).
pub const PAGE_PADDING: f32 = 20.0;
pub const PAGE_MARGIN: f32 = PAGE_PADDING;

/// The distance between two keys in a row. Carried by the cells rather than by spacers between
/// them: that is what keeps a row with fewer gaps — the `0` row — lined up with the rows above.
pub const COLUMN_GAP: f32 = 16.0;

/// The display card's insets: wide, but shallow. It holds a 34 pt line and a 14 pt one, which is
/// 64 px of content, so 18 px of padding on *every* side used to push the second line outside the
/// card's own height.
pub const CARD_PAD_H: f32 = 18.0;
pub const CARD_PAD_V: f32 = 10.0;
pub const CARD_RADIUS: f32 = 10.0;
pub const PRIMARY_FONT: f32 = 38.0;
pub const SECONDARY_FONT: f32 = 24.0;

/// The gap between the display's two lines.
pub const LINE_GAP: f32 = 6.0;

/// A key's corner radius and the size of its label.
pub const KEY_RADIUS: f32 = 16.0;
pub const KEY_FONT: f32 = 30.0;

use pomelo_widgets::preferences::ThemeMode;

/// The three colours one key paints with.
pub struct KeyPalette {
    pub fill: Color,
    pub pressed: Color,
    pub text: Color,
}

/// The colours for each role.
pub fn palette(kind: Kind) -> KeyPalette {
    palette_for(kind, ThemeMode::Dark)
}

/// The colours for each role under the given theme.
pub fn palette_for(kind: Kind, theme: ThemeMode) -> KeyPalette {
    if theme.is_light() {
        let (fill, pressed, text) = match kind {
            Kind::Function => ((212, 213, 218), (190, 192, 197), rgb((0, 0, 0))),
            Kind::Number => ((255, 255, 255), (230, 230, 235), rgb((0, 0, 0))),
            Kind::Operator | Kind::Equals => ((255, 149, 0), (220, 130, 0), Color::WHITE),
        };
        KeyPalette {
            fill: rgb(fill),
            pressed: rgb(pressed),
            text,
        }
    } else {
        let (fill, pressed) = match kind {
            Kind::Function => ((44, 44, 46), (58, 58, 60)),
            Kind::Number => ((28, 28, 30), (44, 44, 46)),
            Kind::Operator | Kind::Equals => ((255, 159, 10), (255, 179, 64)),
        };

        KeyPalette {
            fill: rgb(fill),
            pressed: rgb(pressed),
            text: Color::WHITE,
        }
    }
}

/// The page background.
pub fn background() -> Color {
    background_for(ThemeMode::Dark)
}

/// The page background for the given theme.
pub fn background_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((242, 242, 247))
    } else {
        Color::BLACK
    }
}

/// The display card.
pub fn card() -> Color {
    card_for(ThemeMode::Dark)
}

/// The display card for the given theme.
pub fn card_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((255, 255, 255))
    } else {
        rgb((18, 18, 20))
    }
}

/// The primary text colour for the display.
pub fn text_primary_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((0, 0, 0))
    } else {
        Color::WHITE
    }
}

/// The secondary expression colour for the display.
pub fn text_secondary_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((142, 142, 147))
    } else {
        rgb((174, 174, 178))
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shares are the design's pixel values, so they have to add up to the panel: that is what
    /// "exact at 480, proportional everywhere else" means, and it is the first thing to break when
    /// a number is edited.
    #[test]
    fn the_page_shares_are_the_design_panel() {
        let page = (2.0 * PAGE_PADDING) as u16 + CARD_SHARE + CARD_GAP_SHARE + KEYPAD_SHARE;

        assert_eq!(
            page, SCREEN as u16,
            "the page's shares no longer add up to the design panel ({page} vs {SCREEN})"
        );
    }

    /// The card's two lines have to fit between its insets, or the second one is drawn outside the
    /// card and clipped away — which is what happened while the padding was 18 px on every side.
    #[test]
    fn the_card_can_hold_both_of_its_lines() {
        // iced lays a text out at 1.2 of its size, which is where these two numbers come from.
        let primary = (PRIMARY_FONT * 1.2).ceil();
        let secondary = (SECONDARY_FONT * 1.2).ceil();
        let content = primary + LINE_GAP + secondary;
        let room = CARD_SHARE as f32 - 2.0 * CARD_PAD_V;

        assert!(
            content <= room,
            "the card's two lines need {content} px and the card gives them {room}"
        );
    }

    #[test]
    fn equals_key_has_same_palette_as_operator() {
        let op_dark = palette_for(Kind::Operator, ThemeMode::Dark);
        let eq_dark = palette_for(Kind::Equals, ThemeMode::Dark);
        assert_eq!(op_dark.fill, eq_dark.fill);
        assert_eq!(op_dark.pressed, eq_dark.pressed);
        assert_eq!(op_dark.text, eq_dark.text);

        let op_light = palette_for(Kind::Operator, ThemeMode::Light);
        let eq_light = palette_for(Kind::Equals, ThemeMode::Light);
        assert_eq!(op_light.fill, eq_light.fill);
        assert_eq!(op_light.pressed, eq_light.pressed);
        assert_eq!(op_light.text, eq_light.text);
    }
}
