//! What the cloud page measures and colours with.
//!
//! Every size comes out of one number — [`FONT_BASE`], the base font size — for the reason the other
//! pages have: a row of a list, the name in it and the address under the name are one drawing of one
//! instance, and a name that kept its size while the address beside it grew would read as two
//! instances' worth of information rather than one.
//!
//! # Three meanings and a colour that means none of them
//!
//! [`good`], [`warn`] and [`bad`] are the same three the battery page, the status bar and the Wi-Fi
//! map use, and they mean the same thing here: up, changing, and not up. [`accent`] is this page's
//! own and means nothing at all — it is the row a finger picked and is about to act on, and the
//! header's glyph — because a page whose every colour was a judgement would have no way left to
//! say "this one is selected".

use iced::Color;

use pomelo_hal::CloudInstanceState;
use pomelo_widgets::preferences::{FONT_BASE, ThemeMode};

/// The panel this page is laid out for: the 480 px square the rest of this project designs against.
///
/// Only the tests read it, and they read it for one thing: whether an instance's name, its id and its
/// address still fit across the panel in one row, which is a fact about the panel rather than about
/// the widget tree.
pub const PANEL: f32 = 480.0;

/// The room between the page and the panel, and between the cards.
pub const MARGIN: f32 = 12.0;
pub const GAP: f32 = 10.0;

/// The corner of everything on this page that has one: the cards, the rows, the buttons.
///
/// One number rather than one per shape, because they are stacked: a row sits inside the card that
/// holds it, and two different radii read as two different hands.
pub const RADIUS: f32 = 12.0;

/// How tall one instance's row is.
///
/// Taller than the two lines in it, because a row is also a thing to *press*: at the base font size
/// this is a comfortable target for a finger, and it is the smallest this project draws one on.
pub const ROW: f32 = 62.0;

/// Everything the page measures with, at the base font size [`FONT_BASE`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The header's glyph, and the only thing on the page drawn bigger than a name.
    pub icon: f32,
    /// An instance's name, and the header's title.
    pub name: f32,
    /// The details beside a name, the card titles and the buttons.
    pub text: f32,
    /// The hint under a name, and the status line under the title.
    pub small: f32,
    /// The region beside a card title, and the note in the footer of a card.
    pub tiny: f32,
}

impl Sizes {
    /// The measurements the base font size asks for: [`FONT_BASE`], with each field a multiple of it.
    ///
    /// The last three are the same multiples the Wi-Fi map uses, which is what makes the two pages
    /// look like two pages of one box: a second caption size that was the first one plus a pixel would
    /// read as a mistake rather than as a step.
    pub fn new() -> Self {
        let base = FONT_BASE;

        Self {
            icon: base * 1.5,
            name: base * 1.0,
            text: base * 0.9,
            small: base * 0.68,
            tiny: base * 0.58,
        }
    }
}

/// The ink for the things that are read: the names, the addresses, the titles.
pub fn ink(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgb8(0xE6, 0xE7, 0xEF),
        ThemeMode::Light => Color::from_rgb8(0x1C, 0x1E, 0x28),
    }
}

/// The ink for the things that are there to be found rather than read: the ids, the regions, the hints.
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

/// An instance's row when no finger has picked it: a step off the card it is in.
pub fn track(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba8(0xFF, 0xFF, 0xFF, 0.05),
        ThemeMode::Light => Color::from_rgba8(0, 0, 0, 0.04),
    }
}

/// The page's own colour: the picked product, the picked instance, and the header's glyph.
///
/// The colour of a cloud seen from below — a grey blue and not a bright one. Deliberately not the
/// weather tile's sky: that one is a forecast, and this page is a list of machines.
pub fn accent() -> Color {
    Color::from_rgb8(0x5B, 0x8F, 0xB0)
}

/// A machine that is up.
pub fn good() -> Color {
    Color::from_rgb8(0x4C, 0xB0, 0x7A)
}

/// A machine that is being changed, or that is in a state this board cannot explain.
pub fn warn() -> Color {
    Color::from_rgb8(0xE8, 0xA3, 0x3D)
}

/// Something that did not work.
pub fn bad() -> Color {
    Color::from_rgb8(0xE0, 0x5A, 0x52)
}

/// The colour of a machine, by what it is doing.
///
/// Three colours for six states, and the mapping is the whole judgement: up is good; down is neither
/// good nor bad — it is what a machine nobody is using looks like, and half the buttons on the page
/// are there to change it — and everything in between, starting, stopping, restarting and the states
/// this board cannot name, is amber, which is what "wait" looks like.
pub fn state_colour(state: CloudInstanceState, theme: ThemeMode) -> Color {
    if state.is_busy() || state == CloudInstanceState::Other {
        warn()
    } else if state.is_running() {
        good()
    } else {
        muted(theme)
    }
}

/// The glyph that goes with a state's colour.
///
/// The same four drawings the launcher's tile and the desktop's status line would use, so that a
/// machine being restarted and a machine that is up are two pictures rather than two sentences.
pub fn state_icon(state: CloudInstanceState) -> pomelo_material_symbols::Icon {
    use pomelo_material_symbols::Icon;

    match state {
        CloudInstanceState::Running => Icon::PLAY_CIRCLE,
        CloudInstanceState::Stopped => Icon::STOP_CIRCLE,
        _ => Icon::PROGRESS_ACTIVITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every size is the expected multiple of the base, so that no page is drawn for the wrong reader.
    #[test]
    fn the_sizes_are_multiples_of_the_base_font_size() {
        let size = Sizes::new();

        assert_eq!(size.icon, FONT_BASE * 1.5);
        assert_eq!(size.name, FONT_BASE);
        assert_eq!(size.text, FONT_BASE * 0.9);
        assert_eq!(size.small, FONT_BASE * 0.68);
        assert_eq!(size.tiny, FONT_BASE * 0.58);

        assert!(size.tiny < size.small, "the smallest step is still a step");
        assert!(size.icon > size.name, "and the glyph is the biggest thing");
    }

    /// The colours mean three different things, and a state is drawn in one of them in either theme.
    ///
    /// Except for a machine that is down, whose grey is the theme's own faint ink: "off" is not a status,
    /// it is the absence of one, and an off machine has to be as quiet on a white page as on a black one.
    #[test]
    fn every_state_gets_one_colour_and_the_colours_are_all_different() {
        for theme in [ThemeMode::Light, ThemeMode::Dark] {
            for state in [
                CloudInstanceState::Running,
                CloudInstanceState::Stopped,
                CloudInstanceState::Starting,
                CloudInstanceState::Stopping,
                CloudInstanceState::Rebooting,
                CloudInstanceState::Other,
            ] {
                let colour = state_colour(state, theme);
                assert!(
                    [good(), warn(), bad(), muted(theme), accent()].contains(&colour),
                    "{state:?} is drawn in a colour this page has a meaning for"
                );
            }

            assert_eq!(state_colour(CloudInstanceState::Running, theme), good());
            assert_eq!(state_colour(CloudInstanceState::Stopped, theme), muted(theme));
            // Everything between the two is amber, which is what "wait" looks like — and so is a state
            // this board cannot name, which is a machine to look at before pressing anything.
            assert_eq!(state_colour(CloudInstanceState::Starting, theme), warn());
            assert_eq!(state_colour(CloudInstanceState::Stopping, theme), warn());
            assert_eq!(state_colour(CloudInstanceState::Rebooting, theme), warn());
            assert_eq!(state_colour(CloudInstanceState::Other, theme), warn());
        }

        // Three colours, three meanings, and none of them is another.
        assert_ne!(good(), warn());
        assert_ne!(good(), muted(ThemeMode::Light));
        assert_ne!(warn(), muted(ThemeMode::Dark));
    }
}
