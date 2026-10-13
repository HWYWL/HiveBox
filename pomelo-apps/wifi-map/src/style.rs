//! What the channel page measures and colours with.
//!
//! Every size comes out of one number — the font tier the system is set to — for the reason the other
//! pages have: the chart, the map and the list are one drawing of the same scan, and a chart that kept
//! its height while the numbers under it grew would read as a decoration rather than as the reading.

use iced::Color;

use pomelo_widgets::preferences::{FontSizeTier, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// Only the tests read it, and they read it for one thing: whether thirteen columns and their labels
/// still fit across the panel, which is a fact about the panel rather than about the widget tree.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the cards.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards, the columns, the squares.
///
/// One number rather than one per shape, because they are stacked: a column sits inside the card that
/// holds it, and two different radii read as two different hands.
pub const RADIUS: f32 = 12.0;

/// How tall the channel chart is drawn, columns and all.
pub const CHART: f32 = 92.0;

/// How tall one row of the signal map is.
///
/// Taller than the squares inside it, so that a network has room to sit in the middle of its row
/// rather than to fill it: the gaps between the rows are what makes the map a grid rather than a block.
pub const MAP_ROW: f32 = 17.0;

/// How wide the signal map's label column is.
///
/// Sized for the longest label the map has — `-100`, four glyphs — at the largest tier, where the tiny
/// step is 17 px: a gutter that fitted the three-glyph labels and clipped the fourth would clip exactly
/// the weakest band, which is the one a person looks at.
pub const MAP_LABEL: f32 = 40.0;

/// The side of one network's square on the signal map.
pub const DOT: f32 = 9.0;

/// Everything the page measures with, for one font tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The one number that is advice rather than a reading: the channel to use.
    pub reading: f32,
    /// The card titles, the network names and the channel numbers.
    pub text: f32,
    /// The captions, the axis labels, the dBm figures.
    pub small: f32,
    /// The count over a column, and the labels down the side of the map.
    pub tiny: f32,
}

impl Sizes {
    /// The measurements a font tier asks for.
    ///
    /// The multipliers are the whole of the design. The recommendation is the one thing on the page
    /// that is read from across a desk; everything else steps down from the tier's base so that a
    /// thirteen-column chart and its labels fit the panel without the page having to be wide.
    pub fn of(tier: FontSizeTier) -> Self {
        let base = tier.base_size();

        Self {
            reading: base * 1.6,
            text: base * 0.9,
            small: base * 0.68,
            tiny: base * 0.58,
        }
    }
}

/// The ink for the things that are read: the names, the numbers, the axis.
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

/// The empty part of a column: the channel that is there and has nothing on it.
pub fn track(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.10),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.08),
    }
}

/// The page's own colour: the suggestion, and the marker on the channel it names.
///
/// A teal rather than one of the three meaning colours, because the three mean something here: green
/// is a channel that is doing fine and red is one that is not, and advice is neither of those.
pub fn accent() -> Color {
    Color::from_rgb8(0x00, 0x96, 0x88)
}

/// A signal that is doing fine.
pub fn good() -> Color {
    Color::from_rgb8(0x4C, 0xB0, 0x7A)
}

/// A signal that is there but worth noticing.
pub fn warn() -> Color {
    Color::from_rgb8(0xE8, 0xA3, 0x3D)
}

/// A signal that is weak, or a channel that is crowded.
pub fn bad() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

/// How full a channel's column is drawn, in colour.
///
/// The same two thresholds as the levels on the battery page, for the same reason: what a person acts
/// on is "this one is fine" and "this one is not", and a channel at 40% is not twice as crowded as one
/// at 20% — the numbers behind this are an overlap model, so the third decimal of them is not a fact.
pub fn crowd(level: f32) -> Color {
    if level >= 0.55 {
        bad()
    } else if level >= 0.25 {
        warn()
    } else {
        good()
    }
}

/// The colour of a network's signal, by the same four bars the status bar and the settings list use.
///
/// Bars rather than dBm, so that the colour of a dot on the map and the bars beside a name in the list
/// are two drawings of one judgement rather than two judgements that could disagree.
pub fn signal(rssi: i8) -> Color {
    match pomelo_hal::signal_bars(rssi) {
        4 | 3 => good(),
        2 => warn(),
        _ => bad(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three colours mean three different things, and the ends of the ramp are the ends of it.
    #[test]
    fn the_crowding_ramp_has_three_steps() {
        assert_eq!(crowd(0.0), good());
        assert_eq!(crowd(0.24), good());
        assert_eq!(crowd(0.25), warn());
        assert_eq!(crowd(0.54), warn());
        assert_eq!(crowd(0.55), bad());
        assert_eq!(crowd(1.0), bad());
    }

    /// The colour of a network follows the bars the rest of the box draws for the same signal.
    #[test]
    fn the_signal_colour_follows_the_bars() {
        assert_eq!(signal(-40), good(), "four bars");
        assert_eq!(signal(-63), good(), "three bars");
        assert_eq!(signal(-70), warn(), "two");
        assert_eq!(signal(-80), bad(), "one");
        assert_eq!(signal(-95), bad(), "none");
    }

    /// Every size a tier asks for grows with the tier, so that no page is drawn for the wrong reader.
    #[test]
    fn the_sizes_follow_the_tier() {
        let small = Sizes::of(FontSizeTier::Small);
        let large = Sizes::of(FontSizeTier::Large);

        assert!(small.reading < large.reading);
        assert!(small.text <= large.text);
        assert!(small.tiny < small.small, "the smallest step is still a step");
        assert!(small.reading > small.text, "and the reading is the biggest thing");
    }
}
