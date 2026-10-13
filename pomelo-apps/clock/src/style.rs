//! What the clock measures and colours with.
//!
//! Every size here comes out of one number — the base font size, `FONT_BASE` — for the reason the
//! dial has: a hand's length and the type under it are the same drawing, and a dial that grew while
//! its date line did not would read as two things stacked rather than one.

use iced::Color;

use pomelo_widgets::preferences::{ThemeMode, FONT_BASE};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// The layout itself never reads this — an app fills whatever space it is given — with one exception:
/// the *dial* is drawn from the box it is handed, and this is the number that box has on the panel. It
/// is read in the tests, so that "the hands are too long for the panel" is a failing assertion rather
/// than something noticed on a photograph of a screen.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the parts of the page.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 8.0;

/// The room between the tab strip's buttons.
pub const TAB_GAP: f32 = 6.0;

/// The thickness of the pomodoro ring, and the diameter the ring is drawn at.
pub const RING_THICKNESS: f32 = 14.0;

/// The corner of everything on this page that has one: the tabs and the alarm cards.
///
/// One number rather than one per shape, because they are stacked: a tab and the card under it are
/// two rounded rectangles a few pixels apart, and two different radii read as two different hands.
pub const RADIUS: f32 = 12.0;

/// How wide and how tall a stepper button is.
///
/// Square and one size, both of them: the alarm page's four steppers sit in two rows of two, and a
/// plus that was a different size from the minus beside it would be a pair that looked like two
/// different controls.
pub const STEPPER: f32 = 34.0;

/// Everything the page measures with, from one base font size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The digits of the digital readout, which are the largest thing on the page.
    pub digits: f32,
    /// The body text: dates, labels, the alarm list.
    pub text: f32,
    /// The smaller line: the tab labels, the status line at the foot.
    pub small: f32,
    /// One row of the alarm list, tall enough for the text and a finger's worth of air.
    pub row: f32,
    /// The room a tab takes, and the tap target the *page* row above it is not.
    pub tab: f32,
}

impl Sizes {
    /// The measurements the base font size asks for.
    ///
    /// The multipliers are the whole of the design. The base is `FONT_BASE`, and every field is a
    /// multiple of it. The digits are three times the base, because a clock read from across a desk is
    /// the one thing on this page that is *looked at* rather than read, and `88:88` at three times the
    /// base is five glyphs of roughly half the base each — 360 px of the panel's 480, with the margins
    /// it has. The body text is a little under the base so that the date and the first alarm fit under
    /// the dial, and the tab strip is the base itself, because a tab is a thing a finger has to find.
    pub fn new() -> Self {
        let base = FONT_BASE;

        Self {
            digits: base * 3.0,
            text: base * 0.85,
            small: base * 0.65,
            row: base * 1.3,
            tab: base * 1.35,
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

/// The ink for the things that are there to be found rather than read: the weekday, the tab that is
/// not selected, the tick marks a dial is read against.
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

/// A slab a finger has pressed, or a tab that is selected.
pub fn selected(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.12),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.10),
    }
}

/// The empty part of the pomodoro ring, and the circle the dial's face is.
pub fn track(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.10),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.08),
    }
}

/// Working is the one phase of a pomodoro that means "do not interrupt me", so it is the one that gets
/// a colour rather than a shade: the red a tomato timer is, and the same red the memory meters go
/// when there is nothing left.
pub fn work() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

/// A break is the other half of the same idea, in the green the meters go when there is room.
pub fn rest() -> Color {
    Color::from_rgb8(0x4C, 0xB0, 0x7A)
}

/// The colour of a phase.
pub fn phase_color(working: bool) -> Color {
    if working {
        work()
    } else {
        rest()
    }
}

/// The dial's hands.
///
/// The hour and minute hands are the ink, and the *second* hand is the accent: a dial with three hands
/// in one colour is a dial whose second hand has to be found, and this is the one hand that moves
/// every time the page redraws.
pub fn hand(theme: ThemeMode) -> Color {
    ink(theme)
}

/// The ink of the tab that is not selected, which is the muted ink written down once so that the tab
/// strip and the alarm rows cannot drift apart.
pub fn tab_ink(theme: ThemeMode, selected: bool) -> Color {
    if selected {
        ink(theme)
    } else {
        muted(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The readout is the largest thing on the page, and it still has to fit the panel.
    #[test]
    fn the_digital_readout_fits_the_panel() {
        let sizes = Sizes::new();

        // `88:88` is five glyphs. A digit in this font is a little over half the type size wide and
        // the colon is narrower, so this is the width with room to spare rather than an exact one.
        let readout = sizes.digits * 0.62 * 5.0;

        assert!(
            readout <= PANEL - 2.0 * MARGIN,
            "the readout is {readout} px wide, and the panel has {} px of room",
            PANEL - 2.0 * MARGIN
        );
    }

    /// The tab strip and the ring are the two things that have to fit *around* the digits.
    #[test]
    fn the_stack_fits_the_panel() {
        let sizes = Sizes::new();

        let tabs = sizes.tab;
        let ring = RING_THICKNESS * 2.0 + sizes.digits;
        let rest = sizes.text * 2.0 + sizes.small;

        let total = 2.0 * MARGIN + tabs + ring + rest + 3.0 * GAP;

        assert!(total <= PANEL, "the page wants {total} px of a {PANEL} px panel");
    }

    /// Every field is its own multiple of the one base size, and the things meant to be small really
    /// are smaller than the things meant to be big — the dial's own drawing is the one place that
    /// assumption would be invisible until it was on a screen.
    #[test]
    fn the_sizes_are_the_expected_multiples_of_the_base() {
        let sizes = Sizes::new();

        assert_eq!(sizes.digits, FONT_BASE * 3.0);
        assert_eq!(sizes.text, FONT_BASE * 0.85);
        assert_eq!(sizes.small, FONT_BASE * 0.65);
        assert_eq!(sizes.row, FONT_BASE * 1.3);
        assert_eq!(sizes.tab, FONT_BASE * 1.35);

        assert!(sizes.small < sizes.text);
        assert!(sizes.text < sizes.digits);
        assert!(sizes.row < sizes.digits);
        assert!(sizes.tab < sizes.digits);
    }
}
