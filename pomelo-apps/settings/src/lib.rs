//! The settings app, built from iced widgets — **a standard iced program**.
//!
//! A port of the settings app this replaces: the same section list, the same grouped cards and
//! the same switches, rebuilt from `iced`'s own widgets.
//!
//! Two decisions in the port are deliberate and worth stating before the code.
//!
//! # The labels are bilingual
//!
//! The interface speaks Simplified Chinese and English, and the language is part of the app's
//! state ([`Language`]): a switch is a state change, and iced's own diff repaints only the rows
//! whose text actually changed. [`i18n`] says why this is a static table and an exhaustive `match`
//! rather than `rust-i18n` or `fluent`.
//!
//! Chinese is the default, because it is the original: this app is a port of a Chinese settings
//! app, and its labels were **only** ASCII because the old 16 KiB Latin subset could not draw a
//! CJK glyph at all. The font carries 4008 Chinese characters now (see
//! `vendor/iced-pomelo-winit/fonts/README.md`), so the originals are back, with the English port
//! kept as the second language.
//!
//! What is translated is the interface: sections, row labels, switches, buttons, footnotes, and
//! the words in values (`未连接`, `已用`). What is not translated is **data** — a network's name, an
//! IP, a MAC, byte counts, a model number, a time. Those are facts about this machine, and the
//! pages below still write them as literals.
//!
//! # The board is injected, and the Wi-Fi page is why
//!
//! The app is handed an `Arc<pomelo_hal::Board>` by whoever builds it — the launcher passes the one
//! it already shares with every app it hosts, and `main.rs` builds the desktop simulator — which is
//! the same shape `music-player` uses. The port deliberately took no board, because every page it
//! had was a *reading*: a battery percentage is served by [`Settings::set_battery`], which the
//! platform pushes into, and a pushed value is testable with nothing but a widget tree.
//!
//! Wi-Fi is not a reading. Scanning and connecting are a conversation — the page asks the radio
//! (`Board::wifi().scan_start()`), waits, and asks again — and a push-only channel would put that
//! conversation in the *launcher*, which is the one thing hosting an app as a widget exists to
//! avoid. So the board is here, and so is the polling ([`Settings::subscription`]).
//!
//! Storage is not a reading either, and it is the same board: whether there is a card in the slot is
//! a question with an answer that changes while the box is running, and nothing outside the board
//! knows it. What is different is how often it is asked — a scan is waited on, a slot is probed when
//! the page opens — which is [`pages::storage`]'s subject rather than this one's.
//!
//! The battery page takes its numbers from the platform: a reading the platform owns stays pushed,
//! and the page reads nothing itself. The temperature it draws is one of those readings — the
//! PMIC's own die temperature, since this board's battery has no NTC to read — and the firmware
//! pushes a new one whenever it moves half a degree.
//!
//! # The restart question
//!
//! One row of the list is not a way in. Resetting the board is the only thing this app can do that
//! a person cannot undo by pressing something else, so it asks before it acts, and the question is
//! a modal over whatever page is up — see [`pages::dialog`].
//!
//! That row is also the one place where asking the board for something *does* something rather than
//! reading it: [`Settings::restart`] is a call the app makes because there is nowhere else in this
//! stack that can make it — the launcher hosting this app is a widget tree, not a supervisor, and
//! resetting a chip is not something a supervisor would do either.
//!
//! # Scrolling
//!
//! Every page is a 44px navigation bar plus an `iced_widget::scrollable` body, so a page taller
//! than the panel scrolls. Switching pages opens the new page at the top: the body is one
//! `scrollable` with one id, and [`Settings::update`] sends it `operation::snap_to` when the page
//! under it changes. The widget that used to do this (`page.rs`) existed only because this platform
//! could not run a widget operation at all; it can now, so the reset is iced's own.

mod i18n;
pub mod pages;
pub mod style;

use std::sync::Arc;

use iced::theme::Palette;
use iced::time::Instant;
use iced::widget::{operation, scrollable};
use iced::{Element, Subscription, Task, Theme};
use pomelo_hal::{Board, WifiState};

pub use pomelo_widgets::{FontSizeTier, Language, SystemPreferences, ThemeMode};
pub use i18n::{Key, LanguageExt};

use pages::common::BODY;
use pages::storage::Storage;
use pages::summary::SystemPanel;
use pages::wifi::Wifi;
use pomelo_widgets::touch_keyboard::KeyAction;

pub use style::SCREEN;

/// Sends the page body back to the top.
///
/// A change of section is a different page inside the *same* `scrollable`: the tree node survives
/// the rebuild, so its offset would survive too, and the sub-page would open wherever the previous
/// one had been scrolled to. Asking for the top is a widget operation, which is what iced has for
/// exactly this.
fn to_top() -> Task<Message> {
    operation::snap_to(BODY, scrollable::RelativeOffset::START)
}

/// The element type every builder below speaks.
type UI<'a> = Element<'a, Message>;

/// Which page is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    /// The grouped list of sections.
    Main,
    /// Wi-Fi: a switch and the current network.
    Wifi,
    /// Memory: a usage bar and the heap numbers.
    Memory,
    /// Storage: a usage bar and the numbers of every mounted volume, the card included.
    Storage,
    /// Battery: the reading the platform last pushed.
    Battery,
    /// System information.
    SystemInfo,
    /// Display and theme.
    Theme,
    /// Date and time.
    Time,
}

/// What the app reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A row of the main list was pressed.
    Open(SettingsSection),
    /// The back button (or the hardware back key) was pressed.
    Back,
    /// The Wi-Fi switch was flipped.
    ToggleWifi,
    /// The 24-hour-time switch was flipped.
    Toggle24Hour,
    /// The language row was pressed, and the interface is now in `Language`.
    SetLanguage(Language),
    /// Set the appearance theme mode.
    SetTheme(ThemeMode),
    /// Cycle to the next font size tier.
    CycleFontTier,
    /// The storage page's "check again": re-probe the card slot, and read what is mounted.
    StorageDetect,
    /// The restart row was pressed. Asks the question; resets nothing.
    Restart,
    /// The restart question's "yes": reset the board.
    RestartConfirm,
    /// The restart question's "no": put the question away and leave the board alone.
    RestartCancel,

    /// The Wi-Fi page asked for a fresh scan: the page was opened, or the scan row was pressed.
    WifiScan,
    /// A frame arrived while the Wi-Fi page was open — the cue to read the radio.
    ///
    /// The instant is the frame's, so the page can read the radio a few times a second instead of
    /// once per frame: iced's `time::every` needs a tokio or smol runtime and this stack has
    /// neither, so a frame *is* the clock here (see [`Settings::subscription`]).
    WifiFrame(Instant),
    /// A network in the list was pressed.
    WifiSelect(usize),
    /// A key of the password prompt's keyboard.
    WifiKey(KeyAction),
    /// The prompt's connect button — or its return key.
    WifiConnect,
    /// The prompt's cancel button.
    WifiCancel,
    /// The prompt's show/hide button: draw the password in the clear, or as dots again.
    WifiReveal,
    /// The connection card's disconnect row.
    WifiDisconnect,
}

/// The battery reading the battery page shows.
///
/// The platform owns the real numbers and pushes them in with [`Settings::set_battery`]; the values
/// here are what the original read from a bench board. There is deliberately no
/// `pomelo_hal` dependency — see the crate documentation.
///
/// No `Eq`, unlike every other type in this app: a temperature is a `f32`, and a float that claims
/// to be totally ordered is a promise it cannot keep. `PartialEq` is what a test compares with, and
/// all this needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    /// Charge, 0 to 100.
    pub percent: u8,
    /// Whether a charger is connected.
    pub charging: bool,
    /// Pack voltage in millivolts.
    pub voltage_mv: u16,
    /// The PMIC's die temperature, when the platform could read one.
    ///
    /// `None` and not a number with a default: a board whose PMIC cannot be asked has no
    /// temperature, and `0.0` on the page would be a reading of freezing hardware. There is no
    /// bench value here the way there is for the other three — a temperature is the one reading this
    /// page shows that changes on its own, and a plausible-looking constant is exactly what it must
    /// not be.
    pub temperature_c: Option<f32>,
}

impl Default for Battery {
    fn default() -> Self {
        Self {
            percent: 88,
            charging: true,
            voltage_mv: 4_100,
            temperature_c: None,
        }
    }
}

/// The settings app.
///
/// What is left here is what the *app* has: which page is up, the preferences every page draws
/// from, the battery reading the platform pushes in, and the routing between them. A page's own
/// state and rules live with its widgets — see [`pages::wifi`], the one page that has any.
pub struct Settings {
    section: SettingsSection,
    is_24h_format: bool,
    battery: Battery,
    /// The system preferences (language, theme, font size tier).
    preferences: SystemPreferences,
    /// The board, for the one thing this app does to it rather than reads from it: a restart.
    ///
    /// The pages that talk to hardware hold their own handle — they were written first and a page
    /// that owns its conversation is what the module docs describe. This one is the app's, because
    /// the reset is the app's: no page owns it.
    board: Arc<Board>,
    /// The Wi-Fi page, which owns the board: it is one of the two pages that talk to hardware rather
    /// than reading a value the platform pushed in.
    wifi: Wifi,
    /// The storage page, and the other one. It holds the same board — a page that asks the SDMMC slot
    /// whether there is a card in it is asking the board, and there is nowhere else to ask.
    storage: Storage,
    /// The readout at the top of the main list, and its own reading of the board. See
    /// [`pages::summary`] for why this one is taken here rather than pushed in the way the battery
    /// reading is: nothing else in this stack owns these numbers, and the page that draws them is
    /// the page that asks for them.
    system: SystemPanel,
    /// Whether the restart question is up, which is the only modal the *app* has.
    ///
    /// A `bool` and not a dialog type: there is one question, and an enum with one variant is a
    /// name for a thing that has not happened yet. The Wi-Fi page's prompt is an index into its own
    /// list of networks, which is why it is not a flag.
    restart_prompt: bool,
}

impl Settings {
    /// The app at the main list, the way the original starts, with `board` for the pages that need
    /// hardware.
    pub fn new(board: Arc<Board>) -> Self {
        // Read before the list is ever drawn, for the reason the Wi-Fi status is: the readout is the
        // first thing on the first page, and a board that never sent a tick would otherwise show an
        // empty panel until the app was left and re-entered.
        let system = SystemPanel::new(&board);

        let mut settings = Self {
            section: SettingsSection::Main,
            is_24h_format: true,
            battery: Battery::default(),
            preferences: SystemPreferences::default(),
            board: Arc::clone(&board),
            wifi: Wifi::new(Arc::clone(&board)),
            storage: Storage::new(board),
            system,
            restart_prompt: false,
        };

        // One read at boot, so the switch and the main list's Wi-Fi row are true before the page is
        // ever opened.
        settings.wifi.read();

        settings
    }

    /// The page that is open. Read by the host and by the tests.
    pub fn section(&self) -> SettingsSection {
        self.section
    }

    /// The Wi-Fi page: its state, and the reads the host and the tests need from it.
    ///
    /// One door rather than a dozen. The reads that used to sit here — the switch, the scan, the
    /// prompt, the password — were all of this page's fields, so they say so now.
    pub fn wifi(&self) -> &Wifi {
        &self.wifi
    }

    /// The storage page: the volumes the board last reported, and the card among them.
    ///
    /// A door of its own, for the same reason `wifi` is one: what the page read is the page's, and
    /// the host and the tests are the only other readers.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Whether the restart question is up.
    ///
    /// There is nothing inside the question to read — it asks about the whole board and holds no
    /// state of its own — so the question *is* the flag, and this is the flag.
    pub fn is_confirming_restart(&self) -> bool {
        self.restart_prompt
    }

    /// Whether the clock is 24-hour.
    pub fn is_24h_format(&self) -> bool {
        self.is_24h_format
    }

    /// The battery reading the battery page shows.
    pub fn battery(&self) -> Battery {
        self.battery
    }

    /// Switches to `language`.
    pub fn set_language(&mut self, language: Language) {
        self.preferences.language = language;
    }

    /// The aggregated system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the aggregated system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// Sets the visual theme mode.
    pub fn set_theme_mode(&mut self, theme: ThemeMode) {
        self.preferences.theme = theme;
    }

    /// Sets the font size tier.
    pub fn set_font_tier(&mut self, tier: FontSizeTier) {
        self.preferences.font_tier = tier;
    }

    /// Opens `section`.
    ///
    /// A change of section opens the new page at the top, which [`Settings::open`] arranges;
    /// setting the section it is already on changes nothing, so the offset survives — which is
    /// exactly what the original's `SettingsModel::set_section` did.
    pub fn set_section(&mut self, section: SettingsSection) {
        self.section = section;
    }

    /// Returns to the main list, reporting whether there was anywhere to return from and what work
    /// the press produced.
    ///
    /// `None` means the back press was *not* consumed: the launcher backgrounds the app instead,
    /// which is the contract the original's `go_back` had. `Some(task)` carries the work the press
    /// produced — sending the page it returns to back to the top.
    ///
    /// The password prompt is a page of its own while it is open, so back closes it before it
    /// leaves the Wi-Fi page — and it closes it *here* rather than in `update`, because the launcher
    /// intercepts `Message::Back` and calls this directly: a modal that only closed when the app was
    /// running standalone would be a modal that never closes on the board.
    pub fn go_back(&mut self) -> Option<Task<Message>> {
        // The two modals first, and neither of them is about which page is up: back closes what is
        // standing over the page before it does anything to the page itself. The restart question
        // is the app's, so it is asked first — there is no arrangement of the two that puts the
        // password sheet under it.
        if self.restart_prompt {
            self.restart_prompt = false;

            // A question sits over the page; the body under it did not move.
            return Some(Task::none());
        }

        if self.wifi.has_prompt() {
            self.wifi.cancel_prompt();

            // The prompt sits over the page; the body under it did not move.
            return Some(Task::none());
        }

        if self.section == SettingsSection::Main {
            None
        } else {
            self.section = SettingsSection::Main;

            // Coming back is a second look: the readout has been off screen for as long as the page
            // behind it was up, and it is the one thing here that went on changing while it was.
            self.refresh_system();

            Some(to_top())
        }
    }

    /// Resets the board, which is what the question was asking about.
    ///
    /// On the board this does not return: the SoC goes back to its bootloader, and the frame that
    /// would have drawn the question away is never drawn. What is left to handle is the case where
    /// it *did* return — a backend that cannot reset — and the app says so rather than going on as
    /// if the machine had restarted.
    fn restart(&mut self) {
        self.restart_prompt = false;

        if let Err(error) = self.board.power().restart() {
            eprintln!("[settings] the board could not be restarted: {error}");
        }
    }

    /// Takes a battery reading the platform pushed in.
    pub fn set_battery(&mut self, reading: Battery) {
        self.battery = reading;
    }

    /// Looks at the machine again, which is what the readout at the top of the list is made of.
    ///
    /// Called by whoever owns the loop when a second has passed — on the board that is
    /// [`pomelo_hal::SystemEvent::Tick`], which the platform's event pump sends once a second.
    ///
    /// Does nothing unless the main list is the page on screen, and that is the whole of its
    /// design. The readout is the only thing in this app that changes on its own, it is visible
    /// from nowhere else, and a frame is not free on this board: the host runs the view and the
    /// damage diff for every message, so a page that kept a clock ticking in the background would
    /// be paying for a number nobody is looking at.
    pub fn refresh_system(&mut self) {
        if self.section == SettingsSection::Main {
            self.system.refresh(&self.board);
        }
    }
}

// No `Default`: the app cannot be built without a board, and a `Default` that invented one would be
// a second composition root. `Settings::new(Arc<Board>)` is the only way in — which is what the
// firmware's root and `main.rs` both call, and what every test does with the simulator.

impl Settings {
    /// The theme: the palette and the background the platform paints behind the tree.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: style::black(),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: self.preferences.theme.background(),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// Reacts to one message, and returns whatever work it produced.
    ///
    /// Only a change of page produces any: the body is a single `scrollable`, and it has to be sent
    /// back to the top when the page under it changes. See the module docs.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open(section) => return self.open(section),
            Message::Back => return self.go_back().unwrap_or_else(Task::none),
            _ => {}
        }

        // Everything else changes what the page *says*, not which page it is.
        self.react(message);

        Task::none()
    }

    /// The messages that change the state of the page on screen, and nothing else.
    fn react(&mut self, message: Message) {
        match message {
            Message::ToggleWifi => self.wifi.toggle(),
            Message::Toggle24Hour => self.is_24h_format = !self.is_24h_format,
            Message::SetLanguage(language) => self.set_language(language),
            Message::SetTheme(theme) => self.set_theme_mode(theme),
            Message::CycleFontTier => self.set_font_tier(self.preferences.font_tier.cycle()),
            Message::StorageDetect => self.storage.detect(),
            // The row asks; the question's two answers are the only things that do or do not.
            Message::Restart => self.restart_prompt = true,
            Message::RestartCancel => self.restart_prompt = false,
            Message::RestartConfirm => self.restart(),
            Message::WifiScan => self.wifi.start_scan(),
            Message::WifiFrame(now) => self.wifi.poll(now),
            Message::WifiSelect(index) => self.wifi.select(index),
            Message::WifiKey(action) => self.wifi.key(action),
            Message::WifiConnect => self.wifi.connect(),
            Message::WifiCancel => self.wifi.cancel_prompt(),
            Message::WifiReveal => self.wifi.reveal(),
            Message::WifiDisconnect => self.wifi.disconnect(),

            // A change of page is `update`'s, above.
            Message::Open(_) | Message::Back => {}
        }
    }

    /// Whether the app is waiting on the radio, which is what [`Settings::subscription`] is for.
    ///
    /// A scan is in flight, or a finished scan's results have not been taken yet, or a connection
    /// is being waited for. Nothing else needs the loop: the page redraws when a finger touches it.
    ///
    /// This is the fix for what a real board showed: `[phase]` lines with `glyphs 0/0` and
    /// `clear 0.00`, one per millisecond, while nothing on the page moved. A frame subscription is
    /// not a timer — every frame it delivers is a message, iced runs the view and the damage diff
    /// for every message, and on this board that is about 10 ms a thousand times a second. So the
    /// page asks for frames only while it is waiting for something it cannot get any other way.
    pub fn is_waiting_on_wifi(&self) -> bool {
        self.wifi.is_waiting(self.section)
    }

    /// The app's subscriptions: the Wi-Fi page's frames, while that page is *waiting* on the radio.
    ///
    /// Nothing here is told the screen's size: the prompt's keyboard is flex, so it fills the card
    /// it is put in whatever the card is.
    ///
    /// `window::frames()` and not `iced::time::every`: that one is `cfg(any(tokio, smol, wasm32))`
    /// and this stack has no async runtime behind it — the board's loop polls its queue of futures
    /// once per frame, so a frame *is* this platform's clock.
    ///
    /// And only while [`Settings::is_waiting_on_wifi`] — a frame is not a free tick:
    /// the host runs the view and the damage diff for every message a subscription produces, which
    /// on the board is ~10 ms, and a page that asked for every frame while nothing was happening
    /// asked for that a thousand times a second.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.is_waiting_on_wifi() {
            iced::window::frames().map(Message::WifiFrame)
        } else {
            Subscription::none()
        }
    }

    // =========================================================================
    // Opening a page: the one thing left here that is the app's rather than a
    // page's, because it is about which page is up.
    // =========================================================================

    /// Opens `section`, and does what opening it means.
    ///
    /// Two pages have an opening move, and they are the two that read hardware. Wi-Fi scans. Storage
    /// re-probes the card slot — because "the card I just put in is not on the page" is exactly what
    /// opening the page is meant to fix, and nothing else in this app would ever look.
    ///
    /// Coming *back* to either does not repeat the move: the Wi-Fi results are still good and the
    /// scan row is there to ask again, and the storage page carries its own button for the same
    /// reason.
    ///
    /// A page that actually changed opens at the top; re-opening the page already showing leaves it
    /// where it was, which is what the original's `set_section` did.
    fn open(&mut self, section: SettingsSection) -> Task<Message> {
        if self.section == section {
            return Task::none();
        }

        let entering_wifi = section == SettingsSection::Wifi;
        let entering_storage = section == SettingsSection::Storage;

        self.set_section(section);

        if entering_wifi {
            self.wifi.start_scan();
        }

        if entering_storage {
            self.storage.detect();
        }

        // A page that *is* the main list has no opening move of its own, but the readout on it does:
        // leaving it for a sub-page and coming back is the one moment it is certainly stale, and it
        // costs five reads to be right about. See [`Settings::refresh_system`].
        self.refresh_system();

        to_top()
    }

    /// Describes the interface for the current state.
    pub fn view(&self) -> UI<'_> {
        use iced::widget::{container, stack};
        use iced::Length;
        use pages::{
            battery::battery_page, dialog::restart_dialog, main_page::main_page, memory::memory_page,
            storage::storage_page, system::system_page, theme::theme_page, time::time_page,
            wifi::wifi_page,
        };

        // No per-section wrapper any more: the body is one `scrollable` with one id, and `update`
        // sends it back to the top when the page changes. See the module docs.
        let page = match self.section {
            SettingsSection::Main => main_page(
                self.preferences,
                (self.wifi.status().state == WifiState::Connected)
                    .then_some(self.wifi.status().ssid.as_str()),
                &self.system,
            ),
            SettingsSection::Wifi => wifi_page(self.preferences, self.wifi.enabled(), &self.wifi),
            SettingsSection::Memory => memory_page(self.preferences),
            SettingsSection::Storage => storage_page(self.preferences, &self.storage),
            SettingsSection::Battery => battery_page(self.preferences, self.battery),
            SettingsSection::SystemInfo => system_page(self.preferences),
            SettingsSection::Theme => theme_page(self.preferences),
            SettingsSection::Time => time_page(self.preferences, self.is_24h_format),
        };

        // The surface the whole app sits on. Painted here rather than left to a theme, because a
        // theme is chosen by *whoever owns the loop*: run `settings` on its own and that is this
        // crate's, but on the board the launcher draws this app inside its own tree under its own
        // theme — whose background is the launcher's deep navy — and an app that says nothing about
        // its own background is wearing the launcher's. Every page of this app is designed against
        // black on an AMOLED panel, where an unlit pixel costs nothing. See `style::background_for`.
        let theme = self.preferences.theme;

        let surface = container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(style::background_for(theme).into()),
                ..container::Style::default()
            });

        // The restart question, when it is up, is a layer *over* the page rather than part of it —
        // the arrangement the Wi-Fi page gives its password sheet, and for the same reason: a modal
        // the page behind it can still be pressed through is not a modal. It is stacked here, by the
        // app, because it is not about a page: the row that opens it is on the list, and the question
        // is about the whole machine.
        match self.restart_prompt {
            true => stack![surface, restart_dialog(self.preferences)].into(),
            false => surface.into(),
        }
    }
}

// =============================================================================
// All page-builder functions have been moved to src/pages/.
// =============================================================================

