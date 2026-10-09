//! The main settings list: a readout, then three cards of tiles.

use iced::widget::text;

use pomelo_material_symbols::Icon;

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::{Card, Tile};
use crate::pages::common::{body, page, title, UI};
use crate::pages::summary::{summary_panel, SystemPanel};
use crate::style;
use crate::{Message, SettingsSection};
use pomelo_widgets::SystemPreferences;

/// The main list, in `language`.
///
/// The list is a way *in*, not a readout: the pages behind the rows are where their facts belong,
/// and a value on a row would be a second place for the same fact to be wrong. The one tile that
/// says anything about this machine is the Wi-Fi one, because which network the radio is on is not
/// visible anywhere else.
///
/// The readout above it is not the exception that breaks that rule, because it is not a row. Nothing
/// on it opens anything, and no page behind the list draws what it draws: a percentage of the heap
/// rather than the heap's byte counts, a die temperature, the fill of the built-in volume rather than
/// each volume's, a date. It is the summary a person checks *before* deciding which of those pages
/// to open — which is why it is above them and not among them, and why it is one card rather than a
/// row per number. Its own file is [`crate::pages::summary`].
///
/// `connected` is the SSID the radio is on, or `None` if it is not on one. The labels are
/// translated; a network's name is not, because it is the network's, not the interface's. See
/// [`crate::i18n`]. `panel` is the readout's own reading of the board, taken by the app.
///
/// Two rows of the last card are not destinations, and they carry no chevron-shaped promise: the
/// language row *is* the switch it looks like a setting for, and the restart row asks a question
/// rather than opening a page. Neither is a reading of this machine — one is about the interface,
/// the other is about the whole of it — so both sit after the three sections that are, and the
/// system readout comes last of all: what the machine *is* is the one thing here nobody opens
/// Settings to change, and the foot of the list is where the things you do not come for go.
pub(crate) fn main_page<'a>(
    preferences: SystemPreferences,
    connected: Option<&str>,
    panel: &SystemPanel,
) -> UI<'a> {
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
            Icon::RESTART_ALT,
            // Red, and above the language row: this is the one row in the list that can interrupt
            // what a person is doing, and the colour is the only warning a list row has to give.
            style::IconColor::Red,
            language.text(Key::Restart),
            Message::Restart,
        ))
        .tile(Tile::action(
            Icon::TRANSLATE,
            style::IconColor::Blue,
            // The row *is* the switch: pressing it hands the app the other language, and the
            // interface you are reading the row in *is* the value — a name here would repeat it.
            language.text(Key::Language),
            Message::SetLanguage(language.other()),
        ))
        .tile(Tile::section(
            Icon::INFO,
            SettingsSection::SystemInfo,
            language.text(Key::System),
        ));

    page(
        title(language.text(Key::Settings), theme),
        body(vec![
            summary_panel(preferences, panel),
            connectivity.view(),
            device.view(),
            system.view(),
        ]),
    )
}
