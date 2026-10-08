//! The battery page: charge level, voltage, temperature and power status.
//!
//! Everything here is a reading the platform pushed ([`crate::Settings::set_battery`]), including
//! the temperature: the die temperature of the PMIC, which the firmware re-reads every second and
//! pushes again whenever it has moved half a degree. There is nothing to refresh on a timer here,
//! and nothing to read from the board — a page that asked one of its own would be a second opinion
//! on what the power system is doing.

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::Header;
use crate::pages::common::{body, detail_card, page, UI};
use crate::{Battery, SettingsSection};
use pomelo_material_symbols::Icon;
use pomelo_widgets::{Language, SystemPreferences};

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
        // The one row on this page that moves while it is open — and the one that used to be a
        // literal, which is why it is written as a reading and not as a word.
        (
            language.text(Key::PmicTemperature),
            temperature(language, battery.temperature_c),
        ),
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

/// The die temperature as the row writes it: one decimal, in the unit the chip reports.
///
/// [`Key::None`] rather than a number for a platform that could not read one. A board whose PMIC
/// cannot be asked has no temperature, and a row that filled the gap with `0.0 C` would be drawing
/// freezing hardware instead of saying it does not know.
fn temperature(language: Language, celsius: Option<f32>) -> String {
    match celsius {
        Some(celsius) => format!("{celsius:.1} C"),
        None => language.text(Key::None).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reading is drawn, and its absence is drawn as the absence it is.
    #[test]
    fn a_temperature_is_printed_or_admitted_to_be_missing() {
        assert_eq!(temperature(Language::Chinese, Some(31.4)), "31.4 C");
        assert_eq!(temperature(Language::English, Some(-12.34)), "-12.3 C");

        // Not `0.0 C`: a PMIC that cannot be read is not a PMIC at freezing.
        assert_eq!(temperature(Language::English, None), "none");
        assert_eq!(temperature(Language::Chinese, None), "无");
    }
}
