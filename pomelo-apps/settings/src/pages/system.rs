//! The system information page: model, OS, CPU, display and renderer.

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::Header;
use crate::pages::common::{body, detail_card, page, UI};
use crate::SettingsSection;
use pomelo_material_symbols::Icon;
use pomelo_widgets::SystemPreferences;

pub(crate) fn system_page<'a>(preferences: SystemPreferences) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    let details = vec![
        (
            language.text(Key::Model),
            "Waveshare ESP32-S3 AMOLED 2.16\"".to_string(),
        ),
        (
            language.text(Key::Os),
            "Pomelo OS v0.2.0 (Build 2026.09)".to_string(),
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
