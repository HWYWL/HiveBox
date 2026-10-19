//! The application store: what the repository publishes, what is installed, and the press between them.
//!
//! # Three lists, and the same row in each
//!
//! [`Section::Installed`] is the catalog read off the two directories; [`Section::Available`] is the
//! manifest's `apps` minus what is installed; [`Section::Updates`] is the intersection where the
//! repository is newer. They are the same row drawn three ways because a person is doing the same
//! thing in all three — reading a name, a version and a size, and deciding whether to press the one
//! button — and only the verb changes: 卸载, 安装, 更新.
//!
//! # A press is a download, and a download is a file
//!
//! Nothing here holds a package in memory. [`HttpBackend::start_to`] writes the bytes into a `.part`
//! file inside the volume's `.staging` directory as they arrive, and [`app_host::manager::install_file`]
//! verifies that file and renames it into place. The progress bar is [`HttpBackend::progress`] read
//! once a frame, which is also why the page subscribes to frames only while something is out.
//!
//! # What happens when it fails
//!
//! Six ways, and each one leaves nothing behind: a download interrupted (the `.part` is removed and
//! the request cancelled), a size that disagrees with the manifest, a SHA-256 that does not match, a
//! package whose container does not hold together, a host with no writable volume, and a module that
//! traps when it is started (the runtime's business, and the launcher's). The first five are
//! [`crate::Prompt`]s over the page; the sixth is the launcher's crash counter.
//!
//! # Offline
//!
//! The manifest is cached to [`CACHE_FILE`] the moment it is read, and the cache is what the page
//! opens on. A box with no network therefore still shows what the repository had last time, and the
//! installed list — which never needed the network — is always true.
//!
//! [`HttpBackend::start_to`]: pomelo_hal::traits::HttpBackend::start_to
//! [`HttpBackend::progress`]: pomelo_hal::traits::HttpBackend::progress

mod ui;
pub mod style;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use iced::theme::Palette;
use iced::{Subscription, Theme};

use pomelo_hal::{Board, HttpFault, HttpState, Sink};
use pomelo_widgets::{Language, SystemPreferences, ThemeMode};

use app_host::manager::{self, Expected};
use app_host::{Broken, Catalog, Installed, Root, Source, Version};
use updater::{AppRelease, Manifest, MANIFEST_URL};

/// The store's title, as the launcher's tile and the page's header write it.
pub const TITLE: &str = "应用商店";

/// The store's Chinese subtitle, which is what a person reads under the title.
pub const SUBTITLE: &str = "安装、更新与卸载应用";

/// Where the last manifest read is kept, so that an offline box still knows what exists.
pub const CACHE_FILE: &str = "/internal/AppData/store/catalog.json";

/// Which of the three lists is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Installed,
    Available,
    Updates,
}

impl Section {
    /// The three, in the order the page shows them.
    pub const ALL: [Section; 3] = [Section::Installed, Section::Available, Section::Updates];

    /// The tab's word, which is the row's verb in the plural.
    pub fn label(self, words: &Words) -> &'static str {
        match self {
            Self::Installed => words.installed,
            Self::Available => words.available,
            Self::Updates => words.updates,
        }
    }
}

/// What the page reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A frame, which is the clock a download's progress is read on.
    Tick(Instant),
    /// One of the three tabs.
    Section(Section),
    /// Ask the repository what it has.
    Check,
    /// Install or upgrade the application with this id.
    Install(String),
    /// Ask before removing the application with this id.
    Uninstall(String),
    /// The prompt's second button: do the thing it asked about.
    Confirm,
    /// The prompt's first button: put it away — and cancel the download, when that is what it covers.
    Dismiss,
}

/// What the last *check* is doing. A press's outcome is [`Prompt`], not this.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Nothing has been asked for yet.
    Idle,
    /// The manifest's request is out.
    Checking,
    /// The manifest has been read; the rows say the rest.
    Answered,
    /// Something answered, and it was not a manifest this page could read.
    Unreadable,
    /// The request did not come back, and this is why.
    Failed(HttpFault),
    /// A package is downloading; the id is whose.
    Downloading(String),
}

/// What a prompt is saying, which is what decides its glyph and its ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A download in flight.
    Working,
    /// A press that did what it said.
    Done,
    /// A press that did not.
    Failed,
}

/// What a prompt's second button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing: the prompt is a report.
    None,
    /// Remove this application, and its data with it.
    Uninstall(String),
}

/// A sentence about the last press, over the page.
///
/// Both a *confirmation* and a *report*: the two are the same widget because they are the same
/// moment — the page stops being a list and asks or answers one thing — and what differs is whether
/// there is a second button.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub title: String,
    pub line: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub bytes: String,
    /// What the first button says: cancel while a download is out, close for everything else.
    pub button: &'static str,
    /// What the second button says, when there is one: the action a confirmation is asking about.
    pub confirm: Option<&'static str>,
    pub tone: Tone,
    pub(crate) action: Action,
}

impl Prompt {
    fn report(title: impl Into<String>, line: impl Into<String>, tone: Tone, words: &Words) -> Self {
        Self {
            title: title.into(),
            line: line.into(),
            detail: String::new(),
            progress: None,
            bytes: String::new(),
            button: words.close,
            confirm: None,
            tone,
            action: Action::None,
        }
    }
}

/// A package that is on its way in, and how far along it is.
pub(crate) struct Download {
    /// The name the manifest gave, for the prompt: the package's own metadata is not readable until
    /// it has landed.
    pub(crate) name: String,
    pub(crate) file: String,
    /// The `.part` file the bytes are landing in, inside the target volume's `.staging`.
    pub(crate) temp: PathBuf,
    /// The volume the package will be installed to, decided before the first byte so that the
    /// temporary file is on the same filesystem as the install and the install is a rename.
    pub(crate) root: Root,
    pub(crate) expected: Expected,
    pub(crate) received: u64,
    pub(crate) total: u64,
}

impl Download {
    /// How much is here, `0.0` to `1.0`, or `None` when nothing has said how big the file is.
    pub(crate) fn fraction(&self) -> Option<f32> {
        if self.total == 0 {
            return None;
        }

        Some((self.received as f32 / self.total as f32).clamp(0.0, 1.0))
    }

    /// `84 KB / 228 KB`, or the bytes alone when nothing has said how many there will be.
    pub(crate) fn bytes_line(&self) -> String {
        match self.total {
            0 => size_text(self.received),
            total => format!("{} / {}", size_text(self.received), size_text(total)),
        }
    }
}

/// The store page.
pub struct Store {
    board: Arc<Board>,
    pub(crate) roots: Vec<Root>,
    pub(crate) catalog: Catalog,
    pub(crate) manifest: Manifest,
    pub(crate) status: Status,
    pub(crate) section: Section,
    pub(crate) download: Option<Download>,
    /// A press's outcome, when there is one to report. The download's own prompt is built from
    /// [`Store::download`] each frame and is not stored here — see [`Store::prompt`].
    pub(crate) prompt: Option<Prompt>,
    pub(crate) preferences: SystemPreferences,
    /// The manifest cache: a convenience, and the only thing on this page that writes a file other
    /// than a package.
    pub(crate) cache: PathBuf,
    /// Whether the installed list has changed since the launcher last asked: an install landed, or a
    /// removal did. The launcher reads it to know when to rescan the grid — see
    /// [`Store::take_changed`].
    changed: bool,
}

impl Store {
    /// The page over the two volumes this project installs to.
    ///
    /// Asks the repository on the way in for the updater's reason: the page's whole subject is what
    /// there is to install, and a page that opened on "press to find out" had not asked the question
    /// it exists for. The installed list is read without the network, so it is true even if the
    /// request never comes back.
    pub fn new(board: Arc<Board>) -> Self {
        Self::with_roots(board, Root::standard(), PathBuf::from(CACHE_FILE))
    }

    /// The page over the roots and cache a composition root chose.
    ///
    /// The launcher uses [`Store::new`]; a test uses this, because `/internal` and `/sdcard` are
    /// paths *the board* has and a test that wrote to them would be a test that wrote to the machine
    /// that ran it.
    pub fn with_roots(board: Arc<Board>, roots: Vec<Root>, cache: PathBuf) -> Self {
        let manifest = read_cache(&cache);
        let mut app = Self {
            board,
            catalog: Catalog::scan(roots.clone()),
            roots,
            manifest,
            status: Status::Idle,
            section: Section::Installed,
            download: None,
            prompt: None,
            preferences: SystemPreferences::default(),
            cache,
            changed: false,
        };

        app.check();

        app
    }

    /// Whether the installed list changed since this was last asked, and forgets the answer.
    ///
    /// The grid is built from the catalog at boot and after every change; this is what tells the
    /// launcher that "after" has arrived. One flag rather than a diff, because the launcher's answer
    /// to either an install or a removal is the same: rescan.
    pub fn take_changed(&mut self) -> bool {
        std::mem::replace(&mut self.changed, false)
    }

    /// Which of the three lists is on screen.
    pub fn section(&self) -> Section {
        self.section
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// Whether something is still out, which is what the page's frames are for.
    pub fn waiting(&self) -> bool {
        matches!(self.status, Status::Checking | Status::Downloading(_))
    }

    /// Whether the check button may be pressed.
    pub fn can_check(&self) -> bool {
        !self.waiting()
    }

    /// The application on screen, if it is installed.
    pub fn installed(&self, id: &str) -> Option<&Installed> {
        self.catalog.get(id)
    }

    /// The directories that looked like applications and were not.
    pub fn broken(&self) -> &[Broken] {
        self.catalog.broken()
    }

    /// What the repository has for an id, when it is newer than what is installed.
    pub fn upgrade_for(&self, app: &Installed) -> Option<&AppRelease> {
        let release = self.manifest.app(&app.meta.id)?;

        let offered = Version::parse(release.version.text())?;

        match Version::parse(&app.meta.version) {
            Some(installed) => offered.is_newer_than(&installed).then_some(release),
            // A box that cannot say what it is running is a box that should be offered the newest
            // thing there is — the updater's rule, and it holds here for the same reason.
            None => Some(release),
        }
    }

    /// The repository's packages that are not installed, in manifest order.
    pub fn available(&self) -> Vec<&AppRelease> {
        self.manifest
            .apps
            .iter()
            .filter(|release| !self.catalog.is_installed(&release.id))
            .collect()
    }

    /// What is installed and has something newer in the repository.
    pub fn updates(&self) -> Vec<(&Installed, &AppRelease)> {
        self.catalog
            .apps()
            .iter()
            .filter_map(|app| self.upgrade_for(app).map(|release| (app, release)))
            .collect()
    }

    /// The name of a repository package in the active language.
    pub fn release_name<'a>(&self, release: &'a AppRelease) -> &'a str {
        self.localized(&release.name_zh, &release.name)
    }

    /// The name of an installed application in the active language.
    pub fn installed_name<'a>(&self, app: &'a Installed) -> &'a str {
        self.localized(&app.meta.name_zh, &app.meta.name)
    }

    fn localized<'a>(&self, zh: &'a str, en: &'a str) -> &'a str {
        match self.preferences.language {
            Language::Chinese if !zh.is_empty() => zh,
            _ if !en.is_empty() => en,
            _ => zh,
        }
    }

    /// The prompt over the page, when there is one.
    ///
    /// A download's prompt is *built* here rather than stored: its fraction and byte count change
    /// every frame, and a stored copy would be a second place the truth of them lives. Everything
    /// else is a press's outcome and was written once, when the press was answered.
    pub fn prompt(&self, words: &Words) -> Option<Prompt> {
        if matches!(self.status, Status::Downloading(_)) {
            let download = self.download.as_ref()?;

            return Some(Prompt {
                title: download.name.clone(),
                line: words.downloading.to_string(),
                detail: download.file.clone(),
                progress: download.fraction(),
                bytes: download.bytes_line(),
                button: words.cancel,
                confirm: None,
                tone: Tone::Working,
                action: Action::None,
            });
        }

        self.prompt.clone()
    }

    /// Asks the repository what it has.
    fn check(&mut self) {
        if self.waiting() {
            return;
        }

        self.status = match self.board.http().start(MANIFEST_URL) {
            Ok(()) => Status::Checking,
            // A request already in flight is one whose answer this page wanted anyway: there is one
            // manifest and one slot, so the press is not lost, it is answered by the request that is
            // already out.
            Err(pomelo_hal::HalError::Busy) => Status::Checking,
            Err(_) => Status::Failed(HttpFault::Unreachable),
        };
    }

    /// Reads whichever request is out, if there is one to read.
    fn poll(&mut self) {
        if matches!(self.status, Status::Downloading(_)) {
            self.poll_download();
        } else {
            self.poll_check();
        }
    }

    fn poll_check(&mut self) {
        // The state is read into a binding first, and not matched on the call: the guard a lock
        // hands back lives until the end of the statement, and a `match` on one would be holding
        // the backend while the arm below asks it for the answer — a deadlock, not a borrow error,
        // because the lock is the board's.
        let state = self.board.http().state();

        match state {
            HttpState::Done => {
                let answer = self.board.http().take_result();

                // Re-read with the answer: a card pulled or put in while the request was out is a
                // different set of installed applications.
                self.catalog = Catalog::scan(self.roots.clone());

                self.status = match answer {
                    Ok(response) if response.status == 200 => match response.text() {
                        Some(document) => {
                            let manifest = Manifest::parse(document);

                            if manifest.is_empty() {
                                // The cache is kept: an answer that is not a manifest says nothing
                                // about what the last real one did, and the rows would otherwise
                                // empty themselves over a captive portal.
                                Status::Unreadable
                            } else {
                                self.write_cache(document);
                                self.manifest = manifest;

                                Status::Answered
                            }
                        }
                        None => Status::Unreadable,
                    },
                    Ok(_) => Status::Unreadable,
                    Err(_) => Status::Unreadable,
                };
            }
            HttpState::Failed(fault) => self.status = Status::Failed(fault),
            HttpState::Idle | HttpState::Running => {}
        }
    }

    /// Starts the download for an application: the manifest's size and hash, a `.part` file on the
    /// volume the package will live on, and one request.
    fn install(&mut self, id: &str) {
        if self.waiting() {
            return;
        }

        let Some(release) = self.manifest.app(id).cloned() else {
            return;
        };

        let Some(version) = Version::parse(release.version.text()) else {
            return;
        };

        let words = Words::of(self.preferences.language);
        let name = self.release_name(&release).to_string();

        // The volume is chosen *before* the first byte: the temporary file has to be on the same
        // filesystem as the install, or the rename that makes an install atomic becomes a copy.
        let Some(root) = manager::root_for(&self.roots, None, release.size) else {
            self.prompt = Some(Prompt::report(
                name,
                words.no_volume,
                Tone::Failed,
                &words,
            ));

            return;
        };

        let staging = root.path.join(manager::STAGING_DIR);

        if let Err(error) = std::fs::create_dir_all(&staging) {
            self.prompt = Some(Prompt::report(
                name,
                format!("{}: {error}", words.storage_failed),
                Tone::Failed,
                &words,
            ));

            return;
        }

        let temp = staging.join(format!("{}-{}.part", release.id, version.text()));
        std::fs::remove_file(&temp).ok();

        let expected = Expected {
            size: (release.size > 0).then_some(release.size),
            sha256: (!release.sha256.is_empty()).then(|| release.sha256.clone()),
        };

        let sink = Sink::File(temp.to_string_lossy().into_owned());

        match self.board.http().start_to(&release.url(), sink) {
            Ok(()) => {
                self.status = Status::Downloading(release.id.clone());
                self.prompt = None;
                self.download = Some(Download {
                    name,
                    file: release.file.clone(),
                    temp,
                    root,
                    expected,
                    received: 0,
                    total: release.size,
                });
            }
            Err(_) => {
                discard(&temp);

                self.prompt = Some(Prompt::report(name, words.busy, Tone::Failed, &words));
            }
        }
    }

    fn poll_download(&mut self) {
        let Some(mut download) = self.download.take() else {
            self.status = Status::Answered;

            return;
        };

        let state = self.board.http().state();

        match state {
            HttpState::Done => self.finish_download(download),
            HttpState::Failed(fault) => {
                // The client removes its own half-written file; this is the second end doing the
                // same, because a `.part` left behind is a package nobody will ever finish.
                discard(&download.temp);

                // The request is over, and its leftovers are not an answer to be collected: the
                // state goes back to `Idle` by starting the next one, so nothing needs taking here.
                self.answer_the_failure(download, &Words::of(self.preferences.language), fault);
            }
            HttpState::Idle | HttpState::Running => {
                if let Some(progress) = self.board.http().progress() {
                    download.received = progress.received;

                    if progress.total > 0 {
                        download.total = progress.total;
                    }
                }

                self.download = Some(download);
            }
        }
    }

    fn answer_the_failure(&mut self, download: Download, words: &Words, fault: HttpFault) {
        self.status = Status::Answered;
        self.prompt = Some(Prompt::report(
            download.name,
            words.fault(fault).to_string(),
            Tone::Failed,
            words,
        ));
    }

    fn finish_download(&mut self, download: Download) {
        let words = Words::of(self.preferences.language);
        let answer = self.board.http().take_result();

        let landed = match answer {
            Ok(response) if response.status == 200 => manager::install_file(
                &download.temp,
                &download.expected,
                &self.roots,
                Some(download.root.source),
            ),
            // A `404` is a body like any other and lands in the file just as willingly: nothing here
            // may keep it, and the row is told the repository did not have the package.
            Ok(_) | Err(_) => {
                discard(&download.temp);

                self.status = Status::Answered;
                self.prompt = Some(Prompt::report(
                    download.name,
                    words.not_found.to_string(),
                    Tone::Failed,
                    &words,
                ));

                return;
            }
        };

        self.status = Status::Answered;

        match landed {
            Ok(installed) => {
                self.catalog = Catalog::scan(self.roots.clone());
                self.changed = true;

                self.prompt = Some(Prompt::report(
                    download.name,
                    format!("{} {}", words.installed_ok, installed.meta.version),
                    Tone::Done,
                    &words,
                ));
            }
            Err(error) => {
                self.prompt = Some(Prompt::report(
                    download.name,
                    error.to_string(),
                    Tone::Failed,
                    &words,
                ));
            }
        }
    }

    /// Asks before removing something.
    fn ask_uninstall(&mut self, id: &str) {
        let words = Words::of(self.preferences.language);

        let name = match self.catalog.get(id) {
            Some(app) => self.installed_name(app).to_string(),
            None => id.to_string(),
        };

        self.prompt = Some(Prompt {
            title: words.uninstall_title.to_string(),
            line: words.uninstall_question.replace("{name}", &name),
            detail: words.uninstall_detail.to_string(),
            progress: None,
            bytes: String::new(),
            button: words.cancel,
            confirm: Some(words.uninstall),
            tone: Tone::Working,
            action: Action::Uninstall(id.to_string()),
        });
    }

    /// Does what the prompt's second button asked about.
    fn confirm(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };

        match prompt.action {
            Action::None => {}
            Action::Uninstall(id) => self.remove(&id),
        }
    }

    fn remove(&mut self, id: &str) {
        let words = Words::of(self.preferences.language);

        let name = match self.catalog.get(id) {
            Some(app) => self.installed_name(app).to_string(),
            None => id.to_string(),
        };

        let root = self
            .catalog
            .get(id)
            .and_then(|app| self.roots.iter().find(|root| root.source == app.source))
            .or_else(|| self.roots.iter().find(|root| root.path.join(id).is_dir()))
            .cloned();

        let Some(root) = root else {
            self.prompt = Some(Prompt::report(
                name,
                words.no_volume,
                Tone::Failed,
                &words,
            ));

            return;
        };

        match manager::uninstall(&root, id) {
            Ok(()) => {
                self.catalog = Catalog::scan(self.roots.clone());
                self.changed = true;

                self.prompt = Some(Prompt::report(
                    name,
                    words.uninstalled_ok.to_string(),
                    Tone::Done,
                    &words,
                ));
            }
            Err(error) => {
                self.prompt = Some(Prompt::report(name, error.to_string(), Tone::Failed, &words));
            }
        }
    }

    /// Puts the prompt away — and stops the download when the prompt is one.
    fn dismiss(&mut self) {
        if matches!(self.status, Status::Downloading(_)) {
            self.board.http().cancel().ok();

            if let Some(download) = self.download.take() {
                discard(&download.temp);
            }

            self.status = Status::Answered;
        }

        self.prompt = None;
    }

    fn write_cache(&self, document: &str) {
        if let Some(dir) = self.cache.parent() {
            std::fs::create_dir_all(dir).ok();
        }

        if let Err(error) = std::fs::write(&self.cache, document) {
            // A cache that cannot be written is not a failure of the check: the page has the
            // manifest in memory and nothing a person pressed depends on the file.
            eprintln!("[store] could not cache the manifest: {error}");
        }
    }

    /// The page's messages.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(_) => self.poll(),
            Message::Section(section) => self.section = section,
            Message::Check => self.check(),
            Message::Install(id) => self.install(&id),
            Message::Uninstall(id) => self.ask_uninstall(&id),
            Message::Confirm => self.confirm(),
            Message::Dismiss => self.dismiss(),
        }
    }

    /// The page.
    pub fn view(&self) -> iced::Element<'_, Message> {
        ui::page(self)
    }

    /// The frames, while something is out.
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
}

/// Throws away a download's half-written file, and the staging directory with it when it is empty.
///
/// Both, and in that order: a `.part` nobody will finish is garbage on a volume whose free space is
/// the install's own limit, and an empty `.staging` left behind would be a directory the next boot's
/// cleanup has to find for no reason. A file that is already gone is not a fault — the client
/// removes its own half.
fn discard(path: &Path) {
    std::fs::remove_file(path).ok();

    if let Some(staging) = path.parent() {
        std::fs::remove_dir(staging).ok();
    }
}

/// The one thing the page needs from `HttpFault`: a sentence.
///
/// The store does not have the updater's `Words`, and a fault is a *reason a person can act on* —
/// look at the network, look at the clock, look at the card — so it is written here as the store's
/// own copy of the same list, in both languages.
fn read_cache(path: &Path) -> Manifest {
    let Ok(document) = std::fs::read_to_string(path) else {
        return Manifest::default();
    };

    let manifest = Manifest::parse(&document);

    if manifest.is_empty() {
        Manifest::default()
    } else {
        manifest
    }
}

/// A number of bytes as a person reads one: `228 KB`, `5.8 MB`, `12 B`.
pub fn size_text(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    const MB: f64 = 1000.0 * 1000.0;

    let bytes = bytes as f64;

    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{:.0} B", bytes)
    }
}

/// The page's fixed words, in the two languages it is written in.
///
/// A struct and not a table, for the reason the updater gives: every one of these is read by name,
/// so a missing word is a compile error rather than a `???` on the panel.
pub struct Words {
    /// The language these words are in, which is what [`Words::fault`] and the rows read.
    pub language: Language,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub installed: &'static str,
    pub available: &'static str,
    pub updates: &'static str,
    pub check: &'static str,
    pub checking: &'static str,
    pub install: &'static str,
    pub upgrade: &'static str,
    pub uninstall: &'static str,
    pub remove: &'static str,
    pub cancel: &'static str,
    pub close: &'static str,
    pub downloading: &'static str,
    pub installed_ok: &'static str,
    pub uninstalled_ok: &'static str,
    pub uninstall_title: &'static str,
    pub uninstall_question: &'static str,
    pub uninstall_detail: &'static str,
    pub empty_installed: &'static str,
    pub empty_available: &'static str,
    pub empty_updates: &'static str,
    pub unreadable: &'static str,
    pub no_volume: &'static str,
    pub storage_failed: &'static str,
    pub busy: &'static str,
    pub not_found: &'static str,
    pub broken: &'static str,
    pub disabled: &'static str,
    pub internal_storage: &'static str,
    pub card: &'static str,
    pub needs_update: &'static str,
}

impl Words {
    /// The words in one language.
    pub fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language: Language::Chinese,
                title: TITLE,
                subtitle: SUBTITLE,
                installed: "已安装",
                available: "可安装",
                updates: "更新",
                check: "检查",
                checking: "正在检查仓库…",
                install: "安装",
                upgrade: "更新",
                uninstall: "卸载",
                remove: "移除",
                cancel: "取消",
                close: "关闭",
                downloading: "正在下载",
                installed_ok: "已安装",
                uninstalled_ok: "已卸载",
                uninstall_title: "卸载应用",
                uninstall_question: "卸载 {name}？",
                uninstall_detail: "它写入的数据也会一起删除。",
                empty_installed: "还没有安装任何应用",
                empty_available: "仓库里没有可安装的应用",
                empty_updates: "所有应用都是最新的",
                unreadable: "仓库的清单读不出来",
                no_volume: "没有可写入的存储",
                storage_failed: "创建下载目录失败",
                busy: "网络正忙，稍后再试",
                not_found: "仓库里没有这个文件",
                broken: "这个包读不出来",
                disabled: "已停用",
                internal_storage: "内置存储",
                card: "存储卡",
                needs_update: "需要更新系统",
            },
            Language::English => Self {
                language: Language::English,
                title: "App Store",
                subtitle: "Install, update and remove applications",
                installed: "Installed",
                available: "Available",
                updates: "Updates",
                check: "Check",
                checking: "Asking the repository…",
                install: "Install",
                upgrade: "Update",
                uninstall: "Remove",
                remove: "Remove",
                cancel: "Cancel",
                close: "Close",
                downloading: "Downloading",
                installed_ok: "Installed",
                uninstalled_ok: "Removed",
                uninstall_title: "Remove application",
                uninstall_question: "Remove {name}?",
                uninstall_detail: "The data it wrote is deleted with it.",
                empty_installed: "No applications are installed",
                empty_available: "Nothing in the repository to install",
                empty_updates: "Every application is up to date",
                unreadable: "The repository's manifest could not be read",
                no_volume: "No writable storage",
                storage_failed: "Could not create the download directory",
                busy: "The network is busy, try again",
                not_found: "The repository does not have that file",
                broken: "This package could not be read",
                disabled: "Stopped",
                internal_storage: "Internal storage",
                card: "Memory card",
                needs_update: "Needs a newer system",
            },
        }
    }

    /// Why a request did not come back.
    pub fn fault(&self, fault: HttpFault) -> &'static str {
        let chinese = self.language == Language::Chinese;

        match (fault, chinese) {
            (HttpFault::NoNetwork, true) => "没有网络",
            (HttpFault::NoNetwork, false) => "No network",
            (HttpFault::ClockNotSet, true) => "系统时间还没设置",
            (HttpFault::ClockNotSet, false) => "The clock has not been set",
            (HttpFault::Unreachable, true) => "连不上仓库",
            (HttpFault::Unreachable, false) => "The repository is unreachable",
            (HttpFault::Timeout, true) => "请求超时",
            (HttpFault::Timeout, false) => "The request timed out",
            (HttpFault::Tls, true) => "安全连接失败",
            (HttpFault::Tls, false) => "The secure connection failed",
            (HttpFault::TooLarge, true) => "应答太大",
            (HttpFault::TooLarge, false) => "The answer is too large",
            (HttpFault::Storage, true) => "写入存储失败",
            (HttpFault::Storage, false) => "Writing to storage failed",
            (HttpFault::Unsupported, true) => "这个固件没有联网能力",
            (HttpFault::Unsupported, false) => "This image has no network client",
        }
    }
}

/// The line under an installed row: its version, the volume it is on, and whether it is stopped.
pub(crate) fn installed_line(app: &Installed, words: &Words) -> String {
    let source = match app.source {
        Source::Internal => words.internal_storage,
        Source::Card => words.card,
    };

    let mut line = format!("{} · {source}", app.meta.version);

    if app.disabled {
        line.push_str(" · ");
        line.push_str(words.disabled);
    }

    line
}

/// The line under a repository row: its version, its size, and the one sentence about it.
pub(crate) fn release_line(release: &AppRelease, words: &Words) -> String {
    let mut line = format!(
        "{} · {}",
        release.version.text(),
        if release.size == 0 {
            "—".to_string()
        } else {
            size_text(release.size)
        }
    );

    if !release.description.is_empty() {
        line.push_str(" · ");
        line.push_str(&release.description);
    }

    // A package built for a newer ABI is shown rather than hidden: the row's button is what cannot
    // work, and a person is owed that before they press it.
    if release.abi > app_abi_major() {
        line.push_str(" · ");
        line.push_str(words.needs_update);
    }

    line
}

/// The ABI major this host implements, restated here because the store never runs a package.
fn app_abi_major() -> u16 {
    1
}
