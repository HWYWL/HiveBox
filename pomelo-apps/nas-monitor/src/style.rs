//! What the NAS page measures and colours with.
//!
//! Every size comes out of one number — [`FONT_BASE`], the base font size — for the reason a table
//! has: columns that do not move when the text does are columns whose numbers run into each other. The
//! colours are the other half, and they are *per metric* rather than per fullness: a ring on a
//! dashboard is identified by its colour before its label is read, which is how a person tells the CPU
//! from the memory at a glance. That is the settings app's argument for its rings, and this page keeps
//! it for its bars.

use iced::Color;

use pomelo_widgets::preferences::{FONT_BASE, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// The layout never reads it — an app fills whatever it is given — but the disk table's column widths
/// are checked against it in a test, which is the one thing that can go wrong with fixed columns.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the parts of the page.
pub const MARGIN: f32 = 14.0;
pub const GAP: f32 = 10.0;

/// How far the header's content stays from the panel's left and right edges.
///
/// The panel is a square with rounded corners and the header is the page's topmost row, so its two
/// ends land in the cut: the machine's name on the left, the uptime on the right, both within a few
/// pixels of the glass. The launcher's status bar is that same row over those same corners and keeps
/// `GUTTER + 7` clear — see `app-launcher`'s `STATUS_INSET`, where the number is reasoned out — and an
/// uptime read against the bezel is worth the same room here. It is wider than [`MARGIN`], so the
/// header is inset a little past the cards under it; the launcher's bar sits past its grid the same way.
pub const HEADER_INSET: f32 = 27.0;
/// The room inside a card and inside a table row, on each side of its text.
pub const CELL_PAD: f32 = 6.0;
/// What the disk list keeps clear on its right, for its scrollbar: iced paints the bar over the
/// content's right edge, so the last column has to stop short of it. See the task viewer's table, which
/// learnt this the hard way.
pub const SCROLLBAR: f32 = 14.0;

/// The column the form's field labels sit in: `地址`, `账号`, `密码` beside what has been typed.
///
/// Wide enough for the longest of the three at the base font size this page is laid out for, and fixed
/// so that the three values start at the same place — a label that sized itself would put the address
/// and the password at different lefts on the same form.
pub const FIELD_LABEL: f32 = 64.0;

/// Everything the page measures with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The table's own text, and the small print under a headline.
    pub text: f32,
    pub small: f32,
    /// The number at the head of a card: the one thing a glance is looking for.
    pub headline: f32,
    /// One row of the disk table.
    pub row: f32,
    /// One card, and the room between two of them.
    pub card: f32,
    pub card_gap: f32,
    /// The bar at the foot of a card.
    pub bar: f32,
}

impl Sizes {
    /// The measurements the base font size asks for: [`FONT_BASE`], with each field a multiple of it.
    pub fn new() -> Self {
        let base = FONT_BASE;

        Self {
            text: base * 0.72,
            small: base * 0.6,
            headline: base * 1.25,
            row: base * 1.1,
            card: base * 3.2,
            card_gap: base * 0.45,
            bar: base * 0.28,
        }
    }
}

/// The ink the numbers are written in.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: labels, units, ages.
pub fn muted(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x8B, 0x8F, 0xA3),
        ThemeMode::Light => Color::from_rgb8(0x6B, 0x70, 0x80),
    }
}

/// The page's own background.
///
/// The dark one is the box's own black ([`ThemeMode::background`]): the panel sits inside a black bezel,
/// and a page that is only *nearly* black draws that bezel's edge on the screen. The cards are what tells
/// the page apart from a card, so the page gives up nothing by being the same nothing as the glass.
pub fn surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => ThemeMode::Dark.background(),
        ThemeMode::Light => Color::from_rgb8(0xF4, 0xF5, 0xF8),
    }
}

/// A card's background: the page's, lifted one step so that a card reads as a thing rather than a
/// region of the page.
pub fn card(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x1C, 0x1E, 0x2A),
        ThemeMode::Light => Color::from_rgb8(0xFF, 0xFF, 0xFF),
    }
}

/// The empty half of a bar.
pub fn track(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x2A, 0x2D, 0x3A),
        ThemeMode::Light => Color::from_rgb8(0xE3, 0xE5, 0xEC),
    }
}

/// The colour of each metric, which is how a card is known before its label is read.
pub fn cpu() -> Color {
    Color::from_rgb8(0x4C, 0x8D, 0xF6)
}

pub fn memory() -> Color {
    Color::from_rgb8(0xA0, 0x6B, 0xF5)
}

pub fn network() -> Color {
    Color::from_rgb8(0x35, 0xC4, 0x6A)
}

pub fn disk() -> Color {
    Color::from_rgb8(0xF5, 0xA5, 0x24)
}

/// The two figures about the disks *as a whole* rather than about one direction of them: how much room
/// there is, and how hard they are being worked.
///
/// Colours of their own rather than [`disk`]'s, because the page has six cards and six colours is the
/// point of them: a card is told from the one beside it by its colour before its label is read.
pub fn storage() -> Color {
    Color::from_rgb8(0x2D, 0xD4, 0xBF)
}

pub fn busy() -> Color {
    Color::from_rgb8(0xE8, 0xC5, 0x4A)
}

/// Whether a machine is there: a green dot when the last look arrived, amber while one is in flight
/// and nothing has come back, red when the last one did not.
///
/// Three states and not two, because "we have not looked yet" is not "it is down" — and on a page whose
/// whole subject is a machine that sleeps, that distinction is most of what the status is for.
pub fn status_ink(theme: ThemeMode) -> StatusInk {
    StatusInk { theme }
}

/// The three inks a status dot is drawn in. A struct rather than three functions because they are read
/// together and are the same decision — see [`status_ink`].
#[derive(Debug, Clone, Copy)]
pub struct StatusInk {
    theme: ThemeMode,
}

impl StatusInk {
    pub fn online(&self) -> Color {
        Color::from_rgb8(0x35, 0xC4, 0x6A)
    }

    pub fn waiting(&self) -> Color {
        Color::from_rgb8(0xF5, 0xA5, 0x24)
    }

    pub fn offline(&self) -> Color {
        Color::from_rgb8(0xE5, 0x48, 0x4F)
    }

    /// The colour of an age: muted while the reading is fresh enough to trust, and the offline ink once
    /// it is older than the interval the board is looking at — which is the point at which the numbers
    /// beside it stopped being a description of now.
    pub fn age(&self, stale: bool) -> Color {
        if stale {
            self.offline()
        } else {
            muted(self.theme)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every size is a multiple of the base font size, and none of them collapses.
    #[test]
    fn every_measurement_is_a_multiple_of_the_base() {
        let size = Sizes::new();

        assert_eq!(size.text, FONT_BASE * 0.72);
        assert_eq!(size.small, FONT_BASE * 0.6);
        assert_eq!(size.headline, FONT_BASE * 1.25);
        assert_eq!(size.row, FONT_BASE * 1.1);
        assert_eq!(size.card, FONT_BASE * 3.2);
        assert_eq!(size.card_gap, FONT_BASE * 0.45);
        assert_eq!(size.bar, FONT_BASE * 0.28);

        assert!(size.text >= 10.0, "text is {size:?}");
        assert!(size.row > size.text, "a row has to be taller than its text");
        assert!(size.card > size.row);
        assert!(size.headline > size.text);
        assert!(size.small < size.text);
        assert!(size.bar > 0.0 && size.bar < size.text);
    }
}
