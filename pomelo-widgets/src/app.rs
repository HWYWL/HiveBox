//! Application metadata shared between sub-apps and system launchers.

use iced::Color;
use pomelo_material_symbols::Icon;

use crate::preferences::Language;

/// A baked bitmap icon, with 16-bit RGB565 color pixels and an 8-bit alpha mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapIcon {
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Little-endian 16-bit RGB565 pixels (`width * height` entries).
    pub rgb565: &'static [u16],
    /// 8-bit alpha mask (`width * height` entries, 0 = transparent, 255 = opaque).
    pub alpha: &'static [u8],
}

impl BitmapIcon {
    /// Constructs a new [`BitmapIcon`].
    pub const fn new(
        width: u16,
        height: u16,
        rgb565: &'static [u16],
        alpha: &'static [u8],
    ) -> Self {
        Self {
            width,
            height,
            rgb565,
            alpha,
        }
    }
}

/// An application icon: either a vector glyph (Material Symbol) or a baked bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppIcon {
    /// A vector glyph from Material Symbols.
    Glyph(Icon),
    /// A hardware-native RGB565 + Alpha bitmap icon.
    Bitmap(BitmapIcon),
}

impl AppIcon {
    /// Constructs a glyph application icon.
    pub const fn glyph(icon: Icon) -> Self {
        Self::Glyph(icon)
    }

    /// Constructs a bitmap application icon.
    pub const fn bitmap(
        width: u16,
        height: u16,
        rgb565: &'static [u16],
        alpha: &'static [u8],
    ) -> Self {
        Self::Bitmap(BitmapIcon::new(width, height, rgb565, alpha))
    }

    /// Returns the icon glyph if this is a [`AppIcon::Glyph`].
    pub const fn as_glyph(&self) -> Option<Icon> {
        match self {
            Self::Glyph(icon) => Some(*icon),
            Self::Bitmap(_) => None,
        }
    }

    /// Returns the bitmap icon if this is a [`AppIcon::Bitmap`].
    pub const fn as_bitmap(&self) -> Option<BitmapIcon> {
        match self {
            Self::Glyph(_) => None,
            Self::Bitmap(b) => Some(*b),
        }
    }

    /// Whether this icon is a baked bitmap.
    pub const fn is_bitmap(&self) -> bool {
        matches!(self, Self::Bitmap(_))
    }
}

impl From<Icon> for AppIcon {
    fn from(icon: Icon) -> Self {
        Self::Glyph(icon)
    }
}

impl From<BitmapIcon> for AppIcon {
    fn from(bitmap: BitmapIcon) -> Self {
        Self::Bitmap(bitmap)
    }
}

/// Metadata describing an application's identity in the launcher and shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppMeta {
    /// Shown under the tile (canonical English name).
    pub name: &'static str,
    /// Simplified Chinese name.
    pub name_zh: &'static str,
    /// Drawn inside the tile (glyph or bitmap).
    pub icon: AppIcon,
    /// The tile's accent, as RGB bytes so this table stays a plain `const`.
    pub accent: (u8, u8, u8),
}

impl AppMeta {
    /// Constructs a new [`AppMeta`] with the given names, icon, and accent color.
    pub const fn new(
        name: &'static str,
        name_zh: &'static str,
        icon: AppIcon,
        accent: (u8, u8, u8),
    ) -> Self {
        Self {
            name,
            name_zh,
            icon,
            accent,
        }
    }

    /// Constructs a new [`AppMeta`] with a glyph icon.
    pub const fn new_with_glyph(
        name: &'static str,
        name_zh: &'static str,
        icon: Icon,
        accent: (u8, u8, u8),
    ) -> Self {
        Self {
            name,
            name_zh,
            icon: AppIcon::Glyph(icon),
            accent,
        }
    }

    /// The localized name in `language`.
    pub const fn localized_name(&self, language: Language) -> &'static str {
        match language {
            Language::Chinese => self.name_zh,
            Language::English => self.name,
        }
    }

    /// Canonical English name.
    pub const fn name_en(&self) -> &'static str {
        self.name
    }

    /// The accent, as an iced [`Color`].
    pub fn color(&self) -> Color {
        let (r, g, b) = self.accent;
        Color::from_rgb8(r, g, b)
    }
}
