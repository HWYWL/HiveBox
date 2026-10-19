//! A package's metadata: the small JSON document that is read before a byte of code is run.
//!
//! It answers the questions the launcher and the store ask *without* instantiating anything — what
//! is this, what version is it, may it have 256 KB of memory, which accent colour does its tile wear
//! — and it is therefore the only part of a package that is parsed on every boot.
//!
//! # The id is also a directory name
//!
//! [`Meta::id`] becomes `/internal/AppData/apps/<id>/`, so it is validated like a path before it is
//! trusted: lowercase ASCII letters, digits, `-` and `_`, at most [`MAX_ID_BYTES`], and it may not be
//! `.` or contain `..`. A package that names itself `../../ota_0` is refused at the border rather
//! than becoming a write outside the directory it was given.

use core::fmt;
use std::str::FromStr;

use app_abi::MAX_MEMORY_KB;

/// The longest id a package may have. Long enough for `power-log`, short enough for a directory name
/// on LittleFS.
pub const MAX_ID_BYTES: usize = 32;

/// The longest name a tile can draw without an ellipsis, in characters rather than bytes.
pub const MAX_NAME_CHARS: usize = 16;

/// What a package says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// The stable identity: `calculator`. Also the directory the package lives in.
    pub id: String,
    /// The English name under the tile.
    pub name: String,
    /// The Chinese name under the tile.
    pub name_zh: String,
    /// The version, written the way this project writes them: `v1.0.0`.
    pub version: String,
    /// The ABI major the package was built against.
    pub abi: u16,
    /// How much linear memory it asks for.
    pub memory_kb: u32,
    /// What host services it may use. See [`app_abi::perm`].
    pub permissions: u32,
    /// The tile's accent, as RGB.
    pub accent: (u8, u8, u8),
    /// One line the store shows under the name. May be empty.
    pub description: String,
    /// A Material Symbols glyph name for the icon when the package carries no bitmap. Empty when the
    /// package relies on its own icon section.
    pub glyph: String,
    /// The oldest firmware this package will run on, when it says. Empty means "this ABI is enough".
    pub min_os: String,
}

impl Meta {
    /// Reads a metadata document.
    ///
    /// Every field that has a default gets one; the four that cannot be guessed — id, name, version
    /// and a memory budget within the limit — are what make a document not a metadata document.
    pub fn parse(document: &str) -> Result<Self, MetaError> {
        let id = mini_json::string(document, "id").ok_or(MetaError::MissingId)?;
        validate_id(id)?;

        let name = mini_json::string(document, "name")
            .filter(|name| !name.is_empty())
            .ok_or(MetaError::MissingName)?;

        let name_zh = mini_json::string(document, "name_zh").unwrap_or(name);
        let version = mini_json::string(document, "version").ok_or(MetaError::MissingVersion)?;

        let abi = mini_json::number(document, "abi").unwrap_or(app_abi::ABI_MAJOR as u64);
        let abi = u16::try_from(abi).map_err(|_| MetaError::BadAbi(abi))?;

        let memory_kb = mini_json::number(document, "memory_kb").unwrap_or(256);
        let memory_kb = u32::try_from(memory_kb).unwrap_or(u32::MAX);

        if memory_kb == 0 || memory_kb > MAX_MEMORY_KB {
            return Err(MetaError::BadMemory(memory_kb));
        }

        let permissions = mini_json::number(document, "permissions").unwrap_or(0);
        let permissions = u32::try_from(permissions).unwrap_or(u32::MAX);

        let accent = mini_json::string(document, "accent")
            .map(Color::from_str)
            .transpose()?
            .unwrap_or(Color(0x7C, 0x83, 0x92))
            .into();

        Ok(Self {
            id: id.to_string(),
            name: truncate(name, MAX_NAME_CHARS),
            name_zh: truncate(name_zh, MAX_NAME_CHARS),
            version: version.to_string(),
            abi,
            memory_kb,
            permissions,
            accent,
            description: mini_json::string(document, "description")
                .unwrap_or("")
                .to_string(),
            glyph: mini_json::string(document, "glyph").unwrap_or("").to_string(),
            min_os: mini_json::string(document, "min_os").unwrap_or("").to_string(),
        })
    }

    /// How many pages of 64 KiB the application's memory budget is.
    pub fn memory_pages(&self) -> u32 {
        self.memory_kb.div_ceil(64)
    }

    /// Whether this package may use a host service.
    pub fn allows(&self, permission: u32) -> bool {
        self.permissions & permission != 0
    }
}

/// Why a metadata document was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetaError {
    MissingId,
    BadId(String),
    MissingName,
    MissingVersion,
    BadAbi(u64),
    BadMemory(u32),
    BadColor,
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingId => write!(f, "应用包没有 id"),
            Self::BadId(id) => write!(f, "应用 id 不合法: {id}"),
            Self::MissingName => write!(f, "应用包没有名字"),
            Self::MissingVersion => write!(f, "应用包没有版本"),
            Self::BadAbi(abi) => write!(f, "应用包声明了不认识的 ABI {abi}"),
            Self::BadMemory(kb) => write!(f, "应用包请求了 {kb} KB 内存（上限 {MAX_MEMORY_KB}）"),
            Self::BadColor => write!(f, "应用包的颜色写错了"),
        }
    }
}

/// An id is safe to use as one path component, and nothing else.
pub fn validate_id(id: &str) -> Result<(), MetaError> {
    if id.is_empty()
        || id.len() > MAX_ID_BYTES
        || id == "."
        || id.contains("..")
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(MetaError::BadId(id.to_string()));
    }

    Ok(())
}

fn truncate(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }

    text.chars().take(chars).collect()
}

/// `#RRGGBB`, the way the manifest and a metadata document write a colour.
struct Color(u8, u8, u8);

impl FromStr for Color {
    type Err = MetaError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let hex = text.trim().strip_prefix('#').ok_or(MetaError::BadColor)?;

        if hex.len() != 6 {
            return Err(MetaError::BadColor);
        }

        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16);

        Ok(Self(
            channel(0).map_err(|_| MetaError::BadColor)?,
            channel(2).map_err(|_| MetaError::BadColor)?,
            channel(4).map_err(|_| MetaError::BadColor)?,
        ))
    }
}

impl From<Color> for (u8, u8, u8) {
    fn from(color: Color) -> Self {
        (color.0, color.1, color.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r##"{
        "id": "calculator",
        "name": "Calculator",
        "name_zh": "计算器",
        "version": "v1.0.0",
        "abi": 1,
        "memory_kb": 256,
        "permissions": 3,
        "accent": "#F69E22",
        "description": "一个计算器",
        "glyph": "calculate"
    }"##;

    #[test]
    fn a_metadata_document_is_read_whole() {
        let meta = Meta::parse(META).expect("the package's own metadata");

        assert_eq!(meta.id, "calculator");
        assert_eq!(meta.name, "Calculator");
        assert_eq!(meta.name_zh, "计算器");
        assert_eq!(meta.version, "v1.0.0");
        assert_eq!(meta.abi, 1);
        assert_eq!(meta.memory_kb, 256);
        assert_eq!(meta.memory_pages(), 4);
        assert_eq!(meta.accent, (0xF6, 0x9E, 0x22));
        assert!(meta.allows(app_abi::perm::NETWORK));
        assert!(!meta.allows(app_abi::perm::NAS));
    }

    /// The defaults are the ones a small application needs, not zeroes that would make it unable to
    /// draw a tile.
    #[test]
    fn the_fields_a_small_application_may_omit_get_defaults() {
        let meta = Meta::parse(r#"{"id": "clock", "name": "Clock", "version": "v1"}"#)
            .expect("the minimum a package must say");

        assert_eq!(meta.name_zh, "Clock", "the English name stands in");
        assert_eq!(meta.abi, app_abi::ABI_MAJOR);
        assert_eq!(meta.memory_kb, 256);
        assert_eq!(meta.accent, (0x7C, 0x83, 0x92));
        assert_eq!(meta.description, "");
        assert!(!meta.allows(app_abi::perm::NETWORK));
    }

    /// The id is a directory name, and every way it could leave that directory is refused.
    #[test]
    fn an_id_that_is_not_a_directory_name_is_refused() {
        for id in ["../ota_0", "a/b", ".", "", "UPPER", "with space"] {
            let document = format!(r#"{{"id": "{id}", "name": "x", "version": "v1"}}"#);
            assert!(
                Meta::parse(&document).is_err(),
                "{id:?} must not be a package id"
            );
        }

        assert!(Meta::parse(r#"{"id": "power-log", "name": "x", "version": "v1"}"#).is_ok());
        assert!(Meta::parse(r#"{"id": "wifi_map", "name": "x", "version": "v1"}"#).is_ok());
    }

    /// A package may not buy itself more memory than the sandbox has, and a name longer than a tile
    /// can draw is cut where the tile would cut it.
    #[test]
    fn limits_are_enforced_at_the_border() {
        let too_much = r#"{"id": "x", "name": "x", "version": "v1", "memory_kb": 1024}"#;
        assert_eq!(Meta::parse(too_much), Err(MetaError::BadMemory(1024)));

        let long = r#"{"id": "x", "name": "abcdefghijklmnopqrstuvwxyz", "version": "v1"}"#;
        assert_eq!(
            Meta::parse(long).unwrap().name.chars().count(),
            MAX_NAME_CHARS
        );
    }

    #[test]
    fn a_colour_must_be_six_hex_digits_after_a_hash() {
        for accent in ["#F69E22", "#000000"] {
            let document =
                format!(r#"{{"id": "x", "name": "x", "version": "v1", "accent": "{accent}"}}"#);
            assert!(Meta::parse(&document).is_ok());
        }

        for accent in ["F69E22", "#F69E2", "#GGGGGG", ""] {
            let document =
                format!(r#"{{"id": "x", "name": "x", "version": "v1", "accent": "{accent}"}}"#);
            assert_eq!(Meta::parse(&document), Err(MetaError::BadColor));
        }
    }
}
