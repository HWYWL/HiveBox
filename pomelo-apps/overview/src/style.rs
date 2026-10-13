//! What the dashboard measures and colours with.
//!
//! Every size here comes out of one number — [`FONT_BASE`], the base font size — for the reason the
//! clock's page has: the readings and the bars under them are one drawing, and a bar that stayed the
//! same height while the number beside it grew would read as a decoration rather than as the same
//! reading twice.

use iced::Color;

use pomelo_widgets::preferences::{FONT_BASE, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// The layout itself never reads this — an app fills whatever space it is given — with one exception:
/// a level bar's *fill* is a share of the track it is drawn in, and how wide that track may be is a
/// fact about the panel. It is read in the tests, so that "a card is wider than the screen it is on"
/// is a failing assertion rather than something noticed on a photograph of a screen.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the cards.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards and the bars inside them.
///
/// One number rather than one per shape, because they are stacked: a bar sits inside the card that
/// holds it, and two different radii read as two different hands.
pub const RADIUS: f32 = 14.0;

/// How tall a level bar is.
pub const BAR: f32 = 10.0;

/// The round ends of a level bar.
///
/// Half its height, so the bar is a capsule: the fill and the track behind it are the same shape, and
/// a fill whose ends were square would poke out of the track's corners the moment it was short.
pub const BAR_RADIUS: f32 = BAR / 2.0;

/// Everything the page measures with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The one number in a card that is looked at rather than read: the charge, the signal, a share.
    pub reading: f32,
    /// The card titles and the values beside them.
    pub text: f32,
    /// The units, and the secondary lines under a reading.
    pub small: f32,
    /// A card's own title row, tall enough for the glyph and a finger's worth of air.
    pub header: f32,
}

impl Sizes {
    /// The measurements the base font size asks for: [`FONT_BASE`], with each field a multiple of it.
    ///
    /// The multipliers are the whole of the design. A reading is a little over one and a half times
    /// [`FONT_BASE`], because it is the one thing in its card that is read from across a desk; the
    /// titles and the secondary lines step down from it so that four cards and their sublines
    /// fit the panel without the page having to scroll to say everything.
    pub fn new() -> Self {
        let base = FONT_BASE;

        Self {
            reading: base * 1.6,
            text: base * 0.9,
            small: base * 0.68,
            header: base * 1.4,
        }
    }
}

/// The ink the readings are written in.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: the card titles, the units, a
/// line the board could not answer.
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

/// A card's own slab, a step up from the page so that five of them read as five things.
pub fn card(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.07),
        ThemeMode::Light => Color::WHITE,
    }
}

/// The empty part of a level bar.
pub fn track(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.10),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.08),
    }
}

/// Plenty left: the green the rest of the box's meters go.
pub fn good() -> Color {
    Color::from_rgb8(0x4C, 0xB0, 0x7A)
}

/// Getting close to something: amber, the one that says *look at this soon*.
pub fn warn() -> Color {
    Color::from_rgb8(0xE8, 0xA3, 0x3D)
}

/// Flat, hot or gone: the red the clock's work phase is.
pub fn bad() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

/// The page's own colour for a reading that is neither good nor bad: the radio, the machine next door.
///
/// A fourth colour rather than one of the three, because the three mean something: painting a healthy
/// Wi-Fi connection green would say it was a *measurement* that had passed, and the only thing green
/// on this page is a bar that is doing fine.
pub fn accent() -> Color {
    Color::from_rgb8(0x4A, 0x86, 0xE8)
}

/// The colour of a charge, where a *full* cell is the good one.
///
/// The two ends are the two things a person acts on — plug it in, or leave it alone — so the colour
/// changes on those and not on a scale: a board at 34% is not twice as urgent as one at 68%.
pub fn charge(percent: f32) -> Color {
    if percent <= 15.0 {
        bad()
    } else if percent <= 35.0 {
        warn()
    } else {
        good()
    }
}

/// The colour of a *use*, where a **full** disk is the bad news — the other way round from [`charge`]
/// and deliberately so, because the two are read as the same bar and must not be the same colour for
/// the same number.
pub fn usage(percent: f32) -> Color {
    if percent >= 90.0 {
        bad()
    } else if percent >= 75.0 {
        warn()
    } else {
        good()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one number that is *read* rather than scanned has to fit beside its card's title: it is the
    /// largest thing on the page and the thing a large font would push off it.
    #[test]
    fn a_reading_fits_the_panel() {
        let sizes = Sizes::new();

        // The widest reading this page draws is a share with its sign and a unit: `100%`. Five
        // glyphs at a little over half the type size is the width with room to spare.
        let reading = sizes.reading * 0.62 * 5.0;

        assert!(
            reading <= PANEL - 2.0 * MARGIN,
            "a reading is {reading} px wide, and the panel has {} px of room",
            PANEL - 2.0 * MARGIN
        );
    }

    /// A reading is the largest thing on the page, and the text around it steps down from it.
    #[test]
    fn a_reading_is_the_largest_thing_on_the_page() {
        let sizes = Sizes::new();

        assert!(sizes.reading > sizes.header, "the reading leads the page");
        assert!(sizes.header > sizes.text);
        assert!(sizes.text > sizes.small);
    }

    /// A nearly-empty cell is red, a middling one amber, and a healthy one green — with the
    /// boundaries on the side they are written.
    #[test]
    fn a_charge_is_coloured_by_what_is_left() {
        assert_eq!(charge(100.0), good());
        assert_eq!(charge(36.0), good());
        assert_eq!(charge(35.0), warn(), "the boundary is in the tighter band");
        assert_eq!(charge(16.0), warn());
        assert_eq!(charge(15.0), bad(), "and so is this one");
        assert_eq!(charge(0.0), bad());
    }

    /// A use is the opposite way round, because the number that is bad is the big one.
    #[test]
    fn a_use_is_coloured_by_how_little_is_left() {
        assert_eq!(usage(0.0), good());
        assert_eq!(usage(74.0), good());
        assert_eq!(usage(75.0), warn(), "the boundary is in the fuller band");
        assert_eq!(usage(89.0), warn());
        assert_eq!(usage(90.0), bad());
        assert_eq!(usage(100.0), bad());
    }

    /// The two are not the same reading, and the test that says so is the one that keeps a full disk
    /// and a flat battery from being drawn the same colour.
    #[test]
    fn the_same_number_means_opposite_things() {
        assert_ne!(charge(90.0), usage(90.0));
        assert_ne!(charge(10.0), usage(10.0));
        assert_eq!(charge(90.0), good());
        assert_eq!(usage(90.0), bad());
    }

    /// The bar's round ends are its own height in two, so that a fill and its track are one shape.
    #[test]
    fn a_bar_is_a_capsule() {
        assert_eq!(BAR_RADIUS, BAR / 2.0);
        assert!(BAR > 0.0);
    }
}
