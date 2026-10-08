//! [Google's Material Symbols](https://fonts.google.com/icons), as a font and as a type.
//!
//! One [`Icon`] per icon in the catalogue, so "which icons can I draw" is a question
//! autocomplete answers, and an icon that does not exist does not compile:
//!
//! ```ignore
//! use pomelo_material_symbols::{self as icons, Icon};
//!
//! // once, where the program is built -- the platform installs what the settings carry:
//! iced::application(..).font(icons::FONT)
//!
//! // and then, anywhere:
//! text(Icon::WIFI.glyph()).font(icons::font()).size(20.0)
//! ```
//!
//! # What it is
//!
//! A pinned, instanced cut of Material Symbols Outlined, in the spirit of a `-sys` crate: the
//! font's bytes, the numbers that say which cut this is, and just enough Rust to name one icon.
//! The provenance is in `README.md`'s *Regenerating* section and the figures are in
//! `fonts/MANIFEST.md`; both are part of what this repository is for, because a font that cannot
//! say where it came from cannot be rebaked.
//!
//! # Why an icon is a glyph
//!
//! Because it is the one road that is already open. `pomelo-gfx` rasterises RGB565 images and
//! 8-bit alpha masks, but iced's renderer exposes neither to an app -- `allocate_image` answers
//! `Unsupported` and `geometry::draw_image` is empty -- and `Canvas::fill_path` only strokes an
//! outline, so an icon drawn as a *path* would not fill either. Glyphs, on the other hand, are a
//! road with traffic on it: the default font travels it, and so does every string in every app.
//!
//! So an icon is a character in the Private Use Area of a font that carries nothing else, and
//! drawing one is drawing text. Two conveniences fall out of that. Every icon is exactly one em
//! square -- the subset's `unitsPerEm` is 960 and so is every advance -- so `size(24.0)` draws a
//! 24×24 icon and `.size()` is the whole of the sizing story. And it is vector: one subset serves
//! 16 px and 96 px, with no bitmaps to rebake per size.
//!
//! # The whole catalogue, and what it costs
//!
//! Every icon Google ships is in the subset -- 4 299 of them -- so there is no list to keep up to
//! date and no rebake before an app can use an icon nobody has used yet. An unused [`Icon`] costs
//! nothing at all: each one is a `const`, and a `const` that nothing references is not emitted.
//! What is paid for, once, is the font: 1.1 MiB of flash (see `fonts/MANIFEST.md`), with no RAM
//! to match -- the bytes are borrowed out of `.rodata` rather than copied, because
//! `FontSystem::load_font` hands `fontdb` the `Cow` it was given.
//!
//! # What is not here
//!
//! * **No widgets.** [`Icon`] is data and [`FONT`] is bytes: no renderer, no platform, no
//!   `Element`. The things that build widgets are in `pomelo-widgets`.
//! * **No default font.** What text looks like when nobody asks is the platform's decision, and
//!   this font is never it: it is installed under its own family name, so nothing draws with it
//!   by accident. A `text(..)` that does not ask for [`font`] gets the platform's font as before.
//! * **No icon lists.** There is no `ALL`: a slice of 4 299 icons would reference every `const`
//!   and so emit them all, and nothing iterates the catalogue -- you name the icon you want.

mod table;

pub use table::Icon;

/// The family the subset registers itself under.
///
/// Material Symbols' own name, taken from the subset's `name` table -- which the baker leaves
/// exactly as upstream wrote it, so that the name does not change when an axis (`FILL`, `wght`,
/// …) is re-pinned. [`font`] is the only thing that should need this string.
pub const FAMILY: &str = "Material Symbols Outlined";

/// The subset itself, as the bytes a program hands the platform.
///
/// A program that draws an icon has to install this once: `iced::application(..).font(FONT)` on a
/// desktop, and the same settings travel to the board when the firmware hosts the program. Without
/// it the family is unknown, and an icon draws as whatever the fallback font makes of a private use
/// code point -- nothing, usually. That is the one way to use this crate wrongly, which is why the
/// example at the top of the module installs the font before it draws anything.
///
/// ~1.1 MiB of flash, in `.rodata`, and no RAM: see the module docs.
pub const FONT: &[u8] = include_bytes!("../fonts/MaterialSymbolsOutlined-Subset.ttf");

/// The font to draw an [`Icon`] with -- the loaded form of [`FONT`].
///
/// ```ignore
/// text(Icon::SETTINGS.glyph()).font(pomelo_material_symbols::font())
/// ```
pub const fn font() -> iced_core::Font {
    iced_core::Font::with_name(FAMILY)
}

use std::borrow::Cow;
use std::fmt;

impl fmt::Display for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.glyph())
    }
}

impl<'a> From<Icon> for Cow<'a, str> {
    fn from(icon: Icon) -> Self {
        Cow::Borrowed(icon.glyph())
    }
}

impl From<Icon> for &'static str {
    fn from(icon: Icon) -> Self {
        icon.glyph()
    }
}

impl From<Icon> for char {
    fn from(icon: Icon) -> Self {
        icon.codepoint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The icons this OS draws, and the code points they must have.
    ///
    /// Not exhaustive -- the table has 4 299 entries and no way to walk them -- but it pins every
    /// one with a call site, so a rebake that silently moved or renamed something is a red build
    /// rather than a wrong picture on the panel. An icon that stops being drawn should be taken out:
    /// a list that only grows stops saying what the OS uses.
    const USED: &[(Icon, char)] = &[
        // The launcher's tiles.
        (Icon::TERMINAL, '\u{eb8e}'),
        (Icon::CALCULATE, '\u{ea5f}'),
        (Icon::COUNTER_0, '\u{f785}'),
        (Icon::WAVING_HAND, '\u{e766}'),
        (Icon::SETTINGS, '\u{e8b8}'),
        (Icon::MUSIC_NOTE, '\u{e405}'),
        // The status bar's signal: a staircase of arcs, one bar at a time. The three share their
        // bottom anchor, which is how the dot stays put as the arcs come and go.
        (Icon::WIFI_OFF, '\u{e648}'),
        (Icon::WIFI_1_BAR, '\u{e4ca}'),
        (Icon::WIFI_2_BAR, '\u{e4d9}'),
        (Icon::WIFI, '\u{e63e}'),
        // ...and its battery, which climbs through seven steps and then full.
        (Icon::BATTERY_ANDROID_0, '\u{f30d}'),
        (Icon::BATTERY_ANDROID_1, '\u{f30c}'),
        (Icon::BATTERY_ANDROID_2, '\u{f30b}'),
        (Icon::BATTERY_ANDROID_3, '\u{f30a}'),
        (Icon::BATTERY_ANDROID_4, '\u{f309}'),
        (Icon::BATTERY_ANDROID_5, '\u{f308}'),
        (Icon::BATTERY_ANDROID_6, '\u{f307}'),
        (Icon::BATTERY_ANDROID_FULL, '\u{f304}'),
        (Icon::BATTERY_ANDROID_FRAME_BOLT, '\u{f250}'),
    ];

    /// [`FONT`] is a font, it is the family [`font`] asks for, and it is the one [`table`] was
    /// generated from.
    ///
    /// The bytes are committed, so the failure this catches is a rebake that went wrong: an empty
    /// file, a truncated one, a subset of some *other* font -- or a font regenerated without
    /// regenerating the table beside it, which is what [`table::FONT_CHECK`] is for.
    #[test]
    fn the_embedded_font_is_the_family_the_table_was_generated_from() {
        assert!(
            FONT.starts_with(&[0x00, 0x01, 0x00, 0x00])
                || FONT.starts_with(b"OTTO")
                || FONT.starts_with(b"true"),
            "the embedded subset does not begin with a font header"
        );
        assert!(
            FONT.len() > 1024,
            "the embedded subset is {} bytes",
            FONT.len()
        );
        assert_eq!(
            table::checksum(FONT),
            table::FONT_CHECK,
            "the subset and the table are from different bakes -- run `python3 tools/bake_icons.py`"
        );

        // The `name` table stores strings as UTF-16BE, so the family is searched for that way
        // round -- a plain `FONT.windows(FAMILY.len())` would never match.
        let wanted: Vec<u8> = FAMILY.encode_utf16().flat_map(u16::to_be_bytes).collect();

        assert!(
            FONT.windows(wanted.len()).any(|window| window == wanted),
            "the embedded subset does not name itself {FAMILY:?}"
        );
    }

    /// An icon is one character, from the Private Use Area, and its glyph string is that character.
    #[test]
    fn the_icons_this_repository_draws_are_one_private_use_character_each() {
        for (icon, point) in USED {
            assert_eq!(
                icon.glyph(),
                point.to_string(),
                "{icon:?} is not U+{:04X}",
                *point as u32
            );
            assert!(
                ('\u{e000}'..='\u{f8ff}').contains(point)
                    || ('\u{f0000}'..='\u{ffffd}').contains(point),
                "{icon:?} is U+{:X}, outside the private use areas icons are drawn in",
                *point as u32
            );
            assert_eq!(
                icon.glyph().chars().count(),
                1,
                "{icon:?} is more than one character"
            );
        }
    }
}
