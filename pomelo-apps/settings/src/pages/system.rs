//! The system information page: model, OS, firmware, CPU, display and renderer.
//!
//! # Which of these rows is a reading and which is a specification
//!
//! One row is a *reading* — the firmware line, which is what the image calls itself, and it is taken
//! from the same place the readout at the top of the list takes it ([`SystemPanel::firmware`]). It
//! used to be the OS line, and before that the literal `Pomelo OS v0.2.0 (Build 2026.09)`, which is
//! a sentence about an image nobody has: the version the build stamps in is a git hash, and the name
//! is whatever CMake was told.
//!
//! The rest are **specifications**: what this board, or this software, is on paper. The OS row is
//! the one about software rather than hardware — this project is a customisation of the upstream
//! `pomelo-ui` ([`Key::OsBasedOnPomeloUi`]) — and that is a fact about the code, which is exactly
//! what the chip cannot be asked for. It is a spec for the same reason `Waveshare ESP32-S3 AMOLED
//! 2.16"` is: nothing on the silicon knows either one.
//!
//! Those stay written down, because a written-down fact that no code reads back is a *spec* rather
//! than a second copy of a reading — and the distinction is worth the words: a spec is wrong only if
//! the hardware or the upstream project changes, a stale reading is wrong the moment it is drawn.

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::Header;
use crate::pages::common::{body, detail_card, page, UI};
use crate::pages::summary::SystemPanel;
use crate::SettingsSection;
use pomelo_hal::FirmwareInfo;
use pomelo_material_symbols::Icon;
use pomelo_widgets::{Language, SystemPreferences};

pub(crate) fn system_page<'a>(
    preferences: SystemPreferences,
    readout: &SystemPanel,
) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    let details = vec![
        (
            language.text(Key::Model),
            "Waveshare ESP32-S3 AMOLED 2.16\"".to_string(),
        ),
        (
            language.text(Key::Os),
            language.text(Key::OsBasedOnPomeloUi).to_string(),
        ),
        (
            language.text(Key::Firmware),
            image(language, readout.firmware()),
        ),
        (
            language.text(Key::Cpu),
            "Xtensa Dual-Core LX7 @ 240MHz".to_string(),
        ),
        (
            language.text(Key::Display),
            "CO5300 480x480 QSPI AMOLED".to_string(),
        ),
        (language.text(Key::Colour), "100% DCI-P3".to_string()),
        (
            language.text(Key::Touch),
            "CST816 capacitive (I2C)".to_string(),
        ),
        (
            language.text(Key::Renderer),
            "iced widgets over pomelo-gfx".to_string(),
        ),
        (
            language.text(Key::Flash),
            "16 MB Quad-SPI Flash".to_string(),
        ),
    ];

    page(
        Header::section(
            Icon::INFO,
            SettingsSection::SystemInfo,
            language.text(Key::About),
        )
        .view(theme),
        body(vec![detail_card(details, theme)]),
    )
}

/// The image as it names itself, or a word when the board could not say.
///
/// The name and the version together, because either alone is half an answer: `firmware` is what
/// this project's build is called and says nothing about which build it is, and `6de00de-dirty` is
/// which build it is with nothing to attach it to. Nothing here is translated — an image's name is
/// its own, not the interface's — and a board that cannot describe itself says so rather than
/// showing the version this code was written against.
fn image(language: Language, firmware: Option<&FirmwareInfo>) -> String {
    match firmware {
        Some(firmware) => format!("{} {}", firmware.name, firmware.release()),
        None => language.text(Key::None).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reading is drawn, and its absence is drawn as the absence it is.
    #[test]
    fn the_image_is_named_or_admitted_to_be_unknown() {
        let firmware = FirmwareInfo {
            name: String::from("pomelo"),
            version: String::from("1.2.3"),
            built: None,
        };

        assert_eq!(image(Language::English, Some(&firmware)), "pomelo 1.2.3");
        assert_eq!(image(Language::English, None), "none");
        assert_eq!(image(Language::Chinese, None), "无");
    }

    /// A build made between two tags is drawn as the release it is a build *of*: the commits it has
    /// moved past that tag are the build's business, and this row's business is the version.
    #[test]
    fn a_build_between_tags_is_drawn_as_its_release() {
        let between_tags = FirmwareInfo {
            name: String::from("firmware"),
            version: String::from("v0.1.1-20261010-3-g16414fb"),
            built: Some(String::from("2026-10-10 14:32:05")),
        };

        assert_eq!(
            image(Language::English, Some(&between_tags)),
            "firmware v0.1.1-20261010"
        );
    }
}
