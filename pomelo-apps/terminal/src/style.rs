//! Metrics and colours.
//!
//! The original terminal's numbers, so the two apps laid out alike: absolute type,
//! one 24 px cell per line, and the iOS dark keyboard palette by name. The prompt colours are
//! [`shell`](crate::shell)'s [`Rgb`](crate::shell::Rgb) roles, converted to iced here — the model
//! has no colour
//! type of its own.

use std::time::Duration;

use crate::shell;
use crate::shell::Rgb;
use iced::Color;

/// The panel the terminal is designed for.
pub const SCREEN: u32 = 480;

/// The transcript's horizontal padding on the 480 px panel (the round AMOLED's safe inset).
pub const PAGE_PADDING: f32 = 22.0;

/// The keyboard band: about 48% of the panel's height, clamped. The formula lives with the
/// keyboard, because the settings app sizes its own band the same way.
pub use pomelo_widgets::touch_keyboard::band_height as keyboard_height;

/// The terminal's type and line cell, from the shell module that owns them.
pub const FONT_SIZE: f32 = shell::TERM_FONT_SIZE;
pub const LINE_HEIGHT: f32 = shell::TERM_LINE_HEIGHT;

/// The active input line's cursor: a bar the width of a stem, which is the shape a caret has
/// everywhere else on a machine that is typed into.
pub const CURSOR_WIDTH: f32 = 2.0;
pub const CURSOR_HEIGHT: f32 = shell::TERM_FONT_SIZE - 2.0;
/// Downward vertical adjustment for the cursor to align with the font's baseline.
pub const CURSOR_OFFSET_Y: f32 = 3.0;

/// Half of the cursor's blink: the bar is drawn for this long, left out for as long again, and the
/// app starts over — see [`Terminal::blink`](crate::Terminal::blink).
///
/// The half iced's own text cursor blinks at, so a page that has both of them on it does not blink
/// two different ways.
pub const CURSOR_BLINK: Duration = Duration::from_millis(500);

/// Measures a string, in logical pixels.
///
/// This is deliberately an estimate: iced's text engine is cosmic-text (shaping, per-glyph
/// advances) and is not reachable from `view`, while the original measured exactly
/// with its baked font. The constant is the same 0.55 × font size the original fallback used,
/// which is close for the Latin subset at one size. It only decides where a transcript line wraps
/// and how much of the input line's tail is shown — never the layout of a key.
pub fn measure(text: &str) -> f32 {
    text.chars().count() as f32 * (FONT_SIZE * 0.55)
}

use pomelo_widgets::preferences::ThemeMode;

/// The model's [`Rgb`] as an iced colour.
pub fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

/// The page background: pure black, the VS Code terminal.
pub fn background() -> Color {
    background_for(ThemeMode::Dark)
}

/// The page background for the given theme mode.
pub fn background_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        rgb((255, 255, 255))
    } else {
        rgb((0, 0, 0))
    }
}

/// The text foreground for the given theme mode.
pub fn text_fg_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(36, 41, 47)
    } else {
        color(shell::VS_TEXT_FG)
    }
}

/// The prompt user part for the given theme mode.
pub fn prompt_user_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(16, 128, 67)
    } else {
        color(shell::VS_PROMPT_USER)
    }
}

/// The prompt directory part for the given theme mode.
pub fn prompt_dir_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(9, 105, 218)
    } else {
        color(shell::VS_PROMPT_DIR)
    }
}

/// The prompt punctuation part for the given theme mode.
pub fn prompt_punct_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(101, 109, 118)
    } else {
        color(shell::VS_PROMPT_PUNCT)
    }
}

/// The prompt symbol part for the given theme mode.
pub fn prompt_sym_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(87, 96, 106)
    } else {
        color(shell::VS_PROMPT_SYM)
    }
}

/// The active command text for the given theme mode.
pub fn command_text_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(31, 35, 40)
    } else {
        color(shell::VS_COMMAND_TEXT)
    }
}

/// The quiet words of a session's status line.
///
/// The prompt's punctuation grey, named for what it is doing here rather than for what it sits next to:
/// a login in flight and a session somebody has left are things to *read* rather than things to act on,
/// and they have to be legible without competing with the page above them.
pub fn muted_for(theme: ThemeMode) -> Color {
    prompt_punct_for(theme)
}

/// A fault, which is the one status line that is not quiet: something is wrong and there is a thing to
/// do about it, which is what the red is for.
pub fn error_for(theme: ThemeMode) -> Color {
    if theme.is_light() {
        Color::from_rgb8(207, 34, 46)
    } else {
        color(shell::VS_ERROR)
    }
}

/// The three colours one key paints with, and the palette that picks them — the shared widgets',
/// so the terminal and the settings app paint a key the same way.
pub use pomelo_widgets::touch_keyboard::style::{palette, KeyPalette};

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}
