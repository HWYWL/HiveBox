//! System-wide preferences and theme tokens shared across Pomelo OS applications.
//!
//! Provides the core types for:
//! - [`Language`]: interface language (Chinese or English)
//! - [`ThemeMode`]: display appearance (AMOLED Dark or Light)
//! - [`FontSizeTier`]: typography scale with three tiers (20px, 24px, 30px)
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

/// The font size tier for primary body text.
///
/// Provides four clean tiers:
/// - [`FontSizeTier::ExtraSmall`]: 18 px (compact tier, app-launcher label scale)
/// - [`FontSizeTier::Small`]: 20 px
/// - [`FontSizeTier::Standard`]: 24 px
/// - [`FontSizeTier::Large`]: 30 px
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontSizeTier {
    /// Extra small scale (base: 18 px).
    ExtraSmall,
    /// Small scale (base: 20 px).
    Small,
    /// Default standard scale (base: 24 px).
    #[default]
    Standard,
    /// Large scale (base: 30 px).
    Large,
}

impl FontSizeTier {
    pub const COMPACT: Self = Self::ExtraSmall;

    /// The four font size tier values in pixels: [18.0, 20.0, 24.0, 30.0].
    pub const SIZES: [f32; 4] = [18.0, 20.0, 24.0, 30.0];

    /// All four font size tiers in ascending order: [ExtraSmall, Small, Standard, Large].
    pub const TIERS: [FontSizeTier; 4] = [
        FontSizeTier::ExtraSmall,
        FontSizeTier::Small,
        FontSizeTier::Standard,
        FontSizeTier::Large,
    ];

    /// Returns the four font size tier values in pixels: [18.0, 20.0, 24.0, 30.0].
    #[inline]
    pub const fn sizes() -> [f32; 4] {
        Self::SIZES
    }

    /// Cycles to the next font size tier (ExtraSmall -> Small -> Standard -> Large -> ExtraSmall).
    #[inline]
    pub fn cycle(self) -> Self {
        match self {
            Self::ExtraSmall => Self::Small,
            Self::Small => Self::Standard,
            Self::Standard => Self::Large,
            Self::Large => Self::ExtraSmall,
        }
    }

    /// Returns the next font size tier.
    #[inline]
    pub fn other(self) -> Self {
        self.cycle()
    }

    /// Human-readable name in `language`.
    pub fn name(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::ExtraSmall, Language::Chinese) => "紧凑 (18px)",
            (Self::ExtraSmall, Language::English) => "Compact (18px)",
            (Self::Small, Language::Chinese) => "小号 (20px)",
            (Self::Small, Language::English) => "Small (20px)",
            (Self::Standard, Language::Chinese) => "标准 (24px)",
            (Self::Standard, Language::English) => "Standard (24px)",
            (Self::Large, Language::Chinese) => "大号 (30px)",
            (Self::Large, Language::English) => "Large (30px)",
        }
    }

    /// The base font size in pixels (18.0 for ExtraSmall, 20.0 for Small, 24.0 for Standard, 30.0 for Large).
    #[inline]
    pub const fn base_size(self) -> f32 {
        match self {
            Self::ExtraSmall => 18.0,
            Self::Small => 20.0,
            Self::Standard => 24.0,
            Self::Large => 30.0,
        }
    }

    /// Label font size for application grid tiles and lists.
    #[inline]
    pub const fn label_size(self) -> f32 {
        self.base_size()
    }
}

/// Aggregated system preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemPreferences {
    /// Active interface language.
    pub language: Language,
    /// Active theme appearance.
    pub theme: ThemeMode,
    /// Active typography scale.
    pub font_tier: FontSizeTier,
}

impl SystemPreferences {
    /// Creates a new preference set.
    pub const fn new(language: Language, theme: ThemeMode, font_tier: FontSizeTier) -> Self {
        Self {
            language,
            theme,
            font_tier,
        }
    }

    /// Returns the active base font size in pixels.
    #[inline]
    pub const fn font_size(&self) -> f32 {
        self.font_tier.base_size()
    }

    /// The four available system font size tiers: [18.0, 20.0, 24.0, 30.0].
    #[inline]
    pub const fn font_sizes() -> [f32; 4] {
        FontSizeTier::SIZES
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
    fn font_tier_toggles() {
        assert_eq!(FontSizeTier::ExtraSmall.base_size(), 18.0);
        assert_eq!(FontSizeTier::Small.base_size(), 20.0);
        assert_eq!(FontSizeTier::Standard.base_size(), 24.0);
        assert_eq!(FontSizeTier::Large.base_size(), 30.0);
        assert_eq!(FontSizeTier::ExtraSmall.cycle(), FontSizeTier::Small);
        assert_eq!(FontSizeTier::Small.cycle(), FontSizeTier::Standard);
        assert_eq!(FontSizeTier::Standard.cycle(), FontSizeTier::Large);
        assert_eq!(FontSizeTier::Large.cycle(), FontSizeTier::ExtraSmall);
    }
}
