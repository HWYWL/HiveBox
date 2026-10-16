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
//! The font's is a *file name* on the card — see [`FONT_DIR`] — because that is where a font lives: the
//! image carries a 24 px coverage cut down to the characters this source tree draws, and the ones it
//! leaves out — a file name, a network's name, a line from a server — need coverage the flash has no room
//! for beside a second app slot. So the file that carries it names its version and the page reads the
//! newest it finds. A box with no card has no installed font version, which is a state this page draws
//! rather than an error.
//!
//! A version can be carried by *two* files, which is what [`card_font`] is about: a face and the table
//! baked from it, naming the same version. The pair and not the table alone, because a table's glyph
//! numbers are the numbers of the font it was baked from — a table covers the characters its *face* has,
//! and the built-in subset has 528 of them. A card that brings a wider face therefore has to bring the
//! coverage for it, and this page names the version of the pair it finds.
//!
//! The firmware's boot asks the same question of the same function, [`card_font`]: the font this page
//! names is the font the interface is drawn with, because both are asking the one question of the one
//! scan.
//!
//! # The glyph table downloads; the image does not yet
//!
//! The two rows' presses are not the same size. A table is a file on a card, so fetching one is
//! [`HttpBackend::start_to`] with a [`Sink::File`] — the bytes go to the card as they arrive and are
//! never on this side of the network stack, which is what makes a quarter megabyte a thing this board
//! can take. An image is a *flash write*, into the app slot it is not running from, and that is a
//! backend [`HttpBackend`] is not: [`Status::Unsupported(Kind::Firmware)`](Status::Unsupported) is what
//! that row's press draws until it exists.
//!
//! A press on the table's row can be *two* requests, because a font that covers more than the image's
//! subset is two files: the wider face, and the table baked from it. They are one install and not two —
//! see [`Download::next`] — so the prompt stays up, the bar goes on, and the row is re-read once, when
//! the table at the end of the chain is on the card.
//!
//! A download in flight is drawn from [`Download`], which is the request's progress read once a frame —
//! the frame subscription exists for exactly that, and a minute of downloading with nothing moving on
//! the screen is indistinguishable from a box that has hung.
//!
//! # What a press leaves behind
//!
//! A press is per-row and its outcome is modal — [`Prompt`] — while the *check* is page-wide and its
//! state is the line at the bottom. The two are drawn apart because they are asked apart: one manifest
//! answers for both rows, and a person who pressed one button is being told about that button.
//!
//! # An install is not finished until the box has been reset
//!
//! A file that has just landed is not a file in use: the interface draws from the table the *boot*
//! loaded, and an image written into the second slot is not the image running. So the sentence that
//! reports an install is the page's one *question* — restart now, or later — and the one place here
//! where two buttons are honest, because there is something to choose between and the box cannot use
//! what it has just taken until it has been reset. See [`Message::Restart`].

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

use pomelo_hal::{Board, HalError, HttpFault, HttpState, Sink, VolumeKind};
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

/// The suffix a face on the card has: the other half of a package, and the half that decides how many
/// characters the table beside it can be about. See [`card_font`].
pub const FACE_SUFFIX: &str = ".otf";

/// What the page reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// A frame, which is the clock the request's progress is read on.
    Tick(Instant),
    /// The check button: ask the repository what it has.
    Check,
    /// The button on a row that has something to fetch.
    Install(Kind),
    /// The prompt's first button: put it away — and abandon the download, when what it is covering is
    /// one. The two things a person might want while a bar moves (let it finish, stop it) are the same
    /// button at different moments: there is nothing to confirm and nothing to choose, only this to be
    /// rid of. It is also the answer to the restart question that is *not* now — see [`Words::later`].
    Dismiss,
    /// The prompt's second button, and the only second button on the page: reset the box, which is the
    /// one way the file that has just landed becomes the file the box is using.
    ///
    /// On the board this does not return: the SoC goes back to its bootloader and the frame that would
    /// have drawn the prompt away is never drawn. What is left to handle is a backend that cannot reset
    /// it, and that case says so rather than going on as if the machine had restarted.
    Restart,
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
    /// A button was pressed that this image cannot honour yet: the OTA write is what this page is
    /// waiting on, and until it exists the honest drawing of that press is the sentence that says so
    /// rather than a progress bar that never moves. Also the glyph table's press on a box with no card,
    /// which is a refusal about the *box* rather than about the code — see [`Words::unsupported_line`].
    Unsupported(Kind),
    /// A download is out, and [`Updater::download`] is how far along it is.
    Downloading(Kind),
    /// A download finished and the file it wrote is where the interface will read it.
    ///
    /// Not the same as [`Status::Answered`], which is what the rows look like once a *check* has
    /// answered: this is a press that changed something, and the rows have been re-read since, so the
    /// version in the row is the one that just landed.
    Installed(Kind),
    /// A download did not become an install, and this is why.
    DownloadFailed(Kind, DownloadFault),
}

/// Why a download did not become an install.
///
/// Beside [`HttpFault`] and not instead of it, because two of the three ways a download fails are not
/// ways a *request* fails: a request that came back `404` did everything an HTTP request is supposed
/// to, and the file still is not there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DownloadFault {
    /// The request itself went wrong, in the ways a request goes wrong: no network, no clock, no
    /// server, no room on the card. Carried rather than restated, because [`HttpFault`] already has a
    /// sentence for each of them and a second list of them here would be a second list to keep in step.
    Http(HttpFault),
    /// The repository answered, and not with the file: a `404`, or a body that is a page rather than a
    /// table. What arrived was written and then thrown away — see [`discard`] — because a body lands in
    /// a file just as willingly whatever it says.
    NotFound,
    /// The file is not the size the manifest named. The one failure this page checks for itself: the
    /// request completed and the file is short of what it should be, which is a table the interface
    /// would read half of, and the card is where a person goes to look.
    Incomplete,
}

/// A download that is out, and how far along it is.
///
/// Kept beside [`Status::Downloading`] rather than inside it, because it is a *reading* of the request
/// and not the page's whole state: it is replaced every frame the request is in flight and dropped the
/// moment the answer is taken, while the status is what the page draws.
pub struct Download {
    /// Where the bytes are coming from: the manifest's own directory and [`Download::file`].
    ///
    /// Kept and not worked out again at each step, because one press can be two files and the second
    /// one's address is the manifest's answer about a name that is not this one — see
    /// [`Updater::install_font`].
    pub(crate) url: String,
    /// The file's name on the repository — what the prompt says is being fetched. Not the path, which
    /// is [`FONT_DIR`] plus this and is nobody's business but the write's.
    pub(crate) file: String,
    /// Where the bytes are going, kept so that the finished size can be read off the card.
    pub(crate) path: String,
    /// How many bytes the manifest said the file is; `0` when it did not say, in which case the
    /// finished size is not checked against anything.
    pub(crate) expected: u64,
    /// How many have arrived so far.
    pub(crate) received: u64,
    /// How many there will be: the server's `Content-Length` when it gave one, and the manifest's
    /// [`Download::expected`] when it did not. `0` only when neither said — a bar with no end, and a
    /// count of bytes rather than a percentage.
    pub(crate) total: u64,
    /// The file to fetch once this one has landed, when the press is a package: the table, behind the
    /// face it was baked from. `None` for everything that is one file.
    ///
    /// Boxed because it is the tail of a list, and the *order* is the whole of what it says: the table
    /// is the file the boot reads, so the font is not installed until the table has landed. A face on
    /// its own is a file nothing draws with — the scan pairs it by version, and it has no table — and a
    /// table on its own would be drawn with the glyph numbers of the face the *image* carries, which is
    /// the failure the pair exists to prevent. So the half that lands first is harmless if the press is
    /// abandoned or the second half fails, and the card is left with the font it already had.
    pub(crate) next: Option<Box<Download>>,
}

impl Download {
    /// One file of a press: where it comes from, what it is called, and where it goes.
    ///
    /// `dir` is the card's directory rather than the file's, because a download's two names are one:
    /// the name on the repository is the name the card gets, and a manifest that named a path would put
    /// a table somewhere the interface does not scan. See [`FONT_DIR`].
    pub(crate) fn of(url: &str, file: &str, dir: &str, size: u64, next: Option<Box<Download>>) -> Self {
        Self {
            url: url.to_string(),
            file: file.to_string(),
            path: format!("{}/{}", dir.trim_end_matches('/'), file),
            expected: size,
            received: 0,
            // The manifest's size to begin with, so that the bar has an end before the server's headers
            // have been read — and gets the server's own the moment they are.
            total: size,
            next,
        }
    }

    /// How much of it is here, from `0.0` to `1.0`, or `None` when nothing has said how big it is.
    ///
    /// The denominator [`Download::total`] and never [`Download::expected`] on its own: the server's
    /// length is the one the bytes actually have, and a manifest that named a size the file does not
    /// have would otherwise be drawn as a bar that stopped short of its own end.
    pub(crate) fn fraction(&self) -> Option<f32> {
        if self.total == 0 {
            return None;
        }

        Some((self.received as f32 / self.total as f32).clamp(0.0, 1.0))
    }

    /// How far along, as a person reads one: `84 KB / 228 KB`, or the bytes alone when nothing has
    /// said how many there will be.
    ///
    /// Both numbers and not just the percentage, because they answer different questions: the
    /// percentage is "how long", and the count is "is this thing actually moving" — which is the one
    /// question a bar that is waiting on a slow card raises and cannot answer by itself.
    pub(crate) fn bytes_line(&self) -> String {
        match self.total {
            0 => size_text(self.received),
            total => format!("{} / {}", size_text(self.received), size_text(total)),
        }
    }
}

/// What the prompt over the page is about, when there is one over the page at all.
///
/// A press's outcome, and only a press's: the check's state is the line at the bottom of the page, which
/// is always there and says one quiet thing, while this is modal and interrupts. The two are drawn apart
/// because they are asked apart — one manifest answers for both rows, and a person who pressed one
/// button is being told about that button. See this module's header.
///
/// A *description* and not a widget: which sentence, whether there is a bar under it, and whether the one
/// button says "cancel" or "close" are decisions about the page's content, and they are made here where
/// the rest of the project can read them and a test can hold them up. Where the ink comes from is
/// [`ui`]'s business.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prompt {
    /// What it is about: the row's own name.
    pub(crate) title: &'static str,
    /// The sentence under the title.
    pub(crate) line: String,
    /// The line under that, when there is one: the file, which is a name a person recognises as the
    /// thing they asked for — and not the path, which is a card's business.
    pub(crate) detail: String,
    /// How much of the file is here, when a download is what this is about; `None` for a prompt that is
    /// only a sentence, and for a download whose size nothing has said.
    pub(crate) progress: Option<f32>,
    /// How far along, in bytes. Empty when there is no download.
    pub(crate) bytes: String,
    /// What the first button says: [`Words::cancel`] while a download is out, [`Words::later`] when the
    /// sentence is a question, and [`Words::close`] for everything else — the same button at the three
    /// moments a person could want it, which is *now*, *not yet* and *whenever*.
    pub(crate) button: &'static str,
    /// What the second button says, when there is one: [`Words::restart`] for an install, which is the
    /// one thing on this page a person is expected to do next. `None` everywhere else, because a second
    /// button is only honest where there is something to choose between — see this module's header.
    pub(crate) confirm: Option<&'static str>,
}

/// A number of bytes as a person reads one: `228 KB`, `5.8 MB`, `12 B`.
///
/// Decimal and not binary, and one decimal place only above a megabyte: a person comparing a file with
/// the room left on a card is reading the card's own numbers, and every card sold is sold in decimal —
/// so a size worked out in 1024s would be a size that disagreed with the box it is about to be put in.
pub(crate) fn size_text(bytes: u64) -> String {
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

/// Throws away the file a download that did not become an install left behind.
///
/// The *name* is how the interface finds a table — see [`card_font_in`] — so a `404` page
/// sitting where a table should be is not a harmless left-over: it is the newest table the box would
/// read, and the boot after it would be drawing from an HTML error page. The failure is reported and
/// the evidence removed.
///
/// A file that is not there is not a fault. The device's client removes its own half-written file, so
/// there are two ends that may have got there first, and the one that arrives second has nothing to do.
fn discard(path: Option<&str>) {
    let Some(path) = path else { return };

    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("[updater] could not remove {path}: {error}");
        }
    }
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
    /// The download in flight, when there is one. `Some` exactly while [`Status::Downloading`] is what
    /// the page is drawing; see [`Download`].
    pub(crate) download: Option<Download>,
    /// Which directory a table is read from and written to. [`FONT_DIR`] on the board, and a temporary
    /// one in a test: the page's subject is *the card's* directory, and a test that downloaded into it
    /// would be a test that wrote to the machine that ran it.
    pub(crate) font_dir: String,
    /// Whether a card is in the slot, as of the last press that needed to know.
    ///
    /// The second half of the glyph table's answer, and a question only the slot itself can settle:
    /// this board wires its microSD to SDMMC with no card-detect line, so "is there anywhere to put a
    /// table" is answered by *mounting* whatever is there rather than by reading a pin. Asked at the
    /// press and not kept in step, because the press is the moment the answer is drawn; see
    /// [`card_mounted`].
    pub(crate) card: bool,
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
        let font_dir = FONT_DIR.to_string();
        let installed = read_installed(&board, &font_dir);
        let mut app = Self {
            board,
            status: Status::Idle,
            manifest: Manifest::default(),
            installed,
            download: None,
            font_dir,
            // Nothing has asked the slot yet, and nothing needs to until a table's button is pressed:
            // what the rows draw before that is a version, and a card is not one.
            card: false,
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
    ///
    /// A download as much as a check, and one subscription does both jobs: a check's frames are there
    /// so that the answer is read the moment it lands, and a download's are there so that the bar
    /// moves. Both are the same sentence — something is out and it is not finished.
    pub(crate) fn waiting(&self) -> bool {
        matches!(self.status, Status::Checking | Status::Downloading(_))
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

    /// Reads whichever request is out, if there is one to read.
    ///
    /// Called from [`Message::Tick`], which is the only clock a frame-driven request has on a board whose
    /// host serves no frames of its own. `Done` is taken exactly once, because the state goes on saying
    /// it until the answer is collected and a page that read it every frame would be a page reading the
    /// same body sixty times a second.
    ///
    /// Which of the two readers runs is [`Status`]'s question rather than the backend's, and that is the
    /// point: there is one request slot, so what is out is whatever the page last asked for, and the
    /// two answers are read differently — one becomes a manifest and the other is already a file.
    fn poll(&mut self) {
        if matches!(self.status, Status::Downloading(_)) {
            self.poll_download();

            return;
        }

        self.poll_check();
    }

    /// Reads the check's request, if there is one to read.
    fn poll_check(&mut self) {
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
                            let installed = read_installed(&self.board, &self.font_dir);
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

    /// Reads the download's request, if there is one to read.
    ///
    /// The progress and *then* the state, in that order and not the other: the counters stop answering
    /// the moment the request leaves `Running`, so a bar read after the state would jump from wherever
    /// it was to nothing on the frame the file lands. Read this way, the last thing the bar draws is
    /// the whole file, and the frame after it the prompt says what happened.
    fn poll_download(&mut self) {
        if let Some(progress) = self.board.http().progress() {
            if let Some(download) = self.download.as_mut() {
                download.received = progress.received;

                // The server's length when it gave one, and the manifest's when it did not — which is
                // already in `total` from the start. A `0` here is a server that said nothing, and the
                // page's own size is a better answer than none.
                if progress.total > 0 {
                    download.total = progress.total;
                }
            }
        }

        // Read out and let go of, rather than matched on directly: the backend hands out a guarded
        // reference, and an arm that went on to borrow the page again would be a borrow held while the
        // guard's own drop is still to run.
        let state = self.board.http().state();

        match state {
            HttpState::Done => {
                let answer = self.board.http().take_result();

                self.finish_download(answer);
            }
            HttpState::Failed(fault) => {
                let kind = self.downloading_kind();

                self.download = None;
                self.status = Status::DownloadFailed(kind, DownloadFault::Http(fault));
            }
            HttpState::Idle | HttpState::Running => {}
        }
    }

    /// The kind whose download is out, which is what a finished or broken request belongs to.
    ///
    /// Read off the status and not kept a second time beside it: the status *is* the answer to "whose
    /// request is this", and a copy of it would be a copy that could disagree.
    fn downloading_kind(&self) -> Kind {
        match self.status {
            Status::Downloading(kind) => kind,
            _ => Kind::Font,
        }
    }

    /// Reads the answer to a finished download.
    ///
    /// The request coming back is not the same as the file being there, and so there are three things
    /// to check rather than one. A `404` is an answer, and its body is a page — which lands in the file
    /// as readily as a table does, and would then be the newest table the interface read. And the size:
    /// the C side refuses to leave a half-written file behind, so a short file here would mean the two
    /// ends disagree about which file this is, which is worth saying rather than discovering as a table
    /// the interface draws half of.
    ///
    /// Every way this can fail ends the same way — [`discard`] — because the file is the thing that
    /// makes the failure dangerous and not the status: see that function.
    ///
    /// A file that landed and is not the last one of a press is not an install yet: what follows it is
    /// started and the bar goes on — see [`Download::next`] — and only the file at the end of the chain
    /// ends the press. The checks are per file and not per press for the reason they exist: a size is a
    /// statement about *this* file, and a `404` on the second half of a package is the second half's
    /// answer.
    fn finish_download(&mut self, answer: Result<pomelo_hal::HttpResponse, HalError>) {
        let kind = self.downloading_kind();
        let download = self.download.take();
        let path = download.as_ref().map(|download| download.path.clone());

        self.status = match answer {
            Ok(response) if response.ok() => {
                let written = path
                    .as_deref()
                    .and_then(|path| std::fs::metadata(path).ok())
                    .map(|meta| meta.len());
                let expected = download.as_ref().map(|it| it.expected).unwrap_or(0);

                match written {
                    Some(written) if expected != 0 && written != expected => {
                        discard(path.as_deref());

                        Status::DownloadFailed(kind, DownloadFault::Incomplete)
                    }
                    _ => match download.and_then(|download| download.next) {
                        Some(next) => self.start_half(*next, kind),
                        None => {
                            // Re-read, so that the row's version is the one that just landed rather
                            // than the one the last check found. This is the whole reason the page can
                            // say "updated" without keeping a version of its own: the card is the
                            // record.
                            let installed = read_installed(&self.board, &self.font_dir);
                            self.installed = installed;

                            Status::Installed(kind)
                        }
                    },
                }
            }
            Ok(_) => {
                discard(path.as_deref());

                Status::DownloadFailed(kind, DownloadFault::NotFound)
            }
            Err(_) => {
                discard(path.as_deref());

                Status::DownloadFailed(kind, DownloadFault::Http(HttpFault::Unreachable))
            }
        };
    }

    /// Starts the file that follows one that landed, and answers what the page is drawing now.
    ///
    /// A press of two files is *one* install to a person, so the status stays [`Status::Downloading`] and
    /// the bar goes on where it was: a prompt that blinked between the two halves would read as two
    /// presses, and the file name under the sentence — which does change — is the answer to "what is it
    /// doing now". What is thrown away is the reading of the request that just ended, because the next
    /// one's progress has not been read yet; the new [`Download`] starts its bar at nothing, which is
    /// where the file it is about is.
    ///
    /// A request that does not get away is a press that failed: there is no fault in the list for "the
    /// second file could not be asked for", and the nearest true thing is the same one a first half that
    /// could not be asked for gets — nothing answerable was reached. The half that landed is a real file
    /// and stays, where it changes nothing until its table is there.
    fn start_half(&mut self, half: Download, kind: Kind) -> Status {
        match self
            .board
            .http()
            .start_to(&half.url, Sink::File(half.path.clone()))
        {
            Ok(()) => {
                self.download = Some(half);

                Status::Downloading(kind)
            }
            Err(_) => Status::DownloadFailed(kind, DownloadFault::Http(HttpFault::Unreachable)),
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
            Message::Install(kind) => self.install(kind),
            Message::Dismiss => self.dismiss(),
            Message::Restart => self.restart(),
        }
    }

    /// A press on one of the two buttons.
    ///
    /// The two rows' presses are not the same size, and this is where that is decided: a table is a file
    /// on a card and the request to fetch one is a request this board already makes, while an image is a
    /// flash write and the backend for that is still the thing this page is waiting on.
    fn install(&mut self, kind: Kind) {
        match kind {
            Kind::Firmware => self.status = Status::Unsupported(Kind::Firmware),
            Kind::Font => self.install_font(),
        }
    }

    /// Starts the glyph table's download — the first of its two files, when it has two — or says why it
    /// did not start.
    ///
    /// Three questions in a row, and each of them is a different answer to draw: is there a card (no →
    /// the one sentence about that), is there something to fetch (no → this press raced the check, so
    /// ask again), and did the request get away (no → the reason it did not). Which file is asked for
    /// first is not one of the questions: it is [`Download::next`]'s and it is the face, so that the
    /// file the boot scans for is the one that finishes the press.
    fn install_font(&mut self) {
        // The slot first: this is the moment the answer is needed, and it is a *mount* rather than a
        // read of a pin — see [`card_mounted`] for why that is not the same question.
        self.card = card_mounted(&self.board);
        if !self.card {
            self.status = Status::Unsupported(Kind::Font);

            return;
        }

        // Taken out of the borrow before anything can make one: the arms below start a request, and a
        // `Release` borrowed from `self` would be a borrow held across that.
        let offer = self.offer(Kind::Font).cloned();
        let Some(release) = offer else {
            // The row's button is only drawn beside an offer, so a press that arrives here is one that
            // raced the check — or a row whose offer the last answer withdrew. Asking again is what a
            // person who pressed it wanted, and it is one request on a button that is already there.
            self.check();

            return;
        };

        // The directory before the file: [`Sink::File`] opens the path it is given and does not make
        // one, and a card in the slot is not yet a card with this page's directory on it.
        if let Err(error) = std::fs::create_dir_all(&self.font_dir) {
            eprintln!("[updater] cannot make {}: {error}", self.font_dir);
            self.status = Status::DownloadFailed(
                Kind::Font,
                DownloadFault::Http(HttpFault::Storage),
            );

            return;
        }

        // The table is the file the boot reads, so it is the one that finishes the press: the face, when
        // the manifest named one, goes in front of it and the two are chained — see [`Download::next`].
        let table = Download::of(
            &release.url(),
            &release.file,
            &self.font_dir,
            release.size,
            None,
        );

        let download = match &release.face {
            Some(face) => Download::of(
                &face.url(),
                &face.file,
                &self.font_dir,
                face.size,
                Some(Box::new(table)),
            ),
            None => table,
        };

        match self
            .board
            .http()
            .start_to(&download.url, Sink::File(download.path.clone()))
        {
            Ok(()) => {
                self.download = Some(download);
                self.status = Status::Downloading(Kind::Font);
            }
            // One request at a time, and the one in flight is the check that found this offer: nothing
            // to draw, because that answer is the one this press was waiting for.
            Err(HalError::Busy) => self.status = Status::Checking,
            // An URL this board cannot fetch is a fault of the manifest's own contents rather than of
            // the network, and there is no fault in the list for "the file it named is not fetchable".
            // The nearest true thing to say is that nothing answerable was reached.
            Err(_) => {
                self.status = Status::DownloadFailed(
                    Kind::Font,
                    DownloadFault::Http(HttpFault::Unreachable),
                )
            }
        }
    }

    /// Resets the box, which is what the second button on an install's prompt asks for.
    ///
    /// The one thing this page can ask for that it cannot undo, and the point of installing anything:
    /// the file has landed and nothing is using it yet. On the board the call does not return — the SoC
    /// is back in its bootloader — so what is left to get right is the host's case, where there is no
    /// board to reset: the prompt goes away, because a page still asking whether to restart a machine
    /// that has already answered would be a page that did not believe its own call.
    ///
    /// A backend that *refuses* is the other case, and there the prompt stays up: the sentence is still
    /// true — the version is on the box and not in use — and pressing again is the answer.
    fn restart(&mut self) {
        // The lock is taken and given back before the prompt is touched: the answer is a `Result` and
        // not a borrow of the board, and `dismiss` wants the page to itself.
        let asked = self.board.power().restart();

        match asked {
            Ok(()) => self.dismiss(),
            Err(error) => eprintln!("[updater] the board could not be restarted: {error}"),
        }
    }

    /// Puts the prompt away, abandoning the download if that is what it was covering.
    ///
    /// The status goes back to what the rows were saying before the press, which is
    /// [`Status::Answered`] when a check has answered and the check itself when one has not: what the
    /// prompt was covering is over either way, and the rows are still the page's subject.
    fn dismiss(&mut self) {
        if matches!(self.status, Status::Downloading(_)) {
            // Abandoned on the way out: the C side notices between chunks and removes the half-written
            // file, so a cancelled download leaves nothing where the interface would find it.
            if let Err(error) = self.board.http().cancel() {
                eprintln!("[updater] could not stop the download: {error}");
            }

            self.download = None;
        }

        self.status = if self.manifest.is_empty() {
            Status::Idle
        } else {
            Status::Answered
        };

        // Whatever the press did to the card, the next drawing of the rows should be the truth: a
        // cancelled download has left the card as it was, and a finished one may have changed which
        // table is the newest.
        let installed = read_installed(&self.board, &self.font_dir);
        self.installed = installed;
    }

    /// What the prompt over the page says, when there is one over the page at all.
    ///
    /// `None` for every state the page draws *in place* — nothing pressed yet, a check on its way, a
    /// check that answered — because a modal is an interruption and only a press earns one. A page that
    /// put its check's state in a popup too would be a page interrupting its reader to say "checking".
    pub(crate) fn prompt(&self, words: &Words) -> Option<Prompt> {
        let mut prompt = Prompt {
            title: "",
            line: String::new(),
            detail: String::new(),
            progress: None,
            bytes: String::new(),
            button: words.close,
            confirm: None,
        };

        match &self.status {
            Status::Unsupported(kind) => {
                prompt.title = words.name(*kind);
                prompt.line = words.unsupported_line(*kind, self.card).to_string();
            }
            Status::Downloading(kind) => {
                prompt.title = words.name(*kind);
                prompt.line = words.downloading.to_string();
                prompt.button = words.cancel;

                if let Some(download) = self.download.as_ref() {
                    prompt.detail = download.file.clone();
                    prompt.progress = download.fraction();
                    prompt.bytes = download.bytes_line();
                }
            }
            Status::Installed(kind) => {
                prompt.title = words.name(*kind);

                match self.installed_version(*kind) {
                    // The version is on the box and not yet in use, so the report is also a question:
                    // restart now, or later. The page's only second button, and the only place where
                    // what a person does *next* is one of two things rather than one.
                    Some(version) => {
                        prompt.line = words.installed_ok_line(*kind, version);
                        prompt.button = words.later;
                        prompt.confirm = Some(words.restart);
                    }
                    // The file landed and the *scan* did not find it, so there is no version to name.
                    // Nothing was installed and this page will not claim otherwise: the nearest true
                    // sentence is the one about a write that did not take — and there is nothing to
                    // restart *into*, so no second button either.
                    None => prompt.line = words.fault_line(HttpFault::Storage).to_string(),
                }
            }
            Status::DownloadFailed(kind, fault) => {
                prompt.title = words.name(*kind);
                prompt.line = words.download_fault_line(*fault).to_string();
            }
            Status::Idle
            | Status::Checking
            | Status::Answered
            | Status::Failed(_)
            | Status::Unreadable => return None,
        }

        Some(prompt)
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
pub(crate) fn read_installed(board: &Board, dir: &str) -> Installed {
    Installed {
        firmware: installed_firmware(board),
        font: installed_font(dir),
    }
}

/// The version stamped into the image this code is running from.
fn installed_firmware(board: &Board) -> Option<Version> {
    let info = board.system().firmware().ok()?;

    Version::parse(&info.version)
}

/// The version of the font in `dir` — the one the boot reads, read off its file name.
fn installed_font(dir: &str) -> Option<Version> {
    card_font_in(dir).map(|font| font.version)
}

/// Whether there is a card in the slot, asked of the slot and not of the filesystem under it.
///
/// [`StorageBackend::refresh`] first and [`StorageBackend::volumes`] after, and not `volumes` alone:
/// a card is not there until something mounts it, so one put in since the last look is invisible to a
/// read that does not touch the slot — and the mount is the only way to know, because this board has
/// no card-detect line. `refresh` is bounded on the C side, so the probe a press pays for is one
/// timeout and not one per frame; the page asks once per press for exactly that reason.
///
/// A board that cannot be asked at all counts as no card, which is the same thing to the one sentence
/// this feeds: a place to put a table. The row says *why* a press did nothing, and "the slot could
/// not be read" is not one of the two reasons a person can act on.
///
/// [`StorageBackend::refresh`]: pomelo_hal::StorageBackend::refresh
/// [`StorageBackend::volumes`]: pomelo_hal::StorageBackend::volumes
fn card_mounted(board: &Board) -> bool {
    if let Err(error) = board.storage().refresh() {
        eprintln!("[updater] the slot could not be probed: {error}");
    }

    board
        .storage()
        .volumes()
        .map(|volumes| {
            volumes
                .iter()
                .any(|volume| volume.kind == VolumeKind::Removable)
        })
        .unwrap_or(false)
}

/// The font the box should be drawing with, as the card holds it.
///
/// A card can carry the coverage on its own — a table baked from the face in this image — or it can
/// carry a wider face *with* the table baked from that face, which is what a press on the page's font
/// row fetches and writes here. [`face`](Self::face) is the difference, and the version is what ties
/// the two files together: both name the same one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFont {
    /// The version the file — or both files — carries: what the page puts on the screen.
    pub version: Version,
    /// The face the table was baked from, when the card brought one. `None` means the table belongs to
    /// the subset this image was built with, and the boot installs that subset as it always did.
    pub face: Option<String>,
    /// The table itself: the file the boot reads and the renderer looks a glyph up in.
    pub table: String,
}

/// The font in [`FONT_DIR`] — the one the interface should be drawing with.
pub fn card_font() -> Option<CardFont> {
    card_font_in(FONT_DIR)
}

/// The font in `dir`, whether it arrived as a face and a table or as a table on its own.
///
/// A *complete* package wins, and that is a rule about correctness rather than about versions: a table
/// that arrived on its own is a table the manifest named *without* a face, which is a manifest saying
/// it was baked from the face this image carries — the only other face a table can belong to — and
/// handing that table to a face that came on a card would be handing it glyph numbers from another
/// font. The renderer refuses those (see `iced_pomelo_winit::fonts`), and a refused table is a box that
/// rasterises every glyph it draws at 1.6 ms a glyph. So the question this answers is "the newest font
/// that can actually be drawn with", which is what makes an older package beat a newer lone table.
///
/// A press that stopped between the two halves of a package reads as exactly that: the face of the
/// version that never finished has no table to be paired with, and is not a font this box draws with
/// until its table is there — see [`Download::next`], which is why the face is the half that goes
/// first.
///
/// The firmware's boot asks this before the first frame and reads what it returns, which is what turns
/// "the font is v3" from a claim into the thing actually being drawn: the page can say which font the
/// box *has* without being able to choose it, and this is the other half of that sentence. It is the
/// same scan as the page's, deliberately and not by accident: the file this returns is the file
/// [`read_installed`] reads the version out of, so the two can never disagree about which font is
/// installed.
///
/// A directory that does not exist, a name that is not UTF-8 and a file that carries no version are
/// each skipped rather than fatal, which is what makes this readable on a board with no card at all.
pub fn card_font_in(dir: &str) -> Option<CardFont> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut faces: Vec<(Version, String)> = Vec::new();
    let mut tables: Vec<(Version, String)> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(version) = Version::from_file_name(name) else {
            continue;
        };

        let path = entry.path().to_string_lossy().into_owned();

        if name.ends_with(FONT_SUFFIX) {
            tables.push((version, path));
        } else if name.ends_with(FACE_SUFFIX) {
            faces.push((version, path));
        }
    }

    let mut best: Option<CardFont> = None;

    // Driven by the tables and not by both halves: a face with no table beside it is not a font this
    // box can draw with — every glyph of every screen would be rasterised on the device — so it is not
    // offered as one here.
    for (version, table) in tables {
        let face = faces
            .iter()
            .find(|(held, _)| held == &version)
            .map(|(_, path)| path.clone());

        let font = CardFont { version, face, table };

        let better = match &best {
            None => true,
            Some(current) => match (font.face.is_some(), current.face.is_some()) {
                (true, false) => true,
                (false, true) => false,
                _ => font.version.is_newer_than(&current.version),
            },
        };

        if better {
            best = Some(font);
        }
    }

    best
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
    /// A download on its way. The sentence above the bar, and the only thing the page says while one is
    /// out that is not a number.
    pub(crate) downloading: &'static str,
    /// The prompt's first button when nothing is in flight behind it: put this away.
    pub(crate) close: &'static str,
    /// The prompt's first button while a download *is* in flight behind it: put this away, and stop the
    /// download with it. A different word for a different thing, though it is the same button.
    pub(crate) cancel: &'static str,
    /// The prompt's first button when the sentence is a question about a restart: *not now*. A word of
    /// its own rather than [`Words::close`], because a question is answered and a report is dismissed.
    pub(crate) later: &'static str,
    /// The prompt's second button, and the only second button on the page: reset the box, so that what
    /// has just landed is what the box is running.
    pub(crate) restart: &'static str,
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
                downloading: "正在下载…",
                close: "关闭",
                cancel: "取消",
                later: "稍后",
                restart: "立即重启",
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
                downloading: "Downloading…",
                close: "Close",
                cancel: "Cancel",
                later: "Later",
                restart: "Restart now",
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

    /// The sentence for a press this image cannot honour: why it did nothing, and what it would take.
    ///
    /// The glyph table's press has two answers and the image's has one, because the two rows are
    /// waiting on two different things. A new image is waiting on a second app slot, which is on the
    /// flash whether a card is in the box or not — so there is nothing else to say. A new table is
    /// waiting on a write that does not exist *and* on a place to put the file, and which of those a
    /// person is told is [`Words::no_card`]'s question and not this one's: a box with no card has
    /// nowhere to put a table even once the write is built, and a box with one is only ever waiting
    /// on the code.
    pub(crate) fn unsupported_line(&self, kind: Kind, card: bool) -> &'static str {
        match (kind, card) {
            (Kind::Font, false) => self.no_card,
            _ => self.unsupported,
        }
    }

    /// What a download that finished says: which version is on the box now, and what it takes to use it.
    ///
    /// The version is the row's own re-read and not a number kept from the press, so this sentence
    /// cannot say a version the card does not have — and the second half is not decoration: the
    /// interface draws from the table the *boot* loaded, so a table that just landed is one the box has
    /// and is not yet one it is using.
    pub(crate) fn installed_ok_line(&self, kind: Kind, version: &Version) -> String {
        let (name, version) = (self.name(kind), version.text());

        if self.chinese {
            format!("{name}已更新到 {version}，重启后生效。")
        } else {
            format!("{name} updated to {version} — restart to use it.")
        }
    }

    /// The sentence for a download that did not become an install.
    ///
    /// The two that are not HTTP faults are the two a request cannot have: a `404` is a request that
    /// did everything right and a file that is not there, and a short file is a request that came back
    /// whole as far as the socket is concerned. Telling them with [`Words::fault_line`] would send a
    /// person to look at the network for a file the repository does not have.
    pub(crate) fn download_fault_line(&self, fault: DownloadFault) -> &'static str {
        match (fault, self.chinese) {
            (DownloadFault::Http(fault), _) => self.fault_line(fault),
            (DownloadFault::NotFound, false) => {
                "The repository has no such file — it may not be published yet."
            }
            (DownloadFault::NotFound, true) => "仓库上没有这个文件，可能还没有发布。",
            (DownloadFault::Incomplete, false) => {
                "The file did not arrive whole, so it was not kept. Try again."
            }
            (DownloadFault::Incomplete, true) => "文件没有完整下载，已经放弃这次更新，请重试。",
        }
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
