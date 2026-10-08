//! The settings app's own tests: what the pages say, through the widget tree.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. `iced_test`'s simulator
//! builds the *real* widget tree, finds a control by the text it is drawn with, presses it, and
//! hands back the message that produced. What the app costs the board — frames, damage, a finger
//! on the panel, a scroll — is asserted in `firmware/panel-tests`, where the panel is.

// These tests apply messages by hand, and a test has no loop to return the `Task` an update
// produces to. What that task does — send the page body back to the top — is the loop's business,
// not the state's. See `Settings::update`.
#![allow(unused_must_use)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iced::time::Instant;
use iced::Size;
use iced_test::Simulator;
use pomelo_hal::{ApInfo, Board, ScanState, WifiState, WifiStatus};
use pomelo_material_symbols::Icon;
use pomelo_widgets::touch_keyboard::{KeyAction, KeyboardMode};

use settings::{FontSizeTier, Language, Message, Settings, SettingsSection, ThemeMode};

/// The reads these tests make, in the names they were written with.
///
/// The app exposes one door — [`Settings::wifi`] — and the reads that used to sit on `Settings` are
/// the Wi-Fi page's own. Rather than rewrite two dozen call sites, the tests say what they need
/// here; that list is also the honest answer to "which reads do these tests use".
trait WifiReads {
    fn wifi_enabled(&self) -> bool;
    fn access_points(&self) -> &[ApInfo];
    fn wifi_status(&self) -> &WifiStatus;
    fn password_prompt(&self) -> Option<&ApInfo>;
    fn password(&self) -> &str;
    fn password_revealed(&self) -> bool;
    fn language(&self) -> Language;
    fn theme_mode(&self) -> ThemeMode;
    fn font_tier(&self) -> FontSizeTier;
}

impl WifiReads for Settings {
    fn wifi_enabled(&self) -> bool {
        self.wifi().enabled()
    }

    fn access_points(&self) -> &[ApInfo] {
        self.wifi().access_points()
    }

    fn wifi_status(&self) -> &WifiStatus {
        self.wifi().status()
    }

    fn password_prompt(&self) -> Option<&ApInfo> {
        self.wifi().prompt()
    }

    fn password(&self) -> &str {
        self.wifi().password()
    }

    fn password_revealed(&self) -> bool {
        self.wifi().revealed()
    }

    fn language(&self) -> Language {
        self.preferences().language
    }

    fn theme_mode(&self) -> ThemeMode {
        self.preferences().theme
    }

    fn font_tier(&self) -> FontSizeTier {
        self.preferences().font_tier
    }
}

/// The panel the layout is designed for. The app fills whatever it is given; a test has to pick a
/// size, and this is the one the design was drawn at.
const PANEL: f32 = 480.0;

/// The app in English, which is the language the labels below are written in.
///
/// The default is Chinese, so a test that means to assert a label has to say which language it is
/// asserting — otherwise it would pass or fail depending on a default it does not control.
fn english() -> Settings {
    let mut settings = Settings::new(simulated());
    settings.update(Message::SetLanguage(Language::English));
    settings
}

/// A board with no hardware behind it: the HAL's simulator, which is what every test here runs
/// against. Five access points, a scan that needs three `tick`s, and a connection that comes up as
/// soon as the network accepts the password.
fn simulated() -> Arc<Board> {
    Arc::new(Board::simulated())
}

/// Every page: the label of its row in the main list, the section it opens, and the title over it.
///
/// The two labels differ on purpose in three places — the row says `Battery` and the page says
/// `Battery & Power`, the row says `System` and the page says `About`, the row says
/// `Theme & Display` and the page says `Display & Theme` — which is the original's wording and
/// worth keeping: the row is what the section is *about*, the title is where you are.
const PAGES: [(&str, SettingsSection, &str); 7] = [
    ("Wi-Fi", SettingsSection::Wifi, "Wi-Fi"),
    ("Memory (RAM)", SettingsSection::Memory, "Memory"),
    ("Storage", SettingsSection::Storage, "Storage"),
    ("Battery", SettingsSection::Battery, "Battery & Power"),
    ("System", SettingsSection::SystemInfo, "About"),
    ("Theme & Display", SettingsSection::Theme, "Display & Theme"),
    ("Date & Time", SettingsSection::Time, "Date & Time"),
];

/// A screen tall enough for the whole main list, for a test that presses any row in it.
///
/// Deliberately generous, and more than twice the design panel's 480: the list is longer than the
/// panel and scrolls on the device, but the simulator has no scroll, so a press needs a screen that
/// shows all of it. It only has to be *taller than the list*, with room for the next few rounds of
/// tuning — a tight fit turns every metric change into a test failure, which is how it went from 800
/// to here when the list's top gap and its gaps between cards both grew.
const TALL: f32 = 1200.0;

/// `settings`' interface on the design panel.
fn interface(settings: &Settings) -> Simulator<'_, Message> {
    screen(settings, PANEL)
}

/// `settings`' interface on a screen `height` logical pixels tall.
///
/// The layout is flex, so it does not notice the difference; only what fits in the viewport does.
fn screen(settings: &Settings, height: f32) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            // By *generic* family: the tester's default is "Fira Sans", which only exists when
            // iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no room for.
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(PANEL, height),
        settings.view(),
    )
}

/// Presses `label` in a freshly built tree of `settings`' current state, and applies what came back.
///
/// On [`TALL`] and not the design panel: the main list is longer than the panel and scrolls, and the
/// simulator has no scroll — `find` only sees what is visible — so a press asks for a screen that
/// shows the whole list. What the panel's own height is for is the *layout*, which is what
/// `interface` checks.
fn press(settings: &mut Settings, label: &str) -> Result<(), iced_test::Error> {
    let mut ui = screen(settings, TALL);
    let _ = ui.click(label)?;

    for message in ui.into_messages() {
        settings.update(message);
    }

    Ok(())
}

#[test]
fn a_row_opens_its_section() -> Result<(), iced_test::Error> {
    let mut settings = english();

    assert_eq!(settings.section(), SettingsSection::Main);

    press(&mut settings, "Memory (RAM)")?;

    assert_eq!(settings.section(), SettingsSection::Memory);
    Ok(())
}

#[test]
fn every_row_opens_its_own_section() -> Result<(), iced_test::Error> {
    let mut settings = english();

    for (row, section, _) in PAGES {
        press(&mut settings, row)?;
        assert_eq!(settings.section(), section, "the row labelled {row}");

        // And back, which is also the only way the next row is on screen again.
        settings.update(Message::Back);
        assert_eq!(settings.section(), SettingsSection::Main, "after {row}");
    }

    Ok(())
}

/// The default is the original's language, and the language row *is* the switch.
///
/// Two things at once, and both are the point: a fresh app speaks Chinese, and pressing the row
/// hands it the other language — iced rebuilds the view from the new state, so the label changes
/// with no redraw machinery of our own.
#[test]
fn the_language_row_switches_between_chinese_and_english() -> Result<(), iced_test::Error> {
    let mut settings = Settings::new(simulated());

    assert_eq!(settings.language(), Language::Chinese);

    {
        let mut ui = interface(&settings);

        assert!(
            ui.find("内存 (RAM)").is_ok(),
            "the list is Chinese by default"
        );
        assert!(ui.find("Memory (RAM)").is_err(), "and not English");
    }

    press(&mut settings, "语言")?;

    assert_eq!(settings.language(), Language::English);

    let mut ui = interface(&settings);

    assert!(
        ui.find("Memory (RAM)").is_ok(),
        "the row switched it to English"
    );
    assert!(ui.find("内存 (RAM)").is_err(), "and Chinese is gone");

    Ok(())
}

/// The simulator's power backend, with a count of how often it was asked to restart.
///
/// The simulator's own `restart` is a no-op by necessity — a desktop process has no board to reset —
/// so a test that wants to see the *call* has to be the thing that sees it. Everything else this
/// backend reports is the simulator's, so the page drawn here is the page the desktop draws.
struct CountingPower {
    sim: pomelo_hal::sim::SimPower,
    restarts: Arc<AtomicUsize>,
}

impl pomelo_hal::PowerBackend for CountingPower {
    fn init(&mut self) -> Result<(), pomelo_hal::HalError> {
        self.sim.init()
    }

    fn battery_percent(&self) -> Result<u8, pomelo_hal::HalError> {
        self.sim.battery_percent()
    }

    fn is_charging(&self) -> Result<bool, pomelo_hal::HalError> {
        self.sim.is_charging()
    }

    fn battery_voltage_mv(&self) -> Result<u32, pomelo_hal::HalError> {
        self.sim.battery_voltage_mv()
    }

    fn chip_temperature_c(&self) -> Result<f32, pomelo_hal::HalError> {
        self.sim.chip_temperature_c()
    }

    /// The one thing this backend does on its own: remember that it was asked.
    fn restart(&self) -> Result<(), pomelo_hal::HalError> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// A board whose power backend is [`CountingPower`].
fn board_that_counts_restarts(restarts: Arc<AtomicUsize>) -> Arc<Board> {
    use pomelo_hal::sim::{SimAudio, SimImu, SimInput, SimMic, SimStorage, SimWifi};

    Arc::new(Board::from_backends(
        Box::new(CountingPower {
            sim: pomelo_hal::sim::SimPower::new(),
            restarts,
        }),
        Box::new(SimWifi::new()),
        Box::new(SimAudio::new()),
        Box::new(SimMic::new()),
        Box::new(SimImu::new()),
        Box::new(SimInput::new()),
        Box::new(SimStorage::new()),
    ))
}

/// The restart row asks before it acts, and only the tick acts.
///
/// A reset is the one thing on this list that pressing something else cannot undo, so the question
/// is the feature — and a test that only pressed the row and found the app still running would pass
/// both on one that had reset the board and on one that had not.
#[test]
fn the_restart_row_asks_before_it_restarts() -> Result<(), iced_test::Error> {
    let restarts = Arc::new(AtomicUsize::new(0));
    let mut settings = Settings::new(board_that_counts_restarts(Arc::clone(&restarts)));
    settings.update(Message::SetLanguage(Language::English));

    assert_eq!(
        restarts.load(Ordering::SeqCst),
        0,
        "nothing has been asked yet"
    );

    // The row, pressed as a finger presses it.
    press(&mut settings, "Restart")?;

    assert!(settings.is_confirming_restart(), "the question is up");
    assert_eq!(
        restarts.load(Ordering::SeqCst),
        0,
        "and asking it resets nothing"
    );

    {
        let mut ui = interface(&settings);
        assert!(
            ui.find("The device will restart now.").is_ok(),
            "the question says what it is about to do"
        );
    }

    // "No" puts it away and leaves the board alone.
    settings.update(Message::RestartCancel);

    assert!(!settings.is_confirming_restart(), "the question is closed");
    assert_eq!(restarts.load(Ordering::SeqCst), 0, "a refusal resets nothing");

    {
        let mut ui = interface(&settings);
        assert!(
            ui.find("The device will restart now.").is_err(),
            "and it is off the screen"
        );
    }

    // "Yes" is the only answer that does anything.
    settings.update(Message::Restart);
    settings.update(Message::RestartConfirm);

    assert!(!settings.is_confirming_restart());
    assert_eq!(restarts.load(Ordering::SeqCst), 1, "and the board was reset");
    assert_eq!(
        settings.section(),
        SettingsSection::Main,
        "the question is answered where it was asked, over the list"
    );

    Ok(())
}

/// Back closes the question rather than the page behind it.
///
/// The launcher intercepts `Message::Back` and calls `Settings::go_back` directly — the hardware
/// back key *is* that message — so a question that only `RestartCancel` closed would be one the back
/// key could not dismiss, and the list under it would go back to itself behind the dimming.
#[test]
fn back_closes_the_restart_question() {
    let restarts = Arc::new(AtomicUsize::new(0));
    let mut settings = Settings::new(board_that_counts_restarts(Arc::clone(&restarts)));

    settings.update(Message::Restart);
    assert!(settings.is_confirming_restart());

    assert!(
        settings.go_back().is_some(),
        "the press is consumed by the question"
    );

    assert!(!settings.is_confirming_restart(), "the question is closed");
    assert_eq!(
        settings.section(),
        SettingsSection::Main,
        "and the list is still the page"
    );
    assert_eq!(restarts.load(Ordering::SeqCst), 0, "and nothing was reset");
}

/// Switching the language is app-wide state, so a page that is open stays open.
#[test]
fn switching_the_language_keeps_the_page() {
    let mut settings = Settings::new(simulated());
    settings.update(Message::Open(SettingsSection::Memory));

    {
        let mut ui = interface(&settings);

        assert!(
            ui.find("内部 SRAM").is_ok(),
            "the page's labels are Chinese"
        );
    }

    settings.update(Message::SetLanguage(Language::English));

    let mut ui = interface(&settings);

    assert!(ui.find("Internal SRAM").is_ok(), "and English afterwards");
    assert_eq!(
        settings.section(),
        SettingsSection::Memory,
        "the page is still the one that was open"
    );
}

#[test]
fn back_returns_to_the_main_list() {
    let mut settings = english();

    settings.update(Message::Open(SettingsSection::Wifi));
    assert_eq!(settings.section(), SettingsSection::Wifi);

    settings.update(Message::Back);

    assert_eq!(settings.section(), SettingsSection::Main);
}

/// The main list is the one page with nowhere to go back to, so it must not offer to.
#[test]
fn the_main_list_has_no_back_button() {
    let settings = english();
    let mut ui = interface(&settings);

    assert!(
        ui.find("Back").is_err(),
        "there is nowhere to go back to from the list"
    );
}

/// Every page says where it is, and can go back via back message.
#[test]
fn every_page_is_titled_and_can_go_back() {
    let mut settings = english();

    for (row, section, title) in PAGES {
        settings.update(Message::Open(section));

        {
            let mut ui = interface(&settings);

            assert!(ui.find(title).is_ok(), "the {row} page is not titled");
            assert!(
                ui.find("Back").is_err(),
                "the {row} page has no on-screen Back button"
            );
        }
        assert!(
            settings.go_back().is_some(),
            "the {row} page can go back to the list"
        );
        assert_eq!(settings.section(), SettingsSection::Main);
    }

    settings.update(Message::Open(SettingsSection::Main));

    {
        let mut ui = interface(&settings);
        assert!(ui.find("Settings").is_ok(), "the list is titled");
        assert!(ui.find("Back").is_err());
    }
    assert!(
        settings.go_back().is_none(),
        "the list cannot go back further"
    );
}

/// The Wi-Fi page reads out what the switch is doing.
///
/// The switch itself is a `toggler` with no label — the text beside it is the label — so the panel
/// tests are where a finger on the switch is exercised. Here the model is asked directly, which is
/// what the switch's own message does, and the page is checked for saying so.
#[test]
fn the_wi_fi_page_says_what_the_switch_is_doing() -> Result<(), iced_test::Error> {
    let mut settings = english();
    settings.update(Message::Open(SettingsSection::Wifi));

    assert!(settings.wifi_enabled());

    {
        // Scoped, because the tree borrows the state it was built from: a simulator alive across
        // an `update` would be a tree of a state that no longer exists.
        let mut ui = interface(&settings);
        assert!(
            ui.find("Scanning…").is_ok(),
            "opening the page asks the radio for a scan, and the page says so"
        );
    }

    settings.update(Message::ToggleWifi);
    assert!(!settings.wifi_enabled());

    let mut ui = interface(&settings);
    assert!(
        ui.find("Wi-Fi is off").is_ok(),
        "and says so once it is off"
    );
    assert!(
        ui.find("192.168.1.108").is_err(),
        "the address is not still on the page"
    );
    assert!(
        ui.find("Scanning…").is_err(),
        "and there is nothing scanning behind a radio that is off"
    );

    Ok(())
}

/// The 24-hour switch is the other one, and its page is the one that shows it.
#[test]
fn the_time_page_follows_the_24_hour_switch() {
    let mut settings = Settings::new(simulated());
    settings.update(Message::Open(SettingsSection::Time));

    assert!(settings.is_24h_format(), "the original starts at 24-hour");

    {
        let mut ui = interface(&settings);
        assert!(ui.find("10:24").is_ok(), "24-hour has no suffix");
    }

    settings.update(Message::Toggle24Hour);
    assert!(!settings.is_24h_format());

    let mut ui = interface(&settings);
    assert!(ui.find("10:24 AM").is_ok(), "12-hour adds the suffix");
    assert!(ui.find("10:24").is_err());
}

/// The battery page shows the reading the platform pushed in, not one of its own.
#[test]
fn the_battery_page_shows_the_reading_it_was_given() {
    let mut settings = Settings::new(simulated());
    settings.update(Message::Open(SettingsSection::Battery));

    settings.set_battery(settings::Battery {
        percent: 42,
        charging: false,
        voltage_mv: 3_700,
        temperature_c: Some(31.4),
    });

    let mut ui = interface(&settings);

    assert!(ui.find("42%").is_ok(), "the level it was given");
    assert!(ui.find("3700 mV").is_ok(), "and the voltage");

    // The temperature is the last row of a page taller than the panel, so it needs a screen that
    // shows it: the simulator has no scroll, and a row below the fold is a row `find` never sees.
    let mut ui = screen(&settings, TALL);

    assert!(
        ui.find("31.4 C").is_ok(),
        "and the die temperature, in the decimal the chip reports it to"
    );
}

/// A platform that could not read a temperature says so rather than drawing a plausible one.
///
/// This is the bug the row was written to avoid: it used to be the literal `31.4 C`, so a board whose
/// PMIC could not be asked drew a number that looked exactly like a reading.
#[test]
fn the_battery_page_admits_when_it_has_no_temperature() {
    let mut settings = Settings::new(simulated());
    settings.update(Message::SetLanguage(Language::English));
    settings.update(Message::Open(SettingsSection::Battery));

    settings.set_battery(settings::Battery {
        percent: 42,
        charging: false,
        voltage_mv: 3_700,
        temperature_c: None,
    });

    let mut ui = screen(&settings, TALL);

    assert!(ui.find("none").is_ok(), "the row says it has no reading");
    assert!(
        ui.find("31.4 C").is_err(),
        "and the page does not invent one to fill the gap"
    );
}

// =============================================================================
// The storage page
// =============================================================================

/// A board whose slot holds what the caller says it does.
///
/// [`simulated`] has a card in it — the simulator's card is a 32 GB one with a little over half of it
/// used — and this is how a test asks for the other board, the one whose slot is empty. The empty
/// slot is the case the page has to survive, and the only way to get one is to build the board with a
/// backend that has no card: `Board::from_backends` is the composition root's door, and a test is a
/// composition root.
fn board_with_slot(card: bool) -> Arc<Board> {
    use pomelo_hal::sim::{SimAudio, SimImu, SimInput, SimMic, SimPower, SimStorage, SimWifi};

    let storage = if card {
        SimStorage::new()
    } else {
        SimStorage::without_card()
    };

    Arc::new(Board::from_backends(
        Box::new(SimPower::new()),
        Box::new(SimWifi::new()),
        Box::new(SimAudio::new()),
        Box::new(SimMic::new()),
        Box::new(SimImu::new()),
        Box::new(SimInput::new()),
        Box::new(storage),
    ))
}

/// The app on a board whose slot holds what the caller says it does, in English.
fn storage_app(card: bool) -> Settings {
    let mut settings = Settings::new(board_with_slot(card));
    settings.update(Message::SetLanguage(Language::English));
    settings
}

/// The page draws every volume it was told about, and the card is one of them.
#[test]
fn the_storage_page_shows_the_built_in_volume_and_the_card() -> Result<(), iced_test::Error> {
    let mut settings = storage_app(true);

    settings.update(Message::Open(SettingsSection::Storage));

    assert!(
        settings.storage().card().is_some(),
        "the simulator's slot has a card in it"
    );

    let mut ui = interface(&settings);

    assert!(
        ui.find("Built-in storage").is_ok(),
        "the board's own partition is a volume of its own"
    );
    assert!(ui.find("SD card").is_ok(), "and the card is the second");

    // Drawn from what the board said, not from a table in the page: the mount point, the filesystem
    // that mounted it, and the figure the card itself reported.
    assert!(ui.find("/internal").is_ok());
    assert!(ui.find("/sdcard").is_ok());
    assert!(ui.find("LittleFS").is_ok());
    assert!(ui.find("FATFS").is_ok());

    for volume in settings.storage().volumes() {
        // The app's own word for the unit, beside the figure the volume itself reported — which is
        // the point: the number is the board's, and a page that rounded its own would be a second
        // opinion on how full a filesystem is.
        let label = format!("{:.1}% Used", volume.used_percent());
        assert!(
            ui.find(label.as_str()).is_ok(),
            "the bar is labelled with the volume's own figure: {label}"
        );
    }

    assert!(
        ui.find("No card inserted").is_err(),
        "there is a card, so the page does not say there is not"
    );
    assert!(ui.find("Check again").is_ok(), "and the way to look again");

    Ok(())
}

/// A screen tall enough for the whole storage page: the flash map and both volume cards together.
///
/// Its own number and not [`TALL`]: the page grew a card when the flash map landed, and [`TALL`] was
/// sized for the main list. The simulator has no scroll, so anything below the fold is not found —
/// and a test that only ever found the first three rows would pass whatever the rows below them said.
const STORAGE_PAGE: f32 = 2400.0;

/// The page says where the whole chip went, and not only the part of it a filesystem can use.
///
/// The built-in volume is 3 MB; the board's flash is 16 MB. A page of volumes alone answers "how much
/// room is left" and never says what the other 13 MB are for, which is the question a person opening
/// a storage page is asking.
#[test]
fn the_storage_page_maps_the_whole_flash_chip() -> Result<(), iced_test::Error> {
    let mut settings = storage_app(true);

    settings.update(Message::Open(SettingsSection::Storage));

    let flash = settings
        .storage()
        .flash()
        .expect("the board's flash is always readable");

    // The chip, from the map's own numbers: 16 MB of NOR, and every byte of it claimed but the tail.
    assert_eq!(flash.total_bytes, 16 * 1024 * 1024);
    assert_eq!(flash.allocated_bytes(), 16 * 1024 * 1024 - 0xF_0000);

    let mut ui = screen(&settings, STORAGE_PAGE);

    assert!(ui.find("Internal flash").is_ok(), "the chip heads the page");

    // Every region the board reported is a row: the four partitions of its table, and the two
    // stretches that are not partitions at all. Drawn from the map, so a board with a different
    // table draws a different list.
    for region in &flash.regions {
        assert!(
            ui.find(region.label.as_str()).is_ok(),
            "the map lists the {} region",
            region.label
        );
    }

    // And what may be done with each, in a word. Four answers, because the one thing the page has to
    // make obvious is that most of this chip is not the user's to fill.
    assert!(ui.find("Read-only").is_ok(), "the firmware cannot be written");
    assert!(ui.find("Writable").is_ok(), "the built-in filesystem can");
    assert!(ui.find("Reserved").is_ok(), "and the bootloader belongs to the system");
    assert!(ui.find("Unallocated").is_ok(), "and the tail belongs to nobody");

    Ok(())
}

/// An empty slot is a line, not a bar at zero: a card that is not there is not a card with nothing on
/// it.
#[test]
fn the_storage_page_says_when_the_slot_is_empty() {
    let mut settings = storage_app(false);

    settings.update(Message::Open(SettingsSection::Storage));

    assert!(settings.storage().card().is_none());
    assert_eq!(
        settings.storage().volumes().len(),
        1,
        "the built-in volume is there whether a card is or not"
    );

    let mut ui = interface(&settings);

    assert!(ui.find("Built-in storage").is_ok());
    assert!(ui.find("No card inserted").is_ok());
    assert!(
        ui.find("SD card").is_err(),
        "and nothing draws the card that is not in the slot"
    );
    assert!(ui.find("/sdcard").is_err());
}

/// Opening the page is what asks the board.
///
/// The page is a reading and the reading has to be taken somewhere: the app carries no volumes at
/// all until someone opens the page, and the whole point of opening it is to get the answer to "is
/// the card I just put in there?".
#[test]
fn opening_the_storage_page_reads_the_board() {
    let mut settings = storage_app(true);

    assert!(
        settings.storage().volumes().is_empty(),
        "nothing has been asked yet"
    );

    settings.update(Message::Open(SettingsSection::Storage));

    assert_eq!(settings.storage().volumes().len(), 2);
    assert!(settings.storage().card().is_some());

    // And going back to the list does not un-ask it: the answer stands until the page is opened again.
    settings.update(Message::Back);
    assert_eq!(settings.section(), SettingsSection::Main);
    assert_eq!(settings.storage().volumes().len(), 2);
}

/// The button is the way to ask again — the whole of what a page that does not poll can offer.
#[test]
fn the_storage_page_can_be_asked_to_look_again() -> Result<(), iced_test::Error> {
    let mut settings = storage_app(false);

    settings.update(Message::Open(SettingsSection::Storage));
    assert!(settings.storage().card().is_none());

    // The press, through the widget tree, as a finger makes it.
    press(&mut settings, "Check again")?;

    assert!(
        settings.storage().card().is_none(),
        "still nothing in the slot"
    );
    assert_eq!(
        settings.storage().volumes().len(),
        1,
        "and the built-in volume is read again"
    );
    assert_eq!(settings.section(), SettingsSection::Storage, "and the page stayed open");

    Ok(())
}

// =============================================================================
// The Wi-Fi page
// =============================================================================

/// The board a Wi-Fi test drives, and the app reading it.
///
/// The `Arc` is shared on purpose: the page reads the radio through its own handle, and the test
/// drives it through this one. `SimWifi` only advances on `tick`, which is what makes these tests
/// deterministic instead of time-dependent.
fn wifi_app() -> (Arc<Board>, Settings) {
    let board = simulated();
    let mut settings = Settings::new(Arc::clone(&board));
    settings.update(Message::SetLanguage(Language::English));

    (board, settings)
}

/// The Wi-Fi page, open, with a finished scan behind it.
fn scanned() -> (Arc<Board>, Settings) {
    let (board, mut settings) = wifi_app();

    settings.update(Message::Open(SettingsSection::Wifi));
    for _ in 0..3 {
        board.tick();
    }
    frame(&mut settings, Duration::from_secs(1));

    (board, settings)
}

/// One frame of the Wi-Fi page's subscription.
///
/// The instant is the caller's, because the page reads the radio at most four times a second: a
/// test that read the clock itself would be testing the throttle half the time, and would pass or
/// fail depending on how busy the machine was.
fn frame(settings: &mut Settings, after: Duration) {
    settings.update(Message::WifiFrame(Instant::now() + after));
}

/// Opening the page is what starts the scan — and the page shows the radio, not a guess.
#[test]
fn opening_the_wifi_page_starts_a_scan() {
    let (board, mut settings) = wifi_app();

    settings.update(Message::Open(SettingsSection::Wifi));

    assert_eq!(board.wifi().scan_state(), ScanState::Scanning);
    assert!(settings.access_points().is_empty(), "nothing yet");

    frame(&mut settings, Duration::ZERO);
    assert!(
        settings.access_points().is_empty(),
        "the simulator needs three ticks before it has anything"
    );

    for _ in 0..3 {
        board.tick();
    }
    frame(&mut settings, Duration::from_secs(1));

    assert_eq!(settings.access_points().len(), 5);

    let found = settings.access_points();
    assert_eq!(found[0].ssid, "ESP-Rust-5G", "strongest first");
    assert!(
        found[0].rssi >= found[1].rssi && found[1].rssi >= found[2].rssi,
        "and the rest in order: {:?}",
        found.iter().map(|ap| ap.rssi).collect::<Vec<_>>()
    );
}

/// The list is drawn from what the scan found: names, the lock, and the way to scan again.
#[test]
fn the_list_shows_what_the_scan_found() -> Result<(), iced_test::Error> {
    let (_, settings) = scanned();

    let mut ui = interface(&settings);

    assert!(ui.find("ESP-Rust-5G").is_ok(), "a network by name");
    assert!(ui.find("OpenGuest").is_ok(), "the open one too");
    assert!(ui.find("secured").is_ok(), "and the lock, as a word");
    assert!(ui.find("Scan again").is_ok(), "the scan is not running");

    Ok(())
}

/// A locked network asks; the password is what connects it.
#[test]
fn a_locked_network_asks_for_its_password() {
    let (_, mut settings) = scanned();

    settings.update(Message::WifiSelect(0));
    assert_eq!(
        settings.password_prompt().map(|ap| ap.ssid.as_str()),
        Some("ESP-Rust-5G"),
        "a locked network opens the prompt"
    );

    // No password is refused by the radio, and the prompt is still there to say so.
    settings.update(Message::WifiConnect);
    assert_ne!(settings.wifi_status().state, WifiState::Connected);
    assert!(settings.password_prompt().is_some(), "still asking");

    {
        let mut ui = interface(&settings);
        assert!(
            ui.find("Connection failed").is_ok(),
            "and the page says why"
        );
    }

    for character in "hunter2".chars() {
        settings.update(Message::WifiKey(KeyAction::Char(character)));
    }
    assert_eq!(settings.password(), "hunter2");

    settings.update(Message::WifiConnect);

    assert!(settings.password_prompt().is_none(), "the prompt is done");
    assert_eq!(settings.wifi_status().state, WifiState::Connected);
    assert_eq!(settings.wifi_status().ssid, "ESP-Rust-5G");
}

/// The prompt's keyboard is the shared one: the mode switch is the keyboard's own state.
#[test]
fn the_prompt_has_the_whole_keyboard() -> Result<(), iced_test::Error> {
    let (_, mut settings) = scanned();

    settings.update(Message::WifiSelect(0));

    {
        let mut ui = interface(&settings);

        assert!(ui.find("q").is_ok(), "letters first");
        assert!(ui.find("123").is_ok(), "with a way to the numbers");
    }

    settings.update(Message::WifiKey(KeyAction::SwitchMode(KeyboardMode::Numbers)));

    {
        let mut ui = interface(&settings);

        assert!(ui.find("#+=").is_ok(), "the numeric page is up");
        assert!(ui.find("ABC").is_ok(), "and the way back");
        assert!(ui.find("q").is_err(), "the letters are gone");
    }

    Ok(())
}

/// An open network connects on the tap: there is nothing to ask for.
#[test]
fn an_open_network_connects_without_a_prompt() {
    let (_, mut settings) = scanned();

    // The simulator's open network is the fourth by signal strength.
    assert_eq!(settings.access_points()[3].ssid, "OpenGuest");

    settings.update(Message::WifiSelect(3));

    assert!(settings.password_prompt().is_none(), "no prompt");
    assert_eq!(settings.wifi_status().state, WifiState::Connected);
    assert_eq!(settings.wifi_status().ssid, "OpenGuest");

    let mut ui = interface(&settings);
    assert!(ui.find("Disconnect").is_ok(), "the page offers the way out");
}

/// The connection card is the radio's numbers, and disconnecting leaves the network.
#[test]
fn the_connection_card_is_the_boards_own_numbers() -> Result<(), iced_test::Error> {
    let (_, mut settings) = scanned();

    // Before anything is connected, the page must not be showing an address.
    {
        let mut ui = interface(&settings);
        assert!(ui.find("0.0.0.0").is_err(), "nothing is connected yet");
        assert!(ui.find("Disconnect").is_err(), "and there is no way out");
    }

    settings.update(Message::WifiSelect(3));

    let address = settings.wifi_status().ip.clone();

    {
        let mut ui = interface(&settings);
        assert!(ui.find(address.as_str()).is_ok(), "the address the radio reported: {address}");
        assert!(ui.find("Disconnect").is_ok(), "and the way out");
    }

    settings.update(Message::WifiDisconnect);

    assert_eq!(settings.wifi_status().state, WifiState::Disconnected);

    Ok(())
}

/// The password is written down when the connection comes up — and not before.
#[test]
fn the_password_is_written_down_once_it_works() {
    let (board, mut settings) = scanned();

    assert!(
        board.wifi().saved().is_none(),
        "a board that has never been on a network remembers nothing"
    );

    // The prompt opens empty, and an empty password is refused before the radio is asked.
    settings.update(Message::WifiSelect(0));
    settings.update(Message::WifiConnect);
    assert!(
        board.wifi().saved().is_none(),
        "an attempt the radio never saw is not written down"
    );

    for character in "hunter2".chars() {
        settings.update(Message::WifiKey(KeyAction::Char(character)));
    }
    settings.update(Message::WifiConnect);

    assert_eq!(settings.wifi_status().state, WifiState::Connected);

    let saved = board.wifi().saved().expect("the network that worked");
    assert_eq!(saved.ssid, "ESP-Rust-5G");
    assert_eq!(saved.password, "hunter2");
    assert!(saved.enabled, "the radio was on when it connected");
    assert!(saved.autoconnect, "and it is wanted again at boot");
}

/// The switch is a setting too, and it is written beside the network it belongs to.
#[test]
fn the_switch_is_written_beside_a_remembered_network() {
    let (board, mut settings) = scanned();

    settings.update(Message::WifiSelect(0));
    for character in "hunter2".chars() {
        settings.update(Message::WifiKey(KeyAction::Char(character)));
    }
    settings.update(Message::WifiConnect);

    assert!(board.wifi().saved().unwrap().enabled, "on, having connected");

    settings.update(Message::ToggleWifi);
    assert!(!board.wifi().saved().unwrap().enabled, "off, and written down");

    settings.update(Message::ToggleWifi);
    assert!(board.wifi().saved().unwrap().enabled, "and on again");
}

/// The switch is the radio's, not the page's: turning it off empties the list.
#[test]
fn the_switch_turns_the_radio_off() -> Result<(), iced_test::Error> {
    let (board, mut settings) = scanned();

    settings.update(Message::ToggleWifi);

    assert!(!settings.wifi_enabled());
    assert!(!board.wifi().is_enabled(), "the radio itself is off");
    assert!(settings.access_points().is_empty(), "and nothing to list");

    let mut ui = interface(&settings);
    assert!(ui.find("Wi-Fi is off").is_ok());

    Ok(())
}

/// The prompt is a modal: a press that lands where the list is never reaches the list.
///
/// This is the one thing about stacking a prompt over a page that has to be asserted rather than
/// assumed — a layer that does not capture is a layer the page answers through, and the page behind
/// this prompt is a screen of buttons. What the press *does* instead is the backdrop's business
/// (it cancels); what matters is that the list did not act.
#[test]
fn the_prompt_takes_the_finger_from_the_list() -> Result<(), iced_test::Error> {
    let (_, mut settings) = scanned();

    settings.update(Message::WifiSelect(0));
    assert!(settings.password_prompt().is_some(), "the prompt is up");

    // `OpenGuest` is a row of the list behind the prompt, and pressing it would connect.
    //
    // On [`TALL`], like [`press`]: the Wi-Fi page's own head puts that row below the design panel's
    // edge, and the simulator has no scroll — what is being asserted is the `stack`, not the fit.
    let mut ui = screen(&settings, TALL);
    let _ = ui.click("OpenGuest")?;

    for message in ui.into_messages() {
        settings.update(message);
    }

    assert_ne!(
        settings.wifi_status().ssid, "OpenGuest",
        "the press went to the list behind the prompt"
    );
    assert_ne!(settings.wifi_status().state, WifiState::Connected);

    Ok(())
}

/// Every sub-page's head carries a way back, and it is the press the board's back key makes.
///
/// A page that can only be left by a hardware key is a page with no way out on a board that has
/// none, so the button is not decoration — it is the exit. It is found the way a `find` finds
/// anything, by the text it is drawn with, and the text of an icon button is its glyph.
#[test]
fn a_sub_page_head_carries_the_way_back() -> Result<(), iced_test::Error> {
    let mut settings = english();

    press(&mut settings, "Memory (RAM)")?;
    assert_eq!(settings.section(), SettingsSection::Memory);

    press(&mut settings, Icon::ARROW_BACK.glyph())?;
    assert_eq!(
        settings.section(),
        SettingsSection::Main,
        "the head's button goes back"
    );

    Ok(())
}

/// The hardware back button closes the prompt before it leaves the page.
///
/// This is the launcher's path, not `update`'s: the launcher intercepts `Message::Back` and calls
/// `go_back` itself, so a prompt that only closed inside `update` would never close on the board.
#[test]
fn the_hardware_back_closes_the_prompt_first() {
    let (_, mut settings) = scanned();

    settings.update(Message::WifiSelect(0));
    assert!(settings.password_prompt().is_some());

    assert!(
        settings.go_back().is_some(),
        "the press is consumed by the prompt"
    );
    assert!(settings.password_prompt().is_none());
    assert_eq!(
        settings.section(),
        SettingsSection::Wifi,
        "and the page is still up"
    );

    assert!(
        settings.go_back().is_some(),
        "the next press leaves the page"
    );
    assert_eq!(settings.section(), SettingsSection::Main);
}

/// The prompt can show what was typed, and go back to dots.
#[test]
fn the_prompt_can_show_what_was_typed() -> Result<(), iced_test::Error> {
    let (_, mut settings) = scanned();

    settings.update(Message::WifiSelect(0));

    for character in "hunter2".chars() {
        settings.update(Message::WifiKey(KeyAction::Char(character)));
    }

    {
        let mut ui = interface(&settings);
        assert!(ui.find("hunter2").is_err(), "dots, not the password");
        assert!(ui.find("Show password").is_ok(), "and the way to reveal it");
    }

    settings.update(Message::WifiReveal);
    assert!(settings.password_revealed());

    {
        let mut ui = interface(&settings);
        assert!(ui.find("hunter2").is_ok(), "the password itself");
        assert!(ui.find("Hide").is_ok(), "and the way back");
    }

    settings.update(Message::WifiReveal);
    assert!(!settings.password_revealed(), "hidden again");

    // And a prompt opened later starts hidden: a password left in the clear is not a state to
    // inherit from the last one.
    settings.update(Message::WifiReveal);
    settings.update(Message::WifiCancel);
    settings.update(Message::WifiSelect(1));

    assert!(!settings.password_revealed());
    assert!(settings.password().is_empty(), "and empty");

    Ok(())
}

/// The Wi-Fi page asks for frames only while it is waiting on the radio.
///
/// This is the regression test for what a real board showed: the app subscribed to every frame to
/// read the radio four times a second, and the host runs the view and the damage diff for every
/// message a subscription produces — so an idle password prompt printed a `[phase]` line a
/// millisecond, with `glyphs 0/0` and `clear 0.00`, for as long as it was open.
#[test]
fn the_frames_stop_when_the_radio_is_no_longer_waited_on() {
    let (board, mut settings) = wifi_app();

    assert!(
        !settings.is_waiting_on_wifi(),
        "the page is not even open, so nothing is waiting"
    );

    settings.update(Message::Open(SettingsSection::Wifi));
    assert!(settings.is_waiting_on_wifi(), "a scan is in flight");

    for _ in 0..3 {
        board.tick();
    }

    assert!(
        settings.is_waiting_on_wifi(),
        "the radio is done but the page has not read the results yet"
    );

    frame(&mut settings, Duration::from_secs(1));

    assert!(
        !settings.is_waiting_on_wifi(),
        "the page has what it was waiting for, so the frames must stop"
    );

    // Merely being *open* does not re-arm it — that is the whole point.
    frame(&mut settings, Duration::from_secs(10));
    assert!(!settings.is_waiting_on_wifi());

    // A radio that is off has nothing to wait for either.
    settings.update(Message::ToggleWifi);
    assert!(!settings.is_waiting_on_wifi());
}

#[test]
fn the_theme_can_be_switched() {
    let mut settings = english();
    assert_eq!(settings.theme_mode(), ThemeMode::Dark);

    settings.update(Message::SetTheme(ThemeMode::Light));
    assert_eq!(settings.theme_mode(), ThemeMode::Light);

    settings.update(Message::SetTheme(ThemeMode::Dark));
    assert_eq!(settings.theme_mode(), ThemeMode::Dark);
}

#[test]
fn the_font_tier_can_be_cycled() {
    let mut settings = english();
    assert_eq!(settings.font_tier(), FontSizeTier::Standard);
    assert_eq!(settings.font_tier().base_size(), 24.0);

    settings.update(Message::CycleFontTier);
    assert_eq!(settings.font_tier(), FontSizeTier::Large);
    assert_eq!(settings.font_tier().base_size(), 30.0);

    settings.update(Message::CycleFontTier);
    assert_eq!(settings.font_tier(), FontSizeTier::ExtraSmall);
    assert_eq!(settings.font_tier().base_size(), 18.0);

    settings.update(Message::CycleFontTier);
    assert_eq!(settings.font_tier(), FontSizeTier::Small);
    assert_eq!(settings.font_tier().base_size(), 20.0);

    settings.update(Message::CycleFontTier);
    assert_eq!(settings.font_tier(), FontSizeTier::Standard);
    assert_eq!(settings.font_tier().base_size(), 24.0);
}

