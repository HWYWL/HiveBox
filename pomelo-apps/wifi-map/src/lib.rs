//! The 2.4 GHz channel map: what is on the air, and which channel is worth using.
//!
//! One screen, and it is a drawing of one scan. The radio is asked once when the page opens and once
//! when somebody presses the button; everything after that is reading [`WifiMap::spectrum`].
//!
//! # The scan is asynchronous, and this page is where that shows
//!
//! [`pomelo_hal::WifiBackend`] has no blocking call: a scan is `scan_start`, then `scan_state` until it
//! is `Done`, then `scan_results`. That is not a detail of the radio — a scan is a couple of seconds of
//! the driver's time, and a page that waited for it would be a page that stopped drawing. So a scan is
//! polled: [`Message::Tick`] asks how it is going, and a frame is asked for *only* while there is an
//! answer still to come. See [`WifiMap::subscription`].
//!
//! # The numbers are a model, and the page says so
//!
//! [`channel`] is where that argument is, and it is the reason this app exists rather than a count of
//! networks per channel: what makes a channel crowded is a 20 MHz signal heard across five channels,
//! and an RSSI, not a tally. The page claims an *ordering* from those numbers — which of 1, 6 and 11 is
//! the least crowded — and never a measurement.
//!
//! # What is out of band is listed, not charted
//!
//! 5 GHz networks are found and shown. They are kept out of the chart's arithmetic because they take up
//! none of the band the chart is about; adding them to the pile would make the page quieter about the
//! noise that is really there. See [`channel`].

mod channel;
mod plot;
pub mod style;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;
use std::time::Instant;

use iced::theme::Palette;
use iced::{Element, Subscription, Theme};

use pomelo_hal::{ApInfo, Board, ScanState};
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

pub use channel::{
    channels, in_band, level_of, overlap, weight, Bin, Spectrum, CLEAN, FIRST, LAST, LEVELS,
};
pub use style::Sizes;

/// What the page reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// A frame, which is the clock the scan's progress is read on.
    Tick(Instant),
    /// The rescan button: look at the air again.
    Rescan,
}

/// The channel map.
///
/// The state is one scan: where the radio is in it ([`ScanState`]), what it found (`aps`), and the same
/// read as a drawing (`spectrum`). The two are kept together rather than derived in `view` because
/// building the spectrum is the only arithmetic on the page, and a `view` that did it would do it once a
/// frame.
pub struct WifiMap {
    board: Arc<Board>,
    /// The radio's own answer to "how is the scan going".
    state: ScanState,
    /// The last completed scan's networks, as the backend reported them.
    pub(crate) aps: Vec<ApInfo>,
    /// The same scan arranged as the chart and the map draw it.
    pub(crate) spectrum: Spectrum,
    /// Whether the results of the scan that just finished have been read.
    ///
    /// The flag that makes "Done" a one-shot: the backend goes on answering `Done` until the next scan
    /// starts, and reading its results every frame would rebuild the spectrum sixty times a second for
    /// an answer that only changes when somebody presses the button.
    collected: bool,
    pub(crate) preferences: SystemPreferences,
}

impl WifiMap {
    /// The page, looking at the air.
    ///
    /// A scan is started here rather than left to the first frame: the page's whole subject is the air,
    /// and a page that opened on an empty chart until something asked would be a page that looked broken
    /// for a second. The ask is asynchronous, so this returns immediately — the frame that draws the
    /// answer arrives when the radio has one.
    pub fn new(board: Arc<Board>) -> Self {
        let mut app = Self {
            board,
            state: ScanState::Idle,
            aps: Vec::new(),
            spectrum: Spectrum::of(&[]),
            collected: true,
            preferences: SystemPreferences::default(),
        };

        app.rescan();

        app
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The air, as the last completed scan found it.
    pub fn spectrum(&self) -> &Spectrum {
        &self.spectrum
    }

    /// The networks of the last completed scan, charted or not.
    pub fn aps(&self) -> &[ApInfo] {
        &self.aps
    }

    /// Where the radio is in the scan the page is waiting on.
    pub fn state(&self) -> ScanState {
        self.state
    }

    /// Whether the page is still waiting for something — a scan in progress, or results that have
    /// arrived and not been read yet.
    ///
    /// The one question [`WifiMap::subscription`] is asked, and it is the whole of the page's frame
    /// budget: a chart of a scan that finished is not a moving picture, so nothing asks for frames once
    /// it has been read.
    pub fn waiting(&self) -> bool {
        match self.state {
            ScanState::Scanning => true,
            ScanState::Done => !self.collected,
            _ => false,
        }
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(_) => self.pull(),
            Message::Rescan => self.rescan(),
        }
    }

    /// Asks the radio for a scan.
    ///
    /// A refused start is the state, not an error to draw a dialog for: a radio switched off is a thing
    /// this page can say in one line, and the button is right there for when it is switched back on.
    pub fn rescan(&mut self) {
        match self.board.wifi().scan_start() {
            Ok(()) => {
                self.state = ScanState::Scanning;
                self.collected = false;
            }
            Err(_) => {
                self.state = ScanState::Error;
                self.collected = true;
            }
        }
    }

    /// Reads what has arrived, and nothing else.
    ///
    /// Reading the results is a lock and a clone, so it happens on the frame the radio says `Done` and
    /// not on any frame before or after — see [`WifiMap::collected`].
    fn pull(&mut self) {
        let state = self.board.wifi().scan_state();
        self.state = state;

        if state == ScanState::Done && !self.collected {
            match self.board.wifi().scan_results() {
                Ok(aps) => {
                    self.spectrum = Spectrum::of(&aps);
                    self.aps = aps;
                }
                Err(_) => {
                    // The radio said `Done` and then had nothing: a race with a scan that started
                    // underneath this one more than a fault, and the honest drawing of it is the empty
                    // chart the page already has the code for.
                    self.spectrum = Spectrum::of(&[]);
                    self.aps.clear();
                }
            }

            self.collected = true;
        }
    }

    /// Frames, and only while this page is waiting for one. See [`WifiMap::waiting`].
    ///
    /// On the board this is never asked for: the launcher merges the subscriptions of the apps it is
    /// actually running, and its own pump is what sends [`Message::Tick`] here. A window has no such
    /// pump, so the app takes the frames it is offered — while there is a reason to.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.waiting() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The theme, by the system's preference.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: style::surface(ThemeMode::Dark),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::surface(ThemeMode::Light),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// The page.
    pub fn view(&self) -> Element<'_, Message> {
        ui::page(self)
    }
}

/// The words on the page, in the language the system is set to.
///
/// One struct with both languages in it — the shape the rest of this project's pages use — so that a
/// page never has a sentence with a language of its own, and adding a word is adding a field the
/// compiler will insist be filled in twice.
pub(crate) struct Words {
    pub(crate) language: Language,
    pub(crate) title: &'static str,
    pub(crate) rescan: &'static str,
    pub(crate) scanning: &'static str,
    pub(crate) failed: &'static str,
    pub(crate) empty: &'static str,
    pub(crate) chart: &'static str,
    pub(crate) chart_hint: &'static str,
    pub(crate) map: &'static str,
    pub(crate) map_hint: &'static str,
    pub(crate) list: &'static str,
    pub(crate) others: &'static str,
    pub(crate) advice: &'static str,
    pub(crate) advice_note: &'static str,
    pub(crate) hidden: &'static str,
    pub(crate) channel_axis: &'static str,
}

impl Words {
    /// The page's words, for `language`.
    pub(crate) fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language,
                title: "信道地图",
                rescan: "重新扫描",
                scanning: "正在扫描…",
                failed: "扫描失败",
                empty: "这一带还没有网络",
                chart: "信道压力",
                chart_hint: "20 MHz 重叠模型",
                map: "信号分布",
                map_hint: "每行 10 dBm",
                list: "网络",
                others: "带外网络",
                advice: "建议信道",
                advice_note: "1、6、11 互不重叠",
                hidden: "隐藏网络",
                channel_axis: "信道",
            },
            Language::English => Self {
                language,
                title: "Wi-Fi Map",
                rescan: "Rescan",
                scanning: "Scanning…",
                failed: "The scan failed",
                empty: "Nothing on the air",
                chart: "Channel pressure",
                chart_hint: "20 MHz overlap model",
                map: "Signal map",
                map_hint: "10 dBm a row",
                list: "Networks",
                others: "Out of band",
                advice: "Use channel",
                advice_note: "1, 6 and 11 never overlap",
                hidden: "Hidden network",
                channel_axis: "Channel",
            },
        }
    }

    /// How many networks the scan found, as the header's status line says it.
    pub(crate) fn found(&self, networks: usize) -> String {
        match self.language {
            Language::Chinese => format!("找到 {networks} 个网络"),
            Language::English if networks == 1 => String::from("1 network"),
            Language::English => format!("{networks} networks"),
        }
    }
}
