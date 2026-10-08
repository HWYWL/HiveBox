//! The main settings list: three cards of tiles.

use iced::widget::text;

use pomelo_material_symbols::Icon;

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::{Card, Tile};
use crate::pages::common::{body, page, title, UI};
use crate::style;
use crate::{Message, SettingsSection};
use pomelo_widgets::SystemPreferences;

/// The main list, in `language`.
///
/// The list is a way *in*, not a readout. The one tile that says anything about this machine is the
/// Wi-Fi one — which network the radio is on is not visible anywhere else — and the pages behind the
/// others are where their facts belong. A value here would be a second place for the same fact to be
/// wrong.
///
/// `connected` is the SSID the radio is on, or `None` if it is not on one. The labels are
/// translated; a network's name is not, because it is the network's, not the interface's. See
/// [`crate::i18n`].
pub(crate) fn main_page<'a>(preferences: SystemPreferences, connected: Option<&str>) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    let wifi = connected
        .unwrap_or_else(|| language.text(Key::NotConnected))
        .to_string();

    // Three cards, grouped the way the list is used: one thing you turn on, three things whose size
    // you come to look at, and three things you set. A card's tiles get a hairline between them; the
    // cards get `CARD_GAP`.
    let connectivity = Card::new(theme).tile(
        Tile::section(Icon::WIFI, SettingsSection::Wifi, language.text(Key::Wifi))
            .secondary(text(wifi)),
    );

    let device = Card::new(theme)
        .tile(Tile::section(
            Icon::MEMORY,
            SettingsSection::Memory,
            language.text(Key::Memory),
        ))
        .tile(Tile::section(
            Icon::STORAGE,
            SettingsSection::Storage,
            language.text(Key::Storage),
        ))
        .tile(Tile::section(
            Icon::BATTERY_FULL,
            SettingsSection::Battery,
            language.text(Key::BatteryRow),
        ));

    let system = Card::new(theme)
        .tile(Tile::section(
            Icon::INFO,
            SettingsSection::SystemInfo,
            language.text(Key::System),
        ))
        .tile(Tile::section(
            Icon::PALETTE,
            SettingsSection::Theme,
            language.text(Key::Theme),
        ))
        .tile(Tile::section(
            Icon::SCHEDULE,
            SettingsSection::Time,
            language.text(Key::Time),
        ))
        .tile(Tile::action(
            Icon::TRANSLATE,
            style::IconColor::Blue,
            // The row *is* the switch: pressing it hands the app the other language, and the
            // interface you are reading the row in *is* the value — a name here would repeat it.
            language.text(Key::Language),
            Message::SetLanguage(language.other()),
        ));

    page(
        title(language.text(Key::Settings), theme),
        body(vec![
            connectivity.view(),
            device.view(),
            system.view(),
        ]),
    )
}
