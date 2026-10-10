//! What the task viewer measures and colours with.
//!
//! Every size here comes out of one number — the font tier the system is set to — for the reason a
//! table has: columns that do not move when the text does are columns whose numbers run into each
//! other. Row heights and column widths are all derived from `base`, so changing the system's text
//! size changes the *density* of the table and not whether it lines up.

use iced::Color;

use pomelo_hal::TaskState;
use pomelo_widgets::preferences::{FontSizeTier, ThemeMode};

/// The room between the page and the panel, and between the parts of the page.
pub const MARGIN: f32 = 14.0;
pub const GAP: f32 = 10.0;
/// The room between two rows of the table.
pub const ROW_GAP: f32 = 2.0;
/// The room inside a row, on each side of its text.
pub const CELL_PAD: f32 = 4.0;

/// Everything the page measures with, for one font tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The table's own text: task names, priorities, stacks.
    pub text: f32,
    /// The line above and below it: the column heads and the footer.
    pub small: f32,
    /// One row of the table, tall enough for the text and a finger's worth of air.
    pub row: f32,
    /// The width of the state column: one letter, and not one word — the letters are what a person
    /// reads here, and the words are in the legend this page does not need.
    pub state: f32,
    /// The priority column. Two digits is the most this scheduler has.
    pub priority: f32,
    /// The stack column: `16.5K` is the widest it gets.
    pub stack: f32,
    /// The core column: one digit, or `-` for a task the scheduler may run anywhere.
    pub core: f32,
    /// A meter: the bar itself, the row it sits in, and the room its label takes.
    pub bar: f32,
    pub meter_row: f32,
    pub meter_label: f32,
}

impl Sizes {
    /// The measurements a font tier asks for.
    ///
    /// The multipliers are the whole of the design, and they are what a table needs rather than what a
    /// list needs: the body text is three quarters of the tier's base so that twelve rows fit where
    /// nine would, and every column is its widest content plus the padding a number wants on both
    /// sides. The meter's bar is a fraction taller than its own text, so that a row of them reads as
    /// bars rather than as underlines.
    pub fn of(tier: FontSizeTier) -> Self {
        let base = tier.base_size();

        Self {
            text: base * 0.75,
            small: base * 0.65,
            row: base * 1.15,
            state: base * 0.9,
            priority: base * 1.1,
            stack: base * 1.6,
            core: base * 0.9,
            bar: base * 0.6,
            meter_row: base * 0.95,
            meter_label: base * 2.1,
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

/// The ink for the things that are there to be found rather than read: the column heads, the row
/// separators, the second number in a meter.
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

/// The empty part of a meter.
pub fn track(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.10),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.08),
    }
}

/// A meter's fill for a pool this full.
///
/// Three colours rather than one, which is exactly what `htop`'s meters do and for the same reason: a
/// bar that is always the same green says how full something is and never whether to care. Green
/// while there is room, amber from three quarters, red from nine tenths — the thresholds a person
/// watching a heap would have picked anyway.
pub fn meter_fill(share: f32) -> Color {
    if share >= 0.9 {
        Color::from_rgb8(0xE0, 0x5A, 0x52)
    } else if share >= 0.75 {
        Color::from_rgb8(0xD8, 0xA1, 0x3C)
    } else {
        Color::from_rgb8(0x4C, 0xB0, 0x7A)
    }
}

/// A task's state in colour.
///
/// Running is the one state that means progress, so it is the one that gets a colour of its own;
/// waiting is the ordinary answer for most tasks at most moments and stays muted; and a task that has
/// been stopped or is on its way out is worth noticing, which is what the amber is for.
pub fn state_ink(state: TaskState, theme: ThemeMode) -> Color {
    match state {
        TaskState::Running => Color::from_rgb8(0x4C, 0xB0, 0x7A),
        TaskState::Blocked => muted(theme),
        TaskState::Suspended | TaskState::Deleted => Color::from_rgb8(0xD8, 0xA1, 0x3C),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every size grows with the tier, and none of them collapses.
    ///
    /// The one thing that could go wrong here is a multiplier small enough to round a column away at
    /// the smallest tier — a table whose numbers overlap is worse than a table that is too small.
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
            assert!(large.row > small.row);
            assert!(large.stack > small.stack);
        }

        for size in sizes {
            assert!(size.text >= 10.0, "text is {size:?}");
            assert!(size.row > size.text, "a row has to be taller than its text");
        }
    }

    /// The meters turn amber and then red as a pool fills, and stay green below that.
    #[test]
    fn a_meter_gets_warmer_as_it_fills() {
        let green = meter_fill(0.1);
        let amber = meter_fill(0.8);
        let red = meter_fill(0.95);

        assert_ne!(green, amber);
        assert_ne!(amber, red);
        assert_eq!(meter_fill(0.74), green, "the first threshold is 75%");
        assert_eq!(meter_fill(0.75), amber);
        assert_eq!(meter_fill(0.89), amber, "and the second is 90%");
        assert_eq!(meter_fill(0.9), red);
    }
}
