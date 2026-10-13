//! System-wide preferences and theme tokens shared across Pomelo OS applications.
//!
//! Provides the core types for:
//! - [`Language`]: interface language (Chinese or English)
//! - [`ThemeMode`]: display appearance (AMOLED Dark or Light)
//! - [`FONT_BASE`]: the one font size the interface draws at
//! - [`SystemPreferences`]: aggregated system preference state

use iced::Color;

/// The interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// 简体中文 (Simplified Chinese). Default for Pomelo OS.
    #[default]
    Chinese,
    /// English.
    English,
}

impl Language {
    /// Returns the other language, for toggle actions.
    #[inline]
    pub fn other(self) -> Self {
        match self {
            Self::Chinese => Self::English,
            Self::English => Self::Chinese,
        }
    }

    /// What this language calls itself.
    #[inline]
    pub fn name(self) -> &'static str {
        match self {
            Self::Chinese => "中文",
            Self::English => "English",
        }
    }

    /// Label for navigation / back actions in this language.
    #[inline]
    pub fn back_label(self) -> &'static str {
        match self {
            Self::Chinese => "返回",
            Self::English => "Back",
        }
    }
}

/// The visual theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// AMOLED pure black (#000000) for zero power on unlit pixels.
    #[default]
    Dark,
    /// Light mode with clean contrast.
    Light,
}

impl ThemeMode {
    /// Returns whether this is dark mode.
    #[inline]
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Returns whether this is light mode.
    #[inline]
    pub fn is_light(self) -> bool {
        matches!(self, Self::Light)
    }

    /// Returns the other theme mode, for toggle actions.
    #[inline]
    pub fn other(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// Human-readable name in `language`.
    pub fn name(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::Dark, Language::Chinese) => "深色模式",
            (Self::Dark, Language::English) => "Dark",
            (Self::Light, Language::Chinese) => "浅色模式",
            (Self::Light, Language::English) => "Light",
        }
    }

    /// Base background color.
    pub fn background(self) -> Color {
        match self {
            Self::Dark => Color::from_rgb8(0, 0, 0),
            Self::Light => Color::from_rgb8(242, 242, 247),
        }
    }

    /// Surface / Card background color.
    pub fn card(self) -> Color {
        match self {
            Self::Dark => Color::from_rgb8(28, 28, 30),
            Self::Light => Color::WHITE,
        }
    }

    /// Navigation bar background color.
    pub fn nav_bar(self) -> Color {
        match self {
            Self::Dark => Color::from_rgb8(18, 18, 18),
            Self::Light => Color::from_rgb8(248, 248, 248),
        }
    }

    /// Primary text color.
    pub fn text_primary(self) -> Color {
        match self {
            Self::Dark => Color::WHITE,
            Self::Light => Color::from_rgb8(0, 0, 0),
        }
    }

    /// Secondary / muted text color.
    pub fn text_secondary(self) -> Color {
        match self {
            Self::Dark => Color::from_rgb8(150, 150, 155),
            Self::Light => Color::from_rgb8(142, 142, 147),
        }
    }

    /// Card border / separator hairline.
    pub fn border(self) -> Color {
        match self {
            Self::Dark => Color::from_rgba8(255, 255, 255, 25.0 / 255.0),
            Self::Light => Color::from_rgba8(0, 0, 0, 20.0 / 255.0),
        }
    }

    /// Subtle row separator hairline.
    pub fn separator(self) -> Color {
        match self {
            Self::Dark => Color::from_rgba8(255, 255, 255, 18.0 / 255.0),
            Self::Light => Color::from_rgba8(0, 0, 0, 15.0 / 255.0),
        }
    }
}

/// The system's base font size, in pixels.
///
/// One size, not a scale. The interface used to offer four tiers (18/20/24/30 px) and the pre-baked
/// glyph tables shipped one per size to match; both are gone. Text is drawn at this size, so the one
/// baked table that ships (`fonts.rs`) is the size the interface actually asks for, and an app that
/// wants smaller type derives it from here rather than from a user-selectable tier.
pub const FONT_BASE: f32 = 24.0;

/// Aggregated system preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemPreferences {
    /// Active interface language.
    pub language: Language,
    /// Active theme appearance.
    pub theme: ThemeMode,
}

impl SystemPreferences {
    /// Creates a new preference set.
    pub const fn new(language: Language, theme: ThemeMode) -> Self {
        Self { language, theme }
    }

    /// The base font size in pixels, which is [`FONT_BASE`].
    #[inline]
    pub const fn font_size(&self) -> f32 {
        FONT_BASE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_toggles() {
        assert_eq!(Language::Chinese.other(), Language::English);
        assert_eq!(Language::English.other(), Language::Chinese);
    }

    #[test]
    fn theme_toggles() {
        assert_eq!(ThemeMode::Dark.other(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.other(), ThemeMode::Dark);
    }

    #[test]
    fn there_is_one_font_size_and_it_is_baked() {
        // The size the interface draws at, the size the baked table was cut at, and the size a
        // preference set reports have to be the same number: the lookup compares the size bit for
        // bit, so a drift of a hundredth of a pixel would silently take every glyph back to swash.
        assert_eq!(FONT_BASE, 24.0);
        assert_eq!(SystemPreferences::default().font_size(), FONT_BASE);
    }
}
