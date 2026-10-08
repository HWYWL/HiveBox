//! The battery page: charge level, voltage and power status.

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::Header;
use crate::pages::common::{body, detail_card, page, UI};
use crate::{Battery, SettingsSection};
use pomelo_material_symbols::Icon;
use pomelo_widgets::SystemPreferences;

pub(crate) fn battery_page<'a>(preferences: SystemPreferences, battery: Battery) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    let details = vec![
        (language.text(Key::Level), format!("{}%", battery.percent)),
        (
            language.text(Key::Power),
            if battery.charging {
                "USB-C".to_string()
            } else {
                "Battery".to_string()
            },
        ),
        (
            language.text(Key::Charging),
            if battery.charging {
                language.text(Key::Charging).to_string()
            } else {
                language.text(Key::NotCharging).to_string()
            },
        ),
        (
            language.text(Key::Voltage),
            format!("{} mV", battery.voltage_mv),
        ),
        (language.text(Key::Health), "98% (excellent)".to_string()),
        (language.text(Key::Pmic), "AXP2101 (I2C 0x34)".to_string()),
        (language.text(Key::LowPowerMode), "off (60Hz)".to_string()),
        (language.text(Key::Temperature), "31.4 C".to_string()),
    ];

    page(
        Header::section(
            Icon::BATTERY_FULL,
            SettingsSection::Battery,
            language.text(Key::Battery),
        )
        .view(theme),
        body(vec![detail_card(details, theme)]),
    )
}
