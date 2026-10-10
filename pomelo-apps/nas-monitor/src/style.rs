//! What the NAS page measures and colours with.
//!
//! Every size comes out of one number — the font tier the system is set to — for the reason a table
//! has: columns that do not move when the text does are columns whose numbers run into each other. The
//! colours are the other half, and they are *per metric* rather than per fullness: a ring on a
//! dashboard is identified by its colour before its label is read, which is how a person tells the CPU
//! from the memory at a glance. That is the settings app's argument for its rings, and this page keeps
//! it for its bars.

use iced::Color;

use pomelo_widgets::preferences::{FontSizeTier, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// The layout never reads it — an app fills whatever it is given — but the disk table's column widths
/// are checked against it in a test, which is the one thing that can go wrong with fixed columns.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the parts of the page.
pub const MARGIN: f32 = 14.0;
pub const GAP: f32 = 10.0;
/// The room inside a card and inside a table row, on each side of its text.
pub const CELL_PAD: f32 = 6.0;
/// What the disk list keeps clear on its right, for its scrollbar: iced paints the bar over the
/// content's right edge, so the last column has to stop short of it. See the task viewer's table, which
/// learnt this the hard way.
pub const SCROLLBAR: f32 = 14.0;

/// Everything the page measures with, for one font tier.
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
    /// The measurements a font tier asks for.
    pub fn of(tier: FontSizeTier) -> Self {
        let base = tier.base_size();

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
pub fn surface(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0x12, 0x13, 0x1A),
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

    /// Every size grows with the tier, and none of them collapses.
    #[test]
    fn every_measurement_grows_with_the_font_tier() {
        let tiers = [
            FontSizeTier::ExtraSmall,
            FontSizeTier::Small,
            FontSizeTier::Standard,
            FontSizeTier::Large,
        ];

        let sizes: Vec<Sizes> = tiers.iter().copied().map(Sizes::of).collect();

        for pair in sizes.windows(2) {
            let (small, large) = (pair[0], pair[1]);
            assert!(large.text > small.text, "{small:?} then {large:?}");
            assert!(large.headline > small.headline);
            assert!(large.row > small.row);
            assert!(large.card > large.row);
        }

        for size in sizes {
            assert!(size.text >= 10.0, "text is {size:?}");
            assert!(size.row > size.text, "a row has to be taller than its text");
            assert!(size.bar > 0.0 && size.bar < size.text);
        }
    }
}
