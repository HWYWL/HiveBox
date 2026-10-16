//! The box's own software: what version it is running, and what the repository has instead.
//!
//! One screen with two rows on it — the firmware this image is, and the glyph table the interface is
//! drawing from — and one button each. Pressing one asks the project's repository what it publishes,
//! reads a version out of the file names it finds ([`release`]), holds it up against the one on the box,
//! and says which of the two things a person does next: nothing, or download.
//!
//! # The check is one request, and the answer is read twice
//!
//! [`MANIFEST_URL`] is a single small document naming both files, so a check is one [`HttpBackend`]
//! request and not two — and it is the *small* request, which matters on a board whose HTTP client will
//! not hold an image in memory: the page decides whether it wants a file before it asks for a byte of
//! it. What it would download is a 5.8 MB image or a quarter-megabyte glyph table, and neither is ever
//! on this side of the network stack.
//!
//! The request is asynchronous for the reason the weather page's is: a request is a second of TLS
//! handshaking, and a page that waited for one would be a page that stopped drawing. [`Message::Tick`]
//! asks how it is going, and a frame is asked for only while there is an answer still to come.
//!
//! # Where the two versions come from
//!
//! The firmware's is the image's own: the build stamps the tag it was cut from into it, and
//! [`SystemBackend::firmware`] hands it back. Nothing on this page knows a version number in a literal,
//! because a version written down twice is a version that is wrong once.
//!
//! The glyph table's is a *file name* on the card — see [`FONT_DIR`] — because that is where a table
//! lives: the image carries a 24 px coverage cut down to the characters this source tree draws, and the
//! ones it leaves out — a file name, a network's name, a line from a server — need a table the flash has
//! no room for beside a second app slot. So the file that carries one names its version and the page reads
//! the newest it finds. A box with no card has no installed font version, which is a state this page
//! draws rather than an error.
//!
//! The firmware's boot reads the same file, through [`newest_font_table`]: the table this page names is
//! the table the interface is drawn with, because both are asking the one question of the one scan.
//!
//! # What this page does not do yet
//!
//! It does not download. Both rows can say whether there is something to fetch and neither can fetch it:
//! a download is either an OTA write into the second app slot or a couple of megabytes onto a card, and
//! both need a backend [`HttpBackend`] is not — see [`Status::Unsupported`], which is what pressing a
//! button draws until they exist.

mod json;
mod release;
pub mod style;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;
use std::time::Instant;

use iced::theme::Palette;
use iced::{Element, Subscription, Theme};

use pomelo_hal::{Board, HalError, HttpFault, HttpState};
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

pub use release::{Kind, Manifest, Release, Version, MANIFEST_URL};

/// Where the glyph tables live, on whichever mount the card was given.
///
/// The card and not the internal partition, and this is the page's one hard requirement: a table is
/// megabytes and the internal flash is holding two app images. A box with no card can still *read* the
/// table it was built or last given, and cannot be given a new one.
pub const FONT_DIR: &str = "/sdcard/fonts";

/// The suffix a table on the card has. Anything else in [`FONT_DIR`] is not this page's to read.
pub const FONT_SUFFIX: &str = ".bin";

/// What the page reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// A frame, which is the clock the request's progress is read on.
    Tick(Instant),
    /// The check button: ask the repository what it has.
    Check,
    /// The button on a row that has something to fetch.
    Install(Kind),
}

/// What the page has to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Nothing has been asked for yet. The rows show what is installed and nothing more.
    Idle,
    /// A request is on its way.
    Checking,
    /// The repository's answer, read: whether either row has anything to offer is [`Updater::offer`]'s
    /// question, and it is asked of the rows rather than kept here, because "is there an update" is a
    /// comparison and not a state the request can be in.
    Answered,
    /// The request did not come back, and this is why.
    Failed(HttpFault),
    /// Something answered, and it was not a manifest this page could read.
    Unreadable,
    /// A button was pressed that this image cannot honour yet: the download backends — the OTA write
    /// and the file write — are what this page is waiting on, and until they exist the honest drawing
    /// of a press is the line that says so rather than a progress bar that never moves.
    Unsupported(Kind),
}

/// What is installed here now.
///
/// Two answers to one question, and either can be missing: an image always has a version, and a box
/// with no card has no glyph table to have one. `None` is that second state and not an error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Installed {
    pub(crate) firmware: Option<Version>,
    pub(crate) font: Option<Version>,
}

/// The updater page.
pub struct Updater {
    board: Arc<Board>,
    pub(crate) status: Status,
    /// What the repository had, as of the last check. Empty until one has answered.
    pub(crate) manifest: Manifest,
    pub(crate) installed: Installed,
    pub(crate) preferences: SystemPreferences,
}

impl Updater {
    /// The page, asking the repository what it has.
    ///
    /// Asked here rather than left to the first frame, for the reason the weather page asks on the way
    /// in: the page's whole subject is *is there a newer one*, and a page that opened on "press to find
    /// out" would be a page that had not done the one thing it is for. What is installed is read without
    /// the network at all, so the two rows say something true even if the check never comes back.
    pub fn new(board: Arc<Board>) -> Self {
        let installed = read_installed(&board);
        let mut app = Self {
            board,
            status: Status::Idle,
            manifest: Manifest::default(),
            installed,
            preferences: SystemPreferences::default(),
        };

        app.check();

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

    /// What is installed of one kind, if anything at all could be read.
    pub(crate) fn installed_version(&self, kind: Kind) -> Option<&Version> {
        match kind {
            Kind::Firmware => self.installed.firmware.as_ref(),
            Kind::Font => self.installed.font.as_ref(),
        }
    }

    /// What the repository has that this box does not, if a check has found anything.
    ///
    /// The comparison and not the manifest: a release the box is already running is not an offer, and
    /// neither is one it cannot judge — a local version that could not be read is treated as *older*,
    /// because a box that cannot say what it is running is a box that should be offered the newest
    /// thing there is.
    pub(crate) fn offer(&self, kind: Kind) -> Option<&Release> {
        let release = self.manifest.of(kind)?;

        match self.installed_version(kind) {
            Some(installed) if !release.version.is_newer_than(installed) => None,
            _ => Some(release),
        }
    }

    /// Whether a request is still out, which is what the page's frames are for.
    pub(crate) fn waiting(&self) -> bool {
        matches!(self.status, Status::Checking)
    }

    /// The export path of the button that asks again: the same text whether or not there is one.
    pub(crate) fn can_check(&self) -> bool {
        !self.waiting()
    }

    /// Asks the repository what it has.
    ///
    /// A second request while one is in flight is not a thing a person can start — the button is off
    /// while the first is out — so a busy backend here is not an error to draw: it is a press that
    /// arrived twice, and the answer already on its way is the one this press wanted.
    fn check(&mut self) {
        let started = self.board.http().start(MANIFEST_URL);

        self.status = match started {
            Ok(()) => Status::Checking,
            Err(HalError::Busy) => Status::Checking,
            // An URL this board cannot fetch is a fault of this page's own constant rather than of the
            // network, and there is no fault in the list for "the page asked for something impossible".
            // The nearest true thing to say is that nothing answerable was reached.
            Err(_) => Status::Failed(HttpFault::Unreachable),
        };
    }

    /// Reads the request, if there is one to read.
    ///
    /// Called from [`Message::Tick`], which is the only clock a frame-driven request has on a board whose
    /// host serves no frames of its own. `Done` is taken exactly once, because the state goes on saying
    /// it until the answer is collected and a page that read it every frame would be a page reading the
    /// same body sixty times a second.
    fn poll(&mut self) {
        let state = self.board.http().state();

        match state {
            HttpState::Done => {
                let answer = self.board.http().take_result();

                self.status = match answer {
                    Ok(response) if response.status == 200 => match response.text() {
                        Some(document) => {
                            self.manifest = Manifest::parse(document);

                            // Re-read with the answer, because what is installed can have changed
                            // under a card that was pulled or put in while the request was out.
                            let installed = read_installed(&self.board);
                            self.installed = installed;

                            if self.manifest.is_empty() {
                                Status::Unreadable
                            } else {
                                Status::Answered
                            }
                        }
                        // A body that is not text is not a manifest, and there is no third thing it
                        // could be that this page would draw differently.
                        None => Status::Unreadable,
                    },
                    // Anything that is not `200` is the same thing to a person — the repository did not
                    // answer with a manifest — and the page says so in one line rather than in a status
                    // code nobody can act on.
                    Ok(_) => Status::Unreadable,
                    Err(_) => Status::Unreadable,
                };
            }
            HttpState::Failed(fault) => {
                self.status = Status::Failed(fault);
            }
            HttpState::Idle | HttpState::Running => {}
        }
    }

    /// The message the page sends when the check button is pressed.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(_) if self.waiting() => self.poll(),
            // A frame is only ever asked for while a request is out — see [`Updater::subscription`] —
            // but a launcher pumps this page's beat whether or not it is waiting, so the rest of the
            // frames arrive here and are dropped rather than read as progress.
            Message::Tick(_) => {}
            Message::Check => {
                if self.can_check() {
                    self.check();
                }
            }
            Message::Install(kind) => self.status = Status::Unsupported(kind),
        }
    }

    /// The page.
    pub fn view(&self) -> Element<'_, Message> {
        ui::page(self)
    }

    /// The frames, while there is an answer to wait for.
    ///
    /// The thing this page cannot do for itself: a window's clock is its frames, and the board's is the
    /// launcher's pump. Nothing to wait for is no subscription at all, which is what keeps a page sitting
    /// on a shelf from asking for a frame a second to redraw a version number.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.waiting() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The theme, by the system's preference.
    ///
    /// `window::frames` and not a timer, for the reason the forecast page gives: a window's clock is its
    /// frames, and asking for them is asking for exactly as many as the compositor was going to draw.
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
}

/// What the box is running and drawing from, read without the network.
///
/// Both halves are best-effort and either may come back `None`, which is the state the page draws as
/// "unknown" rather than as a fault: the card may not be in the slot, and that is not a fault.
pub(crate) fn read_installed(board: &Board) -> Installed {
    Installed {
        firmware: installed_firmware(board),
        font: installed_font(FONT_DIR),
    }
}

/// The version stamped into the image this code is running from.
fn installed_firmware(board: &Board) -> Option<Version> {
    let info = board.system().firmware().ok()?;

    Version::parse(&info.version)
}

/// The newest table in `dir`, read off its file name.
fn installed_font(dir: &str) -> Option<Version> {
    newest_font_table_in(dir).map(|(version, _)| version)
}

/// The newest table in [`FONT_DIR`] — the one the interface should be drawing from.
///
/// The firmware's boot asks this before the first frame and hands the file to
/// `iced_pomelo_winit::fonts::prefer_tables`, which is what turns "the newest table is v3" from a claim
/// into the thing actually being drawn: the page can say which table the box *has* without being able
/// to choose it, and this is the other half of that sentence.
///
/// It is the same scan as the page's, deliberately and not by accident: the file this returns is the
/// file [`read_installed`] reads the version out of, so the two can never disagree about which table is
/// the newest one.
pub fn newest_font_table() -> Option<(Version, String)> {
    newest_font_table_in(FONT_DIR)
}

/// The newest table in `dir`, with the path it is at.
///
/// The path as well as the version, because the two callers want different halves of the same answer:
/// the page wants to *say* which version is installed, and a boot wants to *read* the file. One scan
/// with both in hand is what keeps those two from being different files.
///
/// The *newest* rather than the one this page would write, because the card is a place files are put: a
/// person may have several tables there, and the interface reads the newest one it can find — see this
/// module's header on why the version is in the name. A directory that does not exist, a name that is
/// not UTF-8 and a file that carries no version are each skipped rather than fatal, which is what makes
/// this readable on a board with no card at all.
pub fn newest_font_table_in(dir: &str) -> Option<(Version, String)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut newest: Option<(Version, String)> = None;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };

        if !name.ends_with(FONT_SUFFIX) {
            continue;
        }

        let Some(version) = Version::from_file_name(name) else {
            continue;
        };

        let newer = match &newest {
            Some((held, _)) => version.is_newer_than(held),
            None => true,
        };

        if newer {
            newest = Some((version, entry.path().to_string_lossy().into_owned()));
        }
    }

    newest
}

/// The page's words, in the two languages the interface has.
///
/// Here rather than in the drawing code because they are the page's *content*: a check that found
/// nothing and a check that could not be made are two different sentences, and which one to draw is a
/// decision this struct makes where the rest of the project can read it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Words {
    /// Which language these are, because the two fault tables below are indexed by it and a page that
    /// compared its own title against a literal to find out would be a page guessing at its own state.
    pub(crate) chinese: bool,
    pub(crate) title: &'static str,
    pub(crate) subtitle: &'static str,
    pub(crate) firmware: &'static str,
    pub(crate) font: &'static str,
    pub(crate) check: &'static str,
    pub(crate) checking: &'static str,
    pub(crate) idle: &'static str,
    pub(crate) installed: &'static str,
    pub(crate) unknown: &'static str,
    pub(crate) absent: &'static str,
    pub(crate) current: &'static str,
    pub(crate) available: &'static str,
    pub(crate) install: &'static str,
    pub(crate) unreadable: &'static str,
    pub(crate) unsupported: &'static str,
    pub(crate) no_card: &'static str,
}

impl Words {
    /// The words for `language`.
    pub(crate) fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                chinese: true,
                title: "更新",
                subtitle: "固件与字库",
                firmware: "固件",
                font: "字库",
                check: "检查更新",
                checking: "正在检查…",
                idle: "点上面的按钮检查是否有新版本。",
                installed: "当前",
                unknown: "未知",
                absent: "未安装",
                current: "已是最新",
                available: "可更新到",
                install: "下载并安装",
                unreadable: "仓库没有可读的版本清单，稍后再试。",
                unsupported: "这一步还没接通：新固件要写进第二个分区，新字库要写进 SD 卡。",
                no_card: "字库更新需要 SD 卡：内置 flash 装不下。",
            },
            Language::English => Self {
                chinese: false,
                title: "Update",
                subtitle: "Firmware and glyph table",
                firmware: "Firmware",
                font: "Glyph table",
                check: "Check for updates",
                checking: "Checking…",
                idle: "Press the button above to see whether there is a newer one.",
                installed: "Installed",
                unknown: "unknown",
                absent: "not installed",
                current: "Up to date",
                available: "Update to",
                install: "Download and install",
                unreadable: "The repository has no manifest this page can read. Try again later.",
                unsupported: "This step is not wired up yet: a new firmware needs a second app slot, and a new glyph table needs the card.",
                no_card: "A glyph table needs the SD card: the internal flash cannot hold one beside the images.",
            },
        }
    }

    /// The name of one of the two rows.
    pub(crate) fn name(&self, kind: Kind) -> &'static str {
        match kind {
            Kind::Firmware => self.firmware,
            Kind::Font => self.font,
        }
    }

    /// The sentence for a name in a row: what is installed here.
    pub(crate) fn installed_line(&self, version: Option<&Version>, kind: Kind) -> String {
        match version {
            Some(version) => format!("{} {}", self.installed, version.text()),
            // The firmware always has a version and a glyph table may not, and the two are drawn
            // differently: a font that is not installed is a state, and an image that cannot say what
            // it is running is a fault of the read.
            None if kind == Kind::Font => self.absent.to_string(),
            None => self.unknown.to_string(),
        }
    }

    /// The sentence for what the repository has, once a check has answered.
    pub(crate) fn offer_line(&self, release: &Release) -> String {
        format!("{} {}", self.available, release.version.text())
    }

    /// The one line for a request that did not come back.
    pub(crate) fn fault_line(&self, fault: HttpFault) -> &'static str {
        match (fault, self.chinese) {
            (HttpFault::NoNetwork, false) => "This box is not on a network.",
            (HttpFault::NoNetwork, true) => "这盒子现在不在网络上。",
            (HttpFault::ClockNotSet, false) => {
                "The clock has never been set, so HTTPS cannot be checked."
            }
            (HttpFault::ClockNotSet, true) => "系统时间没有设置，HTTPS 证书无法校验。",
            (HttpFault::Unreachable, false) => "Nothing answered.",
            (HttpFault::Unreachable, true) => "没有连上服务器。",
            (HttpFault::Timeout, false) => "The request timed out.",
            (HttpFault::Timeout, true) => "请求超时。",
            (HttpFault::Tls, false) => "The secure connection did not complete.",
            (HttpFault::Tls, true) => "安全连接没有建立起来。",
            (HttpFault::TooLarge, false) => "The answer was larger than this box will hold.",
            (HttpFault::TooLarge, true) => "服务器返回的内容超出了本机上限。",
            (HttpFault::Storage, false) => "The download could not be written. Is there a card, and room on it?",
            (HttpFault::Storage, true) => "下载的内容没能写进去，看看卡在不在、还有没有空间。",
            (HttpFault::Unsupported, false) => "This image has no HTTP client in it.",
            (HttpFault::Unsupported, true) => "这个镜像里没有 HTTP 客户端。",
        }
    }
}
