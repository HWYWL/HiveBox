//! The server monitor — another computer's numbers, on this panel — **a standard iced program**.
//!
//! Not a program that owns a loop: [`NasMonitor`] is a state with an `update` and a `view`, and
//! `main.rs` — a handful of lines — is what runs it in a window. On the board the launcher hosts it like
//! every other app, and the app cannot tell the difference.
//!
//! # Three pages, because there is more than one machine
//!
//! The list of machines this board has been told about, a form for adding one — the address, the
//! account and the password, typed on the panel's own keyboard — and, for whichever row is picked, the
//! monitoring page itself. A board can be pointed at several boxes, so "which one am I looking at" is a
//! question the page has to be able to ask: a dashboard with no name on it is a dashboard for whichever
//! machine happened to be first in a file.
//!
//! # What the monitoring page is looking at
//!
//! A machine across the network: a server on the same LAN, watched over the board's Wi-Fi so that
//! "is the box all right" is answerable from the desk rather than from a browser on another machine.
//! Which is why that page is laid out like the server's own dashboard and not like a form: cards for
//! the things a person checks (the CPU, the memory, the network, the disks), and a table for the disks
//! themselves.
//!
//! # Nothing here knows what SSH is
//!
//! The reading is [`pomelo_hal::NasReading`] and the asking is
//! [`pomelo_hal::NasBackend`][pomelo_hal::traits::NasBackend]. What the transport is — an SSH login
//! today, a metrics daemon toasted over HTTP tomorrow — is behind that trait, along with the host key,
//! the session and the commands. That is not tidiness for its own sake: this page was *built* against a
//! simulator, on a desktop, with no NAS and no network, and the same code draws the board.
//!
//! # A look is slow, so it is started and then polled
//!
//! [`Message::Tick`] arrives on whatever clock the platform has — the launcher's one-a-second pump on
//! the board, a window's frames on a desktop — and does two different things at two different rates:
//! it *pulls* the last reading into this state every time, because that is a clone and a frame should
//! never show an answer that has already arrived, and it *asks* for a new look only every
//! [`DEFAULT_INTERVAL`], or the interval the credentials file names. Asking is not reading: the backend
//! answers when the machine at the other end does.
//!
//! # Three states, and two of them are not failures
//!
//! A NAS is a machine that sleeps, and a board that has not been told which NAS to watch is a board
//! somebody has not finished setting up. So "unconfigured", "connecting" and "the last look did not
//! arrive" are three different screens — and the last of them keeps the numbers it had, with a growing
//! age beside them, because a NAS that went to sleep did not become a NAS with no disks. See
//! [`pomelo_hal::NasStatus`] and [`pomelo_hal::NasReading::age`].
//!
//! # What is not here yet
//!
//! The rings. The server's own dashboard draws them and this page draws bars, which is a decision worth
//! naming: the settings app's rings live in *its* crate, and a second copy of that drawing code is
//! exactly what `pomelo-widgets` exists to prevent. Moving it there is a small piece of work for two
//! apps instead of one, and it is not done.
//!
//! # Where the devices are written down
//!
//! Nowhere in this crate. The list is [`NasBackend::devices`][pomelo_hal::traits::NasBackend::devices],
//! the form ends in [`NasBackend::remember`][pomelo_hal::traits::NasBackend::remember], and a row's
//! ✕ is [`NasBackend::forget`][pomelo_hal::traits::NasBackend::forget] — the backend is the side that
//! knows how to reach this platform's flash, which on the board is the C side's `stdio` and not
//! `std::fs` at all. That is also why the list is re-read after every write rather than kept in step
//! by hand: the file is the truth, and the page is a drawing of it.

mod format;
pub mod style;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, row, scrollable, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Subscription, Theme};

use pomelo_hal::{Board, NasCredentials, NasFault, NasReading, NasStatus};
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};
use pomelo_widgets::touch_keyboard::{self, KeyAction, KeyboardMode};

/// How long between looks at the machine on the monitor page, when its credentials do not say.
///
/// Five seconds: slow enough that the machine's own disks are not the thing being measured, and fast
/// enough that somebody watching a copy of a file sees the numbers move. See
/// [`NasCredentials::interval`][pomelo_hal::NasCredentials::interval], which is where a person's own
/// answer is read from.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// How long after a machine's *first* look the second one is asked for: see [`NasMonitor::tick`], which
/// is where the asking happens, and the backend's note on why a first look has no rates in it — `/proc`
/// reports totals since boot, so the CPU and the network are differences and one look cannot make one.
///
/// A second, from both sides: the backend takes its second look a second after the first
/// (`pomelo-hal-esp32`'s `nas`, which is where the session is), and this is the app asking on the same
/// beat. The two do not have to agree exactly — an ask the backend is not ready for is a flag it will
/// answer on the next pass — but a page that asked on its interval instead would sit with two blank
/// cards on it for five seconds, which is the wait this exists to remove.
const PRIME: Duration = Duration::from_secs(1);

/// Which of the app's three pages is up.
///
/// Three and not one, because a board can be pointed at more than one machine and "which one am I
/// looking at" is the question the first two pages exist to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The machines this board has been told about, and the button that adds another.
    List,
    /// One machine's numbers: the overview cards and the disk table.
    Monitor,
    /// The address, the account and the password of a machine being added or corrected.
    Form,
}

/// Which half of the monitoring page is up.
///
/// Two and not one, because the two things a person wants are read differently: the cards are looked
/// *at* (a glance, from across a room) and the disk table is read (a row at a time, sitting down). On a
/// 480 px panel trying to do both at once would give neither enough room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Disks,
}

/// Which of the form's fields the keyboard is typing into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Host,
    User,
    Password,
}

/// What is wrong with the form, once somebody has pressed save.
///
/// A list of the two things the form can be missing rather than a sentence: which sentence to draw is
/// the view's business, and it depends on the language like every other word on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wrong {
    /// No address to reach.
    Host,
    /// No account to log in as.
    User,
}

/// What the monitor reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// Time has passed: `now` is when the platform said so. See the crate documentation for what this
    /// does at which rate.
    Tick(Instant),
    /// A finger on one of the monitoring page's two tabs.
    Show(Tab),
    /// A row of the list was picked, by its place in the list.
    ///
    /// A place and not a host: the row is a `button` built in the same frame the finger lands in, so the
    /// list cannot have moved under it — and what a tap does is read this app's own state.
    Open(usize),
    /// The list's "add a machine" button.
    Add,
    /// A row's edit button: the form, filled in with the machine that row names.
    ///
    /// A place and not a host, for the reason [`Message::Open`] is one: the row is built in the same
    /// frame the finger lands in, so the index is read from this app's own state and nothing else.
    EditDevice(usize),
    /// A row's delete button: ask about dropping that machine, by its place in the list.
    ///
    /// A question and not the act. The row sits in a list somebody is scrolling with a thumb, and what
    /// it would drop is an address, an account and a password that nobody has written down anywhere else
    /// — so the row asks and [`Message::ConfirmDelete`] is what answers. The files app's `Prompt` is the
    /// same trade made the same way.
    DeleteDevice(usize),
    /// The confirmation's yes: the machine the row asked about goes, and the page stays on the list.
    ConfirmDelete,
    /// The confirmation's no, and the way out of a question nobody meant to ask.
    CancelDelete,
    /// The monitoring page's terminal button: a shell on the machine this page is about.
    ///
    /// Handled by *nothing here* — see the arm in [`NasMonitor::update`]. Opening an app is the
    /// launcher's business and this page is a widget inside it, so all the tap can do is say that
    /// somebody asked, and the launcher is what hears it. A standalone monitor (`nas-monitor`'s own
    /// `main`) has no launcher to hear it, which is why the entry does nothing at all there.
    Terminal,
    /// A finger on one of the form's fields.
    Focus(Field),
    /// A key of the form's keyboard.
    Key(KeyAction),
    /// Whether the form draws the password in the clear.
    Reveal(bool),
    /// The form's save button, or the keyboard's return key.
    Save,
    /// The form's delete button: drop the machine being corrected from the list.
    Delete,
    /// Back to the list: the form's cancel button, and the arrow on the monitoring page.
    Back,
}

/// The server monitor.
pub struct NasMonitor {
    board: Arc<Board>,
    /// The machines the board has been told about, as the list page draws them. Re-read from the
    /// backend after every write rather than kept in step by hand: the file is the truth and the page
    /// is a drawing of it.
    devices: Vec<NasCredentials>,
    /// The last reading of each machine this app has watched, by address — what a row of the list says
    /// under its name.
    ///
    /// This app's own memory rather than the backend's: the backend watches one machine at a time, and
    /// a row nobody has opened has nothing to show rather than a number from another machine's session.
    seen: BTreeMap<String, NasReading>,
    /// The last reading of the machine the monitor page is on, which is the backend's snapshot as of
    /// the last tick.
    ///
    /// Held rather than asked for in `view`, for the reason the task viewer holds its reading: a frame
    /// costs a lock and a clone either way, and a state that is read once per tick is a state the tests
    /// can drive by hand.
    reading: NasReading,
    /// The machine the monitor page is on, if it is on one.
    ///
    /// Read for the name in its header and for the interval between looks; whether the board can watch
    /// a machine *at all* is not this field's business — that arrives as [`NasFault::Unsupported`] in
    /// the reading, because it is the backend that knows.
    watched: Option<NasCredentials>,
    /// Which of the three pages is up.
    screen: Screen,
    /// The machine a row has asked about dropping, that nobody has answered yet — by address.
    ///
    /// By address and not by place, because the question outlives the frame it was asked in: the rows are
    /// re-drawn from the list on every tick, and the question has to stay about the machine somebody
    /// pointed at rather than about whatever is in that row by the time the answer comes. See
    /// [`Message::DeleteDevice`].
    confirming: Option<String>,
    /// What has been typed into the form.
    form: Form,
    tab: Tab,
    /// When the board was last asked to look, for the cadence in [`NasMonitor::tick`].
    asked: Option<Instant>,
    preferences: SystemPreferences,
}

/// What the form holds, and what it is for.
///
/// One struct for adding a machine and for correcting one, because they are the same three fields and
/// the same button: what [`Form::editing`] changes is whether the address is one the list already has —
/// which is what makes saving an *update* rather than an addition, and what the delete button is
/// offered on.
struct Form {
    /// The address the form was opened on, when it was opened on a machine already in the list.
    editing: Option<String>,
    host: String,
    user: String,
    password: String,
    /// The host key written down for this machine, carried through a correction rather than dropped:
    /// it is not one of the form's fields, but throwing it away would be this page editing a file it
    /// does not understand. See `nas_credentials`.
    host_key: Option<String>,
    /// Which of the three fields the keyboard is typing into.
    focus: Field,
    /// Which page of the keyboard is up.
    keyboard: KeyboardMode,
    /// Whether the password is drawn in the clear. A password typed on a touch keyboard is a password
    /// typed with a thumb, and being able to read it back is what keeps it from being typed twice.
    revealed: bool,
    /// What was wrong with it, the last time somebody pressed save.
    wrong: Option<Wrong>,
}

impl Form {
    /// An empty form: a machine being added.
    fn adding() -> Self {
        Self {
            editing: None,
            host: String::new(),
            user: String::new(),
            password: String::new(),
            host_key: None,
            focus: Field::Host,
            keyboard: KeyboardMode::Lower,
            revealed: false,
            wrong: None,
        }
    }

    /// The form filled in with `device`: a machine being corrected.
    fn of(device: &NasCredentials) -> Self {
        Self {
            editing: Some(device.host.clone()),
            host: device.host.clone(),
            user: device.user.clone(),
            password: device.password.clone(),
            host_key: device.host_key.clone(),
            ..Self::adding()
        }
    }

    /// What the fields say, if they say enough to log in with.
    ///
    /// An address and an account, and nothing else: the port is 22 and the interval is five unless
    /// somebody edits the file by hand, which is the pair of defaults `nas_credentials` documents. The
    /// password may be empty, because an account with no password is a thing a box can have and
    /// refusing it here would be this page disagreeing with `ssh`.
    fn device(&self) -> Result<NasCredentials, Wrong> {
        let host = self.host.trim();
        let user = self.user.trim();

        if host.is_empty() {
            return Err(Wrong::Host);
        }
        if user.is_empty() {
            return Err(Wrong::User);
        }

        Ok(NasCredentials {
            host: host.to_string(),
            port: 22,
            user: user.to_string(),
            password: self.password.clone(),
            interval_secs: 5,
            host_key: self.host_key.clone(),
        })
    }

    /// The field the keyboard is typing into, as the string it types onto.
    fn typed(&mut self) -> &mut String {
        match self.focus {
            Field::Host => &mut self.host,
            Field::User => &mut self.user,
            Field::Password => &mut self.password,
        }
    }

    /// A key of the form's keyboard.
    ///
    /// The return key is not here: what it means is "save", and saving is the app's — this struct
    /// cannot reach a backend. See [`NasMonitor::update`].
    fn key(&mut self, action: KeyAction) {
        match action {
            KeyAction::Char(character) => self.typed().push(character),
            KeyAction::Space => self.typed().push(' '),
            KeyAction::Backspace => {
                self.typed().pop();
            }
            KeyAction::SwitchMode(mode) => self.keyboard = mode,
            KeyAction::Enter => {}
        }

        // Typing again is somebody answering what the last save complained about.
        self.wrong = None;
    }
}

impl NasMonitor {
    /// The monitor, on the list of machines the board has been told about.
    pub fn new(board: Arc<Board>) -> Self {
        let devices = board.nas().devices();

        let mut monitor = Self {
            board,
            devices,
            seen: BTreeMap::new(),
            reading: NasReading::default(),
            watched: None,
            screen: Screen::List,
            confirming: None,
            form: Form::adding(),
            tab: Tab::Overview,
            // No look is asked for here: the page opens on a list, and a machine nobody has picked is a
            // machine nobody has asked about. The clock starts when a row is picked — see
            // [`NasMonitor::open`].
            asked: Some(Instant::now()),
            preferences: SystemPreferences::default(),
        };

        monitor.pull();

        monitor
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The last reading, as the monitoring page draws it. Read by the tests and by a host; a page gets
    /// it through [`NasMonitor::view`].
    pub fn reading(&self) -> &NasReading {
        &self.reading
    }

    /// The machines the board has been told about, as the list draws them.
    pub fn devices(&self) -> &[NasCredentials] {
        &self.devices
    }

    /// The machine a row has asked about dropping, if one has been asked about and not answered.
    ///
    /// Read by the list, which draws the question instead of the rows while it is up, and by the tests,
    /// which is what a state with no other reader is for.
    pub fn confirming(&self) -> Option<&str> {
        self.confirming.as_deref()
    }

    /// Which of the three pages is up.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The last reading this app has seen of `host`, if it has seen one. What a row of the list draws
    /// under its name.
    pub fn seen(&self, host: &str) -> Option<&NasReading> {
        self.seen.get(host)
    }

    /// The machine the monitoring page is on, if it is on one.
    pub fn watched(&self) -> Option<&NasCredentials> {
        self.watched.as_ref()
    }

    /// Which half of the monitoring page is up.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// How long between looks: what the credentials say, or [`DEFAULT_INTERVAL`].
    pub fn interval(&self) -> Duration {
        self.watched
            .as_ref()
            .map_or(DEFAULT_INTERVAL, NasCredentials::interval)
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.tick(now),
            Message::Show(tab) => self.tab = tab,
            Message::Open(index) => self.open(index),
            Message::Add => {
                self.form = Form::adding();
                self.screen = Screen::Form;
            }
            // A row's pencil, which is the same form the header's pencil opens: one machine, three
            // fields. What differs is where the machine comes from — the row it was pressed on rather
            // than the one the monitoring page happens to be on.
            Message::EditDevice(index) => {
                if let Some(device) = self.devices.get(index).cloned() {
                    self.form = Form::of(&device);
                    self.screen = Screen::Form;
                }
            }
            // The row asks the question; [`Message::ConfirmDelete`] is what acts on it. See
            // [`Message::DeleteDevice`] for why the two are apart.
            Message::DeleteDevice(index) => {
                self.confirming = self.devices.get(index).map(|device| device.host.clone());
            }
            Message::ConfirmDelete => {
                if let Some(host) = self.confirming.take() {
                    self.forget(&host);
                }
            }
            Message::CancelDelete => self.confirming = None,
            // The terminal entry: the page asks and the launcher opens. See [`Message::Terminal`] for
            // why the act is not here — a page cannot change which app is on screen — and for what a
            // standalone monitor does instead, which is nothing at all.
            Message::Terminal => {}
            Message::Focus(field) => self.form.focus = field,
            Message::Key(action) => {
                // The keyboard's return key saves, like the button under it — and saving is the app's,
                // because the file is the backend's. See [`Form::key`], which is why this is here.
                if action == KeyAction::Enter {
                    self.save();
                } else {
                    self.form.key(action);
                }
            }
            Message::Reveal(revealed) => self.form.revealed = revealed,
            Message::Save => self.save(),
            Message::Delete => self.delete(),
            Message::Back => self.cancel(),
        }
    }

    /// One frame's worth of clock: pull what has arrived, and ask for a new look when it is time.
    ///
    /// Two rates in one message, because the platform only sends one: the *pull* is every tick, so that
    /// an answer that arrived between two ticks is on screen at the next one, and the *ask* is on the
    /// interval — and only while the monitoring page is up. A look is a network round trip to another
    /// machine, and a page nobody is on has no business opening sessions to it.
    ///
    /// The one thing asked for off the interval is a reading with no CPU in it, which is what a machine's
    /// first look comes back with and what [`PRIME`] is for: the card would otherwise be a dash until the
    /// next look five seconds later, and the machine it is about is answering the whole time. See
    /// [`PRIME`], and the backend's note on the second look it takes for the same reason.
    pub fn tick(&mut self, now: Instant) {
        self.pull();

        // On the list the same tick has a second job: keeping the rows' statuses current. That is a check
        // — a socket per machine, not a session — and it is asked for by the page that draws the answers,
        // which is what stops it: leaving this screen stops the ticks, and the checks with them.
        if self.screen == Screen::List {
            let _ = self.board.nas().check();
        }

        if self.screen != Screen::Monitor {
            return;
        }

        // What this app has to show decides how soon to ask again: the interval once there is a reading
        // with rates in it, the priming beat while there is not. Asking for a look the backend is not
        // ready to take costs nothing — it is a flag and a wake, and the look happens when it is due.
        let spacing = if self.reading.cpu.is_some() {
            self.interval()
        } else {
            PRIME
        };

        let due = match self.asked {
            Some(asked) => now.duration_since(asked) >= spacing,
            None => true,
        };

        if due {
            self.asked = Some(now);
            let _ = self.board.nas().refresh();
        }
    }

    /// Frames, which is the only clock a window has.
    ///
    /// On the board this is never asked for: the launcher merges the subscriptions of the apps it is
    /// actually running, and its own pump is what sends [`Message::Tick`] here. A window has no such
    /// pump, so the app takes the frames it is offered and throttles them itself.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::window::frames().map(Message::Tick)
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
        let theme = self.preferences.theme;
        let sizes = style::Sizes::new();
        let ink = style::ink(theme);
        let muted = style::muted(theme);
        let words = Words::of(self.preferences.language);

        let page: Element<'_, Message> = match self.screen {
            Screen::List => self.list(sizes, theme, ink, muted, &words),
            Screen::Form => self.form(sizes, theme, ink, muted, &words),
            Screen::Monitor => self.monitor(sizes, theme, ink, muted, &words),
        };

        container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(style::surface(theme).into()),
                ..container::Style::default()
            })
            .into()
    }

    /// One machine's page: its header, and whichever of its two halves is up.
    ///
    /// The notice states come before the tabs rather than inside them, because a board that cannot
    /// watch anything has no halves to switch between — see the crate documentation on the three states
    /// that are not failures.
    fn monitor(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
        words: &Words,
    ) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.reading.status {
            // The board cannot do this at all. Not "the machine is down" and not "nothing is set up":
            // nothing a person does to the machine changes this screen, so it says what would.
            NasStatus::Offline(NasFault::Unsupported) => notice(&words.unsupported, sizes, muted),
            // Nothing to watch, which is a board somebody has not finished setting up.
            NasStatus::Unconfigured => notice(&words.unconfigured, sizes, muted),
            _ => match self.tab {
                Tab::Overview => self.overview(sizes, theme, ink, muted, words),
                Tab::Disks => self.disks(sizes, ink, muted, words),
            },
        };

        column![self.header(sizes, theme, ink, muted, words), body]
            .spacing(style::GAP)
            .padding(style::MARGIN)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The line at the top: which machine, how it is answering, and how old the numbers are.
    ///
    /// The age is in the header rather than beside each card because it is one fact about the whole
    /// reading — every number on the page is from the same look — and it is the fact that decides
    /// whether any of them should be believed. It turns amber-red once it is older than the interval,
    /// which is the moment the numbers stopped describing now.
    fn header(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
        words: &Words,
    ) -> Element<'_, Message> {
        let status = &self.reading.status;
        let inks = style::status_ink(theme);

        let dot = match status {
            NasStatus::Online => inks.online(),
            NasStatus::Connecting => inks.waiting(),
            NasStatus::Offline(_) => inks.offline(),
            NasStatus::Unconfigured => muted,
        };

        // What the reader is told, in the order they need it: why it is not online, or how old the
        // numbers are. A board that cannot do this at all has already been told what is wrong, so the
        // header's job there is only to name the machine.
        let note = match status {
            NasStatus::Online => self
                .reading
                .age()
                .map(|age| format::age(age, words.language))
                .unwrap_or_default(),
            NasStatus::Connecting => String::from(words.connecting),
            NasStatus::Offline(fault) => String::from(words.fault(*fault)),
            NasStatus::Unconfigured => String::new(),
        };

        let stale = self
            .reading
            .age()
            .is_some_and(|age| age > self.interval() * 2);

        let name = self
            .watched
            .as_ref()
            .map(|credentials| credentials.host.clone())
            .unwrap_or_else(|| String::from(words.server));

        let uptime = self
            .reading
            .uptime
            .map(|uptime| format::uptime(uptime, words.language))
            .unwrap_or_default();

        let identity = column![
            row![
                text(Icon::DNS.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.text)
                    .color(muted),
                Space::new().width(Length::Fixed(style::CELL_PAD * 0.5)),
                text(name).size(sizes.text).color(ink),
            ]
            .align_y(Alignment::Center),
            row![
                text(Icon::CIRCLE.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.small * 0.7)
                    .color(dot),
                Space::new().width(Length::Fixed(style::CELL_PAD * 0.5)),
                text(note).size(sizes.small).color(inks.age(stale)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(2.0);

        // The way back to the list comes first on the row: a page about one machine has to be able to
        // say *which* machine and to offer a way out of it, and the arrow is the way out.
        let mut line = Row::with_children(vec![
            icon_button(Message::Back, Icon::ARROW_BACK, sizes, ink, muted),
            Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
            identity.into(),
        ])
        .width(Length::Fill)
        .align_y(Alignment::Center);

        if !uptime.is_empty() {
            line = line.push(Space::new().width(Length::Fill));
            line = line.push(
                column![
                    text(words.uptime).size(sizes.small).color(muted),
                    text(uptime).size(sizes.text).color(ink),
                ]
                .spacing(1.0),
            );
        }

        // The two halves on the left and the one thing a person does *to* the machine the page is about
        // on the right: a shell on it. Not a half of the page, which is why it sits past the fill rather
        // than with the tabs. Correcting the machine used to be here as well, as a bare pencil — it is
        // the list row's now, where it can be reached without opening a session with a machine whose
        // credentials may be the very thing that is wrong. See [`Message::EditDevice`].
        let tabs = Row::with_children(vec![
            self.tab_button(Tab::Overview, words.overview, sizes, theme, ink, muted),
            self.tab_button(Tab::Disks, words.disks, sizes, theme, ink, muted),
            Space::new().width(Length::Fill).into(),
            terminal_button(Message::Terminal, words.terminal, sizes, theme, ink),
        ])
        .spacing(style::CELL_PAD)
        .align_y(Alignment::Center);

        // Inset past [`style::MARGIN`] so the name and the uptime clear the panel's rounded corners:
        // this is the topmost row, and it is the one that meets them. See [`style::HEADER_INSET`].
        column![line, tabs]
            .spacing(style::GAP * 0.6)
            .padding([0.0, style::HEADER_INSET - style::MARGIN])
            .into()
    }

    /// The page that says which machines there are.
    ///
    /// A list and not a picker: it is the app's own front door, so it is where machines are added to it
    /// — and, because a row says what this session last saw of the machine it names, it is also the page
    /// that answers "are both boxes all right" without opening either. See [`NasMonitor::seen`].
    fn list(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
        words: &Words,
    ) -> Element<'_, Message> {
        let header = column![
            text(words.title).size(sizes.headline).color(ink),
            text(format::machines(self.devices.len(), words.language))
                .size(sizes.small)
                .color(muted),
        ]
        .spacing(1.0)
        // Inset like every other topmost row: see [`style::HEADER_INSET`].
        .padding([0.0, style::HEADER_INSET - style::MARGIN]);

        let body: Element<'_, Message> = if self.devices.is_empty() {
            // Nothing told to it yet, which is a board somebody has not finished setting up rather than
            // a failure — the same thing the monitoring page says about itself when it has no machine.
            container(text(words.no_devices).size(sizes.text).color(muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .into()
        } else {
            let mut rows = Column::new()
                .spacing(style::CELL_PAD)
                .width(Length::Fill);

            for (index, device) in self.devices.iter().enumerate() {
                rows = rows.push(self.device_row(index, device, sizes, theme, ink, muted, words));
            }

            scrollable(rows).height(Length::Fill).into()
        };

        // What a row's bin leaves behind: the question, in the place the "add a machine" button holds.
        // Over the list rather than instead of it, so the row it is about stays on screen while it is
        // asked — the address in the question is one to check against the row it names.
        let footer: Element<'_, Message> = match &self.confirming {
            Some(host) => confirm_bar(host, sizes, theme, ink, words),
            None => add_button(sizes, theme, ink, words),
        };

        column![header, body, footer]
            .spacing(style::GAP)
            .padding(style::MARGIN)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// One machine in the list: where it is, who logs in, and whatever this app last saw of it.
    fn device_row(
        &self,
        index: usize,
        device: &NasCredentials,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
        words: &Words,
    ) -> Element<'_, Message> {
        let inks = style::status_ink(theme);

        // What is known about it, from the two things that know anything: a *look* — which this session
        // has made at the machines somebody has opened, and which says whether the session worked and why
        // not — and a *check*, which every machine gets while this list is up and which says only whether
        // the board can reach its port. Which of the two answers is [`device_status`]'s business.
        let known = self.seen(&device.host).map(|reading| &reading.status);
        let reached = self.board.nas().answering(&device.host);

        let (dot, note) = device_status(known, reached, inks, muted, words);

        // The row is three buttons and not one, which is the shape this grew into: the identity half
        // opens the page about the machine, and the pencil and the bin beside it correct and drop it
        // without opening anything. The half that opens it is a button of its own rather than the card
        // around all three, because a button inside a button is a press whose owner is whichever one the
        // platform walks to first — so the card is a plain container and each action is its own button.
        let identity = button(
            Row::with_children(vec![
                text(Icon::DNS.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.text)
                    .color(muted)
                    .into(),
                Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
                Column::with_children(vec![
                    text(device.host.clone()).size(sizes.text).color(ink).into(),
                    // The word is drawn in the dot's ink: green for a machine that answered, red for one
                    // that did not. One decision said twice, which is why the two are picked together.
                    text(format!("{} · {note}", device.user))
                        .size(sizes.small)
                        .color(dot)
                        .into(),
                ])
                .spacing(1.0)
                .width(Length::Fill)
                .into(),
                Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
                text(Icon::CIRCLE.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.small * 0.7)
                    .color(dot)
                    .into(),
            ])
            .align_y(Alignment::Center)
            .width(Length::Fill),
        )
        .padding(style::CELL_PAD)
        .width(Length::Fill)
        .on_press(Message::Open(index))
        .style(move |_theme, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Pressed) {
                    style::track(theme)
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            border: Border {
                radius: sizes.small.into(),
                ..Border::default()
            },
            ..button::Style::default()
        });

        // The two ways to correct the machine, to the right of the row and clear of the half that opens
        // it. Both are words on chips and not bare glyphs: see [`row_action`], where the arithmetic is.
        let danger = style::status_ink(theme).offline();

        container(
            Row::with_children(vec![
                identity.into(),
                row_action(words.edit, Message::EditDevice(index), sizes, theme, ink),
                row_action(words.delete, Message::DeleteDevice(index), sizes, theme, danger),
            ])
            .spacing(style::CELL_PAD * 0.5)
            .align_y(Alignment::Center)
            .width(Length::Fill),
        )
        .padding(style::CELL_PAD * 0.5)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(style::card(theme).into()),
            border: Border {
                radius: sizes.small.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
    }

    /// The page that writes a machine down: three fields, the panel's own keyboard, and the line a
    /// mistake appears on.
    fn form(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
        words: &Words,
    ) -> Element<'_, Message> {
        let fields = Column::with_children(vec![
            self.field(Field::Host, words.host, false, sizes, theme, ink, muted),
            self.field(Field::User, words.account, false, sizes, theme, ink, muted),
            self.field(Field::Password, words.password, true, sizes, theme, ink, muted),
        ])
        .spacing(style::CELL_PAD)
        .width(Length::Fill);

        // The line a mistake appears on, and it keeps its place whether there is one or not: a page
        // whose fields jump up and down as somebody types is a page that moves under the finger doing
        // the typing.
        let wrong: Element<'_, Message> = text(match self.form.wrong {
            Some(wrong) => words.wrong(wrong),
            None => "",
        })
        .size(sizes.small)
        .color(style::status_ink(theme).offline())
        .into();

        let buttons = Row::with_children(vec![
            form_button(words.save, Message::Save, true, sizes, theme, ink),
            Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
            form_button(words.cancel, Message::Back, false, sizes, theme, ink),
            Space::new().width(Length::Fill).into(),
            self.delete_button(sizes, theme, words),
        ])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let title = if self.form.editing.is_some() {
            words.edit_device
        } else {
            words.add
        };

        column![
            container(text(title).size(sizes.headline).color(ink))
                .padding([0.0, style::HEADER_INSET - style::MARGIN]),
            // The rail is not drawn, which is the other half of what [`style::SCROLLBAR`] is about: iced
            // paints the bar *over* its content's right edge, so a field that runs the full width of the
            // page has its last ten pixels — its border, its corner — covered by a strip of the page's
            // own colour, and reads as a box whose right side has gone off the panel. The task viewer
            // keeps that strip clear because a table has a last column to stop short of it; a field has
            // only its own edge, and there is nowhere to move it to. The wifi map's cards, which are
            // full-width the same way, made the same trade and said so.
            //
            // What is given up is the bar's hint that there is more below — the fields are taller than
            // the room left over the keyboard. What is not given up is the dragging: the strip still
            // scrolls under a finger. See [`NasMonitor::form`] for the arithmetic.
            scrollable(fields)
                .direction(Direction::Vertical(Scrollbar::hidden()))
                .height(Length::Fill),
            wrong,
            buttons,
            container(touch_keyboard::band_with_theme(
                self.form.keyboard,
                Message::Key,
                theme,
            ))
            .height(Length::Fixed(touch_keyboard::band_height(style::PANEL)))
            .width(Length::Fill),
        ]
        .spacing(style::GAP)
        .padding(style::MARGIN)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// One field of the form: what it is, and what has been typed into it.
    fn field(
        &self,
        field: Field,
        label: &str,
        secret: bool,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
    ) -> Element<'_, Message> {
        let focused = self.form.focus == field;

        let typed = match field {
            Field::Host => &self.form.host,
            Field::User => &self.form.user,
            Field::Password => &self.form.password,
        };

        // A password is dots, and the eye beside it turns them back into letters: a password typed on a
        // touch keyboard is typed with a thumb, and being able to read it back is what keeps it from
        // being typed twice. See [`Form::revealed`].
        let value: Element<'_, Message> = if secret && !self.form.revealed {
            dots(typed.chars().count(), sizes, ink)
        } else {
            text(typed.clone()).size(sizes.text).color(ink).into()
        };

        let mut contents = Row::with_children(vec![
            container(text(label.to_string()).size(sizes.small).color(muted))
                .width(Length::Fixed(style::FIELD_LABEL))
                .into(),
            value,
        ])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        if secret {
            contents = contents.push(Space::new().width(Length::Fill));
            contents = contents.push(icon_button(
                Message::Reveal(!self.form.revealed),
                if self.form.revealed {
                    Icon::VISIBILITY_OFF
                } else {
                    Icon::VISIBILITY
                },
                sizes,
                ink,
                muted,
            ));
        }

        button(contents)
            .padding(style::CELL_PAD * 1.5)
            .width(Length::Fill)
            .on_press(Message::Focus(field))
            .style(move |_theme, _status| button::Style {
                background: Some(style::card(theme).into()),
                border: Border {
                    radius: sizes.small.into(),
                    // The field the keyboard is typing into is the one with a line around it. A caret
                    // would say the same thing, and this says it without a clock to blink against.
                    width: if focused { 1.5 } else { 0.0 },
                    color: if focused {
                        style::network()
                    } else {
                        Color::TRANSPARENT
                    },
                },
                ..button::Style::default()
            })
            .into()
    }

    /// The form's delete button, on a machine that is already in the list and on nothing else.
    ///
    /// Off a form opened to *add* a machine there is nothing to drop, so the button is not drawn rather
    /// than drawn and refused — and it sits at the far end of the row from save, which is where a
    /// destructive button belongs.
    fn delete_button(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'static, Message> {
        if self.form.editing.is_none() {
            return Space::new().into();
        }

        let danger = style::status_ink(theme).offline();

        button(text(words.delete.to_string()).size(sizes.text))
            .padding([style::CELL_PAD * 1.2, style::CELL_PAD * 2.5])
            .on_press(Message::Delete)
            .style(move |_theme, status| {
                let pressed = matches!(status, button::Status::Pressed);

                button::Style {
                    background: Some(if pressed { danger } else { style::card(theme) }.into()),
                    text_color: if pressed { Color::WHITE } else { danger },
                    border: Border {
                        radius: sizes.small.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .into()
    }

    /// One of the two tabs: the page's own name on a button that says whether it is the one showing.
    ///
    /// A `button` and not the `container` this used to be, which is the whole of why the disks half of
    /// this page did nothing when it was pressed: a container has no press to give, so the message that
    /// switches the tab was never sent and the page sat on its overview for ever.
    fn tab_button(
        &self,
        tab: Tab,
        label: &str,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
    ) -> Element<'static, Message> {
        let showing = self.tab == tab;
        let accent = style::network();

        button(text(label.to_string()).size(sizes.small))
            .padding([style::CELL_PAD * 0.6, style::CELL_PAD * 2.0])
            .on_press(Message::Show(tab))
            .style(move |_theme, status| {
                // Only a finger on it. `Hovered` is not a press: this platform leaves the pointer where
                // the finger lifted, so a button that was *touched* stays hovered until something else
                // moves the pointer. See the settings app's `is_pressed`, which paid for this once.
                let pressed = matches!(status, button::Status::Pressed);

                button::Style {
                    background: Some(
                        match (showing, pressed) {
                            (true, _) => accent,
                            (false, true) => style::track(theme),
                            (false, false) => Color::TRANSPARENT,
                        }
                        .into(),
                    ),
                    text_color: if showing { ink } else { muted },
                    border: iced::Border {
                        radius: (sizes.small * 0.8).into(),
                        ..iced::Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .into()
    }

    /// The cards: what a person checks from across the room.
    fn overview(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
        words: &Words,
    ) -> Element<'_, Message> {
        let cpu = self.reading.cpu;
        let memory = self.reading.memory;
        let network = self.reading.network;

        // A first reading has no interval behind it, so there is no usage to report — which is why the
        // CPU card can be a dash rather than a zero. See `NasCpu::usage_percent`.
        let cpu_card = card(
            words.cpu,
            cpu.map(|cpu| format!("{:.0}%", cpu.usage_percent))
                .unwrap_or_else(|| format::NOTHING.to_string()),
            cpu.and_then(|cpu| cpu.temperature_c)
                .map(|c| format!("{} {}", words.temperature, format::temperature(c)))
                .unwrap_or_default(),
            cpu.map(|cpu| cpu.usage_percent / 100.0),
            style::cpu(),
            sizes,
            theme,
            ink,
            muted,
        );

        let memory_card = card(
            words.memory,
            memory
                .map(|memory| format!("{:.0}%", memory.used_percent()))
                .unwrap_or_else(|| format::NOTHING.to_string()),
            memory
                .map(|memory| {
                    format!(
                        "{} / {}",
                        format::bytes(memory.used_bytes()),
                        format::bytes(memory.total_bytes)
                    )
                })
                .unwrap_or_default(),
            memory.map(|memory| memory.used_percent() / 100.0),
            style::memory(),
            sizes,
            theme,
            ink,
            muted,
        );

        let disks = &self.reading.disks;

        let read: u64 = disks.iter().map(|disk| disk.read_per_sec).sum();
        let written: u64 = disks.iter().map(|disk| disk.write_per_sec).sum();

        let network_card = card(
            words.network,
            network
                .map(|network| format::rate(network.received_per_sec))
                .unwrap_or_else(|| format::NOTHING.to_string()),
            network
                .map(|network| {
                    format!(
                        "{} {}   {} {}",
                        words.received,
                        format::rate(network.received_per_sec),
                        words.sent,
                        format::rate(network.sent_per_sec)
                    )
                })
                .unwrap_or_default(),
            None,
            style::network(),
            sizes,
            theme,
            ink,
            muted,
        );

        // The headline is what the disks are doing altogether, and the line under it splits the two
        // directions. The read rate alone would not do as the headline: on a machine whose page cache
        // answers every read — three gigabytes of one on the NAS this was written against — `/proc`'s
        // read counter can sit still for hours, and a card titled 硬盘读写 whose big number never moves
        // reads as a card that is broken. The network card's headline is its incoming half because a
        // network has one that matters more; a disk has no such half, so this one's is the sum of both.
        let disk_card = card(
            words.disks_speed,
            format::rate(read + written),
            format!(
                "{} {}   {} {}",
                words.read,
                format::rate(read),
                words.written,
                format::rate(written)
            ),
            None,
            style::disk(),
            sizes,
            theme,
            ink,
            muted,
        );

        // The two figures about the disks *as a whole*: the room they have between them, and the one of
        // them that is working. Both were in the reading and neither was drawn — this half of the page
        // had a table of capacities on it and no summary of them.
        let capacity: u64 = disks.iter().filter_map(|disk| disk.size_bytes).sum();

        let storage_card = card(
            words.storage,
            if disks.is_empty() {
                format::NOTHING.to_string()
            } else {
                format::bytes(capacity)
            },
            if disks.is_empty() {
                String::new()
            } else {
                format::disks(disks.len(), words.language)
            },
            None,
            style::storage(),
            sizes,
            theme,
            ink,
            muted,
        );

        let busiest = disks
            .iter()
            .max_by(|a, b| a.busy_percent.total_cmp(&b.busy_percent));

        let busy_card = card(
            words.disk_busy,
            busiest
                .map(|disk| format!("{:.0}%", disk.busy_percent))
                .unwrap_or_else(|| format::NOTHING.to_string()),
            busiest.map(|disk| disk.device.clone()).unwrap_or_default(),
            busiest.map(|disk| disk.busy_percent / 100.0),
            style::busy(),
            sizes,
            theme,
            ink,
            muted,
        );

        // Six cards and not four. Two rows of two gave each card half the page and it drew one line of
        // text at the top of it, which is the empty half the page used to be; three rows of two on the
        // same panel is the same screen holding the four a person checks *and* the two that answer the
        // next question they ask — how much room the disks have, and which of them is the busy one.
        Column::with_children(vec![
            Row::with_children(vec![cpu_card, memory_card])
                .spacing(style::GAP)
                .into(),
            Row::with_children(vec![network_card, disk_card])
                .spacing(style::GAP)
                .into(),
            Row::with_children(vec![storage_card, busy_card])
                .spacing(style::GAP)
                .into(),
        ])
        .spacing(style::GAP)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The disks: the table, which is the half of the NAS's dashboard that has to be read a row at a
    /// time.
    fn disks(&self, sizes: style::Sizes, ink: Color, muted: Color, words: &Words) -> Element<'_, Message> {
        let mut rows = Column::new()
            .spacing(style::CELL_PAD * 0.5)
            .width(Length::Fill);

        rows = rows.push(table_row(
            words
                .heads
                .iter()
                .map(|head| cell(head, sizes.small, muted))
                .collect(),
        ));

        // The room the identity column has, which both of its lines are cut to: see [`identity_room`]
        // for the arithmetic and [`format::fit`] for the cutting. Read once for the whole table, the
        // column being the same width on every row.
        let room = identity_room(sizes);

        for disk in &self.reading.disks {
            // The identity cell is two lines: the NAS's own word for what the disk is for, and under it
            // the kernel's name and the model. The model is what makes this cell wide, and a column of
            // its own would take the width of every other column put together — so it goes *under* the
            // use rather than beside it, which is what the eye reads anyway.
            let identity: Element<'_, Message> = Column::with_children(vec![
                cell(&format::fit(&disk.purpose, room, sizes.text), sizes.text, ink),
                cell(
                    &identity_detail(&disk.device, &disk.model, room, sizes.small),
                    sizes.small,
                    muted,
                ),
            ])
            .into();

            rows = rows.push(table_row(vec![
                identity,
                    cell(
                        &disk
                            .size_bytes
                            .map(format::bytes)
                            .unwrap_or_else(|| format::NOTHING.to_string()),
                        sizes.text,
                        ink,
                    ),
                    cell(
                        &disk
                            .temperature_c
                            .map(format::temperature)
                            .unwrap_or_else(|| format::NOTHING.to_string()),
                        sizes.text,
                        ink,
                    ),
                    cell(&format!("{:.0}%", disk.busy_percent), sizes.text, ink),
                cell(&format::rate(disk.read_per_sec), sizes.text, muted),
                cell(&format::rate(disk.write_per_sec), sizes.text, muted),
            ]));
        }

        scrollable(container(rows).padding(iced::Padding {
            right: style::SCROLLBAR,
            ..iced::Padding::ZERO
        }))
        .height(Length::Fill)
        .into()
    }

    /// Picks `index` of the list: the board is pointed at that machine, and its page goes up.
    fn open(&mut self, index: usize) {
        let Some(device) = self.devices.get(index).cloned() else {
            return;
        };

        // Talking the backend into watching it *is* how this app points anything anywhere: an address
        // the list already has is corrected rather than added a second time, so this is the same call
        // the form makes. See the trait's note on who touches the file.
        let _ = self.board.nas().remember(&device);

        self.watched = Some(device);
        self.reading = NasReading::default();
        self.tab = Tab::Overview;
        self.screen = Screen::Monitor;

        // The first look is asked for here and not on the next tick: a page opened onto a machine that
        // is up should not spend its first interval blank. `refresh` only *asks* — the answer arrives
        // when the machine answers — so the pull after it is this app's, and on a board it leaves the
        // page saying "connecting…", which is the truth about a session that has not come up yet.
        let _ = self.board.nas().refresh();
        self.asked = Some(Instant::now());
        self.pull();
    }

    /// Saves the form: the machine goes into the list, and the page goes back to it.
    fn save(&mut self) {
        let device = match self.form.device() {
            Ok(device) => device,
            Err(wrong) => {
                self.form.wrong = Some(wrong);
                return;
            }
        };

        // A correction, and not a second machine — which is the same thing only while the address is left
        // alone. The backend keys its list by host (see `NasBackend::remember`), so writing down an
        // address the list does not have yet *adds* it, and the one the form was opened on stays where it
        // was: a list holding a typo and its correction side by side, both of them a machine to somebody
        // reading it. `Form::editing` is the address that has to go; it is what the note on `Form` means
        // by a correction being an update rather than an addition.
        //
        // Skipped when the two are alike, which is the ordinary correction — an account, a password, a
        // host key. There is nothing to drop then, and the `remember` below does the updating on its own.
        if let Some(was) = self.form.editing.clone() {
            if was != device.host {
                let _ = self.board.nas().forget(&was);
                self.seen.remove(&was);

                // The page cannot stay on an address the list no longer has, and `reload` does not read
                // `watched` back: which machine is being looked at is a choice made on a page
                // (`NasBackend::watching`), not a fact written in the file.
                if self.watched.as_ref().is_some_and(|held| held.host == was) {
                    self.watched = None;
                    self.reading = NasReading::default();
                }
            }
        }

        if self.board.nas().remember(&device).is_err() {
            // The board could not write it down — a flash that is full, or an image with no backend in
            // it at all. The form stays up with what was typed still in it, which is the only copy of
            // it there is.
            self.form.wrong = Some(Wrong::Host);
            return;
        }

        self.reload();
        self.screen = Screen::List;
    }

    /// Drops the machine the form is on, and goes back to the list.
    fn delete(&mut self) {
        let Some(host) = self.form.editing.clone() else {
            // Nothing to delete: this form was opened to *add* a machine.
            return;
        };

        self.forget(&host);
    }

    /// Drops `host` and goes back to the list.
    ///
    /// One body for the two places that drop a machine — the form's delete button, and a row's once its
    /// question has been answered — because there is more to it than the backend's `forget`: what this
    /// app remembers about the machine goes with it, and so does the monitoring page if that was the
    /// machine it was about. Two copies of that would be two chances to drop a machine and leave a page
    /// open onto nothing.
    fn forget(&mut self, host: &str) {
        let _ = self.board.nas().forget(host);

        self.seen.remove(host);

        // If that was the machine the monitoring page was on, the page has nothing left to be about.
        if self
            .watched
            .as_ref()
            .is_some_and(|device| device.host == host)
        {
            self.watched = None;
            self.reading = NasReading::default();
        }

        self.reload();
        self.screen = Screen::List;
    }

    /// Leaves the form without writing anything down.
    fn cancel(&mut self) {
        self.screen = Screen::List;
    }

    /// Re-reads the list from the backend, which is where it lives.
    fn reload(&mut self) {
        self.devices = self.board.nas().devices();
    }

    /// Reads the backend's snapshot into this state.
    ///
    /// Also into this app's own map, keyed by the machine it came from: the backend watches one machine
    /// at a time, and a row watched once keeps saying what it said — with its age — rather than going
    /// blank the moment somebody looks at a different one.
    fn pull(&mut self) {
        self.reading = self.board.nas().reading();

        if let Some(device) = &self.watched {
            self.seen.insert(device.host.clone(), self.reading.clone());
        }
    }
}

/// What a row of the list says about a machine: the ink of its dot and the word beside it.
///
/// The two places an answer can come from, and which of them wins, decided in one place because they are
/// one decision. A *check* says whether the machine is there *now* — a socket to its SSH port, opened by
/// the backend while this page is up — and a *reading* says whether a session with it worked, which is
/// older but knows more. So:
///
/// - a session that failed is what the row says, because a port that answers is not a login that works: a
///   machine whose password was changed is up, and 在线 about it would hide the only thing to fix;
/// - a reading that arrived is said as 在线, but a check that disagrees is newer and takes the row;
/// - and a machine nobody has looked at is what the checks are for — which of these boxes is there,
///   answerable without opening a session to each of them.
///
/// `None` from the check is "no answer yet", which is 检测中… and not 离线: a machine that has not been
/// knocked on is not a machine that did not answer.
fn device_status(
    known: Option<&NasStatus>,
    reached: Option<bool>,
    inks: style::StatusInk,
    muted: Color,
    words: &Words,
) -> (Color, String) {
    match known {
        Some(NasStatus::Offline(fault)) => (inks.offline(), String::from(words.fault(*fault))),
        Some(NasStatus::Connecting) => (inks.waiting(), String::from(words.connecting)),
        Some(_) => match reached {
            Some(false) => (inks.offline(), String::from(words.offline)),
            _ => (inks.online(), String::from(words.online)),
        },
        None => match reached {
            Some(true) => (inks.online(), String::from(words.online)),
            Some(false) => (inks.offline(), String::from(words.offline)),
            None => (muted, String::from(words.checking)),
        },
    }
}

/// The disk table's columns, in multiples of the body text: `None` for the one that takes what is
/// left, which is the disks' identity.
///
/// Sized against the heads and the widest value that goes under each — `8.5M/s` is the widest thing a
/// rate column holds and `500.0G` the widest a capacity does — and checked against the panel by a test,
/// because a column that does not fit is a column that arrives on the panel with half a head.
const DISK_WIDTHS: [Option<f32>; 6] = [
    None,
    Some(2.6),
    Some(2.6),
    Some(2.6),
    Some(3.9),
    Some(3.9),
];

/// The room the disk table's identity column is left with, in pixels.
///
/// The five other columns are fixed multiples of the body text ([`DISK_WIDTHS`]) and the gaps between
/// columns are fixed too, so what the identity gets is the panel less everything else. A function
/// rather than arithmetic inside the view, because the width test has to compute the *same* number:
/// a cell drawn to one width and checked against another is exactly how the model came to be cut to
/// sixteen characters — which at the base font size are 143 px wide against this column's 138 — and
/// then arrive on the panel hanging off the edge of it.
fn identity_room(sizes: style::Sizes) -> f32 {
    let fixed: f32 = DISK_WIDTHS
        .iter()
        .flatten()
        .map(|share| share * sizes.text)
        .sum();
    let gaps = style::CELL_PAD * (DISK_WIDTHS.len() - 1) as f32;

    style::PANEL - style::MARGIN * 2.0 - style::SCROLLBAR - fixed - gaps
}

/// The second line of a disk's identity: the kernel's name for it, and the model when the column can
/// hold any of one.
///
/// The name always goes in — it is what the row *is*, and it is short — and the model follows as far
/// as the room goes. Cut as a pair rather than one after the other, so that a line too long for the
/// column ends in one ellipsis instead of two.
///
/// A machine that reports no model at all says so by having nothing to append, which is the ordinary
/// case for an eMMC — `mmcblk0boot0 · ` is a separator with nothing after it. See [`format::fit`].
fn identity_detail(device: &str, model: &str, room: f32, size: f32) -> String {
    let line = if model.is_empty() {
        device.to_string()
    } else {
        format!("{device} · {model}")
    };

    format::fit(&line, room, size)
}

/// Which end each column's text sits on: the first column is the row's identity and reads from the
/// left, and the other five are numbers, which line up on the right so that a column of them can be
/// read down without a monospaced face.
const DISK_ALIGN: [Alignment; 6] = [
    Alignment::Start,
    Alignment::End,
    Alignment::End,
    Alignment::End,
    Alignment::End,
    Alignment::End,
];

/// One row of the disk table: the same six columns as the head above it, with different elements in
/// them.
///
/// The head and the rows are both made here, which is the whole point of it being a function: a head
/// laid out apart from the rows under it is a head that can drift, and this project has already paid
/// for that lesson once. See the task viewer's `table_row`.
fn table_row(cells: Vec<Element<'_, Message>>) -> Element<'_, Message> {
    let cells = cells
        .into_iter()
        .enumerate()
        .map(|(column, cell)| {
            let cell = container(cell).align_x(DISK_ALIGN[column]);

            match DISK_WIDTHS[column] {
                Some(share) => cell.width(Length::FillPortion((share * 100.0) as u16)).into(),
                None => cell.width(Length::Fill).into(),
            }
        });

    Row::with_children(cells)
        .spacing(style::CELL_PAD)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .into()
}

/// One cell of the table: text, at a size, in a colour. Where it lines up is its *column's* business,
/// and how wide it is too — see [`table_row`], which is the only place either is decided, and why the
/// head above a column cannot drift from it.
fn cell(value: &str, size: f32, color: Color) -> Element<'static, Message> {
    text(value.to_string()).size(size).color(color).into()
}

/// One card: what the metric is, the number a glance wants, the line under it, and a bar.
///
/// A card with no share draws no bar, which tells the ones whose subject is a *share of a whole* — the
/// CPU, the memory, the busiest disk — from the ones whose subject is a rate or a count and has nothing
/// to be full of. A bar for "8.4M/s" would be a bar with nothing to be a bar of.
///
/// The body is centred in the card rather than pinned to the top of it, because the card is an equal
/// share of the page and the text is not: centred, the room left over is a margin above and below the
/// number instead of a hole under it. See the settings app's rings, which are centred for the same
/// reason — a number at the top of a tall box reads as a box with something missing.
fn card(
    title: &str,
    headline: String,
    detail: String,
    share: Option<f32>,
    accent: Color,
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
    muted: Color,
) -> Element<'static, Message> {
    let mut body = Column::with_children(vec![
        text(title.to_string()).size(sizes.small).color(muted).into(),
        text(headline).size(sizes.headline).color(accent).into(),
        text(detail).size(sizes.small).color(ink).into(),
    ])
    .spacing(2.0)
    .width(Length::Fill);

    if let Some(share) = share {
        body = body.push(bar(share, accent, sizes, theme));
    }

    container(body)
        .padding(style::CELL_PAD * 2.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(style::card(theme).into()),
            border: iced::Border {
                radius: (sizes.small).into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// The bar at the foot of a card: the filled part, the empty part, both rounded as one.
///
/// `FillPortion` and not a measured width — the layout engine already knows what to do with the two
/// shares, and a portion of *zero* is a portion it reads as "no share at all", which is why both halves
/// are clamped to at least one.
fn bar(share: f32, accent: Color, sizes: style::Sizes, theme: ThemeMode) -> Element<'static, Message> {
    let filled = (share.clamp(0.0, 1.0) * 100.0).round().clamp(1.0, 99.0) as u16;
    let empty = 100 - filled;

    let segment = move |portion: u16, color: Color| -> Element<'static, Message> {
        container(Space::new())
            .width(Length::FillPortion(portion))
            .height(Length::Fixed(sizes.bar))
            .style(move |_theme| container::Style {
                background: Some(color.into()),
                ..container::Style::default()
            })
            .into()
    };

    Row::with_children(vec![
        segment(filled, accent),
        segment(empty, style::track(theme)),
    ])
    .spacing(1.0)
    .width(Length::Fill)
    .into()
}

/// A bare glyph on a button: the actions in a header that are not readings — back, reveal.
///
/// Drawn muted and turning ink under the finger, which is the pair of colours every other muted thing
/// on the page uses, and with no fill at all: a header has no room for a second shape on it.
///
/// A pencil was in that list until the monitoring page's header lost its own: correcting a machine is
/// the list row's, where it can be reached without a session. See [`Message::EditDevice`], and
/// [`terminal_button`], which is what the header kept.
fn icon_button(
    message: Message,
    icon: Icon,
    sizes: style::Sizes,
    ink: Color,
    muted: Color,
) -> Element<'static, Message> {
    button(
        text(icon.glyph())
            .font(pomelo_material_symbols::font())
            .size(sizes.text)
            .color(muted),
    )
    .padding(style::CELL_PAD * 0.8)
    .on_press(message)
    .style(move |_theme, status| button::Style {
        background: None,
        text_color: if matches!(status, button::Status::Pressed) {
            ink
        } else {
            muted
        },
        ..button::Style::default()
    })
    .into()
}

/// The monitoring page's one action: a shell on the machine the header is about, as a glyph *and* a
/// word.
///
/// A word, because the two tabs beside it are words: a glyph alone on that row would be a third thing
/// whose meaning has to be guessed at, and it is the one thing there nobody can guess from the page
/// they are looking at. Not *only* the word either, or it would read as a third tab — a half of the
/// page rather than something done to the machine.
///
/// Sized with the tabs' own numbers, so the row is one height whichever shape each button holds, and
/// pressed the way they are: the fill becomes [`style::track`] under the finger. What it rests on is
/// [`style::card`] rather than that track, and the surface it sits on is why — the row's chips rest on
/// a card, where a card-coloured one would be nothing at all, and this button rests on the page, where
/// a card is what a shape looks like. See [`row_action`] for the same trade made the other way round.
fn terminal_button(
    message: Message,
    label: &str,
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
) -> Element<'static, Message> {
    button(
        Row::with_children(vec![
            text(Icon::TERMINAL.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.small)
                .color(ink)
                .into(),
            Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
            text(label.to_string()).size(sizes.small).color(ink).into(),
        ])
        .align_y(Alignment::Center),
    )
    .padding([style::CELL_PAD * 0.6, style::CELL_PAD * 2.0])
    .on_press(message)
    .style(move |_theme, status| button::Style {
        background: Some(
            if matches!(status, button::Status::Pressed) {
                style::track(theme)
            } else {
                style::card(theme)
            }
            .into(),
        ),
        text_color: ink,
        border: Border {
            radius: (sizes.small * 0.8).into(),
            ..Border::default()
        },
        ..button::Style::default()
    })
    .into()
}

/// A row's own action: a word on a button, and the button the size of a finger.
///
/// The pencil and the bin were bare glyphs here until they were tried with a thumb, and a glyph with
/// nothing around it is a target the width of its own ink — twenty-odd pixels across, on a panel where
/// the row above and the row below are what a slip hits instead. A word is wider than a glyph at the
/// same type size, and it says what the button does without having to be learnt, which is what a row
/// read in passing needs.
///
/// The chip is [`style::track`] and not [`style::card`] like every other button on this page rests on.
/// Those buttons sit *on the page*, where a card-coloured one is a shape against it; this one sits on a
/// card, where the same colour is nothing at all. The track is the one surface here that is neither the
/// card nor a meaning — [`style::network`] is the network, [`style::status_ink`] is a status — so a chip
/// of it is what "pressable" looks like inside a row. Under the finger the chip fills with the ink the
/// word was drawn in and the word cuts out of it, which is the same trade the buttons above the keyboard
/// make by filling: the colour that meant the word becomes the thing behind it.
fn row_action(
    label: &str,
    message: Message,
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
) -> Element<'static, Message> {
    button(text(label.to_string()).size(sizes.text))
        .padding([style::CELL_PAD * 1.2, style::CELL_PAD * 2.0])
        .on_press(message)
        .style(move |_theme, status| {
            let pressed = matches!(status, button::Status::Pressed);

            button::Style {
                background: Some(
                    if pressed {
                        ink
                    } else {
                        style::track(theme)
                    }
                    .into(),
                ),
                text_color: if pressed { style::surface(theme) } else { ink },
                border: Border {
                    radius: sizes.small.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
}

/// The list's "add a machine" button: the one thing on that page that is not a machine.
///
/// Filled with the card colour and turning the network green under a finger, because it is the page's
/// only action and the thing somebody who has just opened the app is most likely to be looking for.
fn add_button(
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
    words: &Words,
) -> Element<'static, Message> {
    button(
        Row::with_children(vec![
            text(Icon::ADD.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.text)
                .color(ink)
                .into(),
            Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
            text(words.add).size(sizes.text).color(ink).into(),
        ])
        .align_y(Alignment::Center),
    )
    .padding(style::CELL_PAD * 1.5)
    .width(Length::Fill)
    .on_press(Message::Add)
    .style(move |_theme, status| button::Style {
        background: Some(
            if matches!(status, button::Status::Pressed) {
                style::network()
            } else {
                style::card(theme)
            }
            .into(),
        ),
        border: Border {
            radius: sizes.small.into(),
            ..Border::default()
        },
        ..button::Style::default()
    })
    .into()
}

/// The question a row's bin asks: which machine, and the two answers to it.
///
/// A question and not a second tap on the bin, because the two are not the same promise: a bin that
/// dropped the machine would be a row where a thumb that slipped while scrolling has already done it,
/// and what it drops is an address, an account and a password nobody has written down anywhere else.
/// 取消 is drawn and not merely implied by tapping elsewhere, because "anywhere else" on this page is a
/// row, and a row is a button.
fn confirm_bar(
    host: &str,
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
    words: &Words,
) -> Element<'static, Message> {
    let danger = style::status_ink(theme).offline();

    Row::with_children(vec![
        text(format!("{} {host}?", words.delete_ask))
            .size(sizes.text)
            .color(ink)
            .width(Length::Fill)
            .into(),
        form_button(words.cancel, Message::CancelDelete, false, sizes, theme, ink),
        Space::new().width(Length::Fixed(style::CELL_PAD)).into(),
        danger_button(words.delete, Message::ConfirmDelete, sizes, theme, danger),
    ])
    .spacing(style::CELL_PAD * 0.5)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// A button in the destructive colour: the confirmation's yes.
///
/// Styled the way the form's own delete button is — red on the card, filling with red under the finger
/// and turning its word white — so that the two places this app drops a machine are recognisably the
/// same act. See [`NasMonitor::delete_button`], which is the other one.
fn danger_button(
    label: &str,
    message: Message,
    sizes: style::Sizes,
    theme: ThemeMode,
    danger: Color,
) -> Element<'static, Message> {
    button(text(label.to_string()).size(sizes.text))
        .padding([style::CELL_PAD * 1.2, style::CELL_PAD * 2.5])
        .on_press(message)
        .style(move |_theme, status| {
            let pressed = matches!(status, button::Status::Pressed);

            button::Style {
                background: Some(if pressed { danger } else { style::card(theme) }.into()),
                text_color: if pressed { Color::WHITE } else { danger },
                border: Border {
                    radius: sizes.small.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
}

/// One of the form's two ordinary buttons: save and cancel, which differ only in which one is filled.
fn form_button(
    label: &str,
    message: Message,
    primary: bool,
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
) -> Element<'static, Message> {
    button(text(label.to_string()).size(sizes.text))
        .padding([style::CELL_PAD * 1.2, style::CELL_PAD * 2.5])
        .on_press(message)
        .style(move |_theme, status| button::Style {
            background: Some(
                match (primary, matches!(status, button::Status::Pressed)) {
                    (true, _) => style::network(),
                    (false, true) => style::track(theme),
                    (false, false) => style::card(theme),
                }
                .into(),
            ),
            // White on the filled one whatever the theme: the accent is the same green either way, and
            // ink would be dark ink on it in light mode.
            text_color: if primary { Color::WHITE } else { ink },
            border: Border {
                radius: sizes.small.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

/// A password, as one dot per character.
///
/// Painted rather than typed: the font this board carries is a Chinese and Latin subset with no `•` in
/// it, which is the same wall the settings app's password prompt runs into — see its
/// `style::typed_password`, which draws its own dots for the same reason.
fn dots(count: usize, sizes: style::Sizes, ink: Color) -> Element<'static, Message> {
    let dot = sizes.small * 0.35;

    let mut row = Row::new().spacing(4.0).align_y(Alignment::Center);

    for _ in 0..count {
        row = row.push(
            container(Space::new())
                .width(Length::Fixed(dot))
                .height(Length::Fixed(dot))
                .style(move |_theme| container::Style {
                    background: Some(ink.into()),
                    border: Border {
                        radius: (dot / 2.0).into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
        );
    }

    row.into()
}

/// A whole-page notice: what is missing, said once, where the cards would be.
///
/// The page keeps its header in this state, because the header is what says which machine — and a
/// person who has two NASes and one of them unconfigured needs to know *which* one is quiet.
fn notice(message: &str, sizes: style::Sizes, muted: Color) -> Element<'static, Message> {
    container(text(message.to_string()).size(sizes.text).color(muted))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// The words on the page, in the language the system is set to.
///
/// A struct and not a `match` at each use, because there are a dozen of them and a page that translated
/// eleven would be worse than one that translated none. The task viewer's `columns` is the same idea for
/// a table's heads.
struct Words {
    language: Language,
    /// The app's own name, which is the list's heading: the board's grid has a tile, and a page inside
    /// the app has no other place to say what this is.
    title: &'static str,
    /// The list's button, and the form's heading when it is adding one.
    add: &'static str,
    /// The form's heading when it is correcting one.
    edit_device: &'static str,
    no_devices: &'static str,
    /// What a row says about a machine: that it answered, that it did not, and what it says while the
    /// answer has not come back.
    ///
    /// Words and not only the dot beside them. The dot is the same decision — green, red, or neither —
    /// but a colour on its own asks the reader to remember which way round it goes, and this list is read
    /// from across a room. See [`NasBackend::answering`], which is where the answer comes from.
    online: &'static str,
    offline: &'static str,
    checking: &'static str,
    host: &'static str,
    account: &'static str,
    password: &'static str,
    save: &'static str,
    cancel: &'static str,
    /// A row's pencil, now that it is a word rather than a glyph. It is the row's alone: the form a row
    /// opens says [`Words::edit_device`] at its head instead, because "edit" as a heading names a mode
    /// and "edit <machine>" names the machine — see [`NasMonitor::form`].
    edit: &'static str,
    delete: &'static str,
    /// What a row's bin says before it means it. The machine's address is appended to it by
    /// [`confirm_bar`], because the question is about one machine and the name is the only part of the
    /// sentence that is not the same every time.
    delete_ask: &'static str,
    wrong_host: &'static str,
    wrong_user: &'static str,
    cpu: &'static str,
    memory: &'static str,
    network: &'static str,
    disks_speed: &'static str,
    storage: &'static str,
    disk_busy: &'static str,
    received: &'static str,
    sent: &'static str,
    read: &'static str,
    written: &'static str,
    temperature: &'static str,
    uptime: &'static str,
    connecting: &'static str,
    overview: &'static str,
    disks: &'static str,
    /// The word on the button beside those two tabs: a shell on the machine the page is about. See
    /// [`terminal_button`], which is where it is drawn, and [`Message::Terminal`], which is what pressing
    /// it says.
    terminal: &'static str,
    heads: [&'static str; 6],
    /// What to call the machine before one has been named — the header's fallback, for a board nobody
    /// has pointed at anything yet. The credentials' host is what goes there once there are any.
    server: &'static str,
    unconfigured: &'static str,
    unsupported: &'static str,
    no_network: &'static str,
    unreachable: &'static str,
    authentication: &'static str,
    host_key: &'static str,
    protocol: &'static str,
}

impl Words {
    fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language,
                title: "服务器监控",
                add: "添加设备",
                edit_device: "编辑设备",
                no_devices: "还没有添加服务器：点下面的按钮填上地址和账号",
                online: "在线",
                offline: "离线",
                checking: "检测中…",
                host: "地址",
                account: "账号",
                password: "密码",
                save: "保存",
                cancel: "取消",
                edit: "编辑",
                delete: "删除",
                delete_ask: "确定删除",
                wrong_host: "要填地址",
                wrong_user: "要填账号",
                cpu: "CPU",
                memory: "内存",
                network: "网络",
                disks_speed: "硬盘读写",
                storage: "存储",
                disk_busy: "磁盘繁忙",
                received: "接收",
                sent: "发送",
                read: "读取",
                written: "写入",
                temperature: "温度",
                uptime: "本次运行",
                connecting: "正在连接…",
                overview: "概览",
                disks: "硬盘",
                terminal: "终端",
                heads: ["硬盘", "容量", "温度", "繁忙", "读取", "写入"],
                server: "服务器",
                unconfigured: "还没有配置服务器：这台板子还不知道要看哪一台",
                unsupported: "本版固件还没有 SSH 客户端，暂时看不了服务器",
                no_network: "板子没连上网络",
                unreachable: "连不上那台机器",
                authentication: "账号或密码不对",
                host_key: "主机密钥变了",
                protocol: "双方谈不到一起（协议不匹配）",
            },
            Language::English => Self {
                language,
                title: "Server monitor",
                add: "Add a server",
                edit_device: "Edit server",
                no_devices: "No servers yet: use the button below to add an address and an account",
                online: "online",
                offline: "offline",
                checking: "checking…",
                host: "Address",
                account: "Account",
                password: "Password",
                save: "Save",
                cancel: "Cancel",
                edit: "Edit",
                delete: "Delete",
                delete_ask: "Delete",
                wrong_host: "an address is required",
                wrong_user: "an account is required",
                cpu: "CPU",
                memory: "Memory",
                network: "Network",
                disks_speed: "Disks",
                storage: "Storage",
                disk_busy: "Disk busy",
                received: "in",
                sent: "out",
                read: "read",
                written: "written",
                temperature: "temp",
                uptime: "up",
                connecting: "connecting…",
                overview: "Overview",
                disks: "Disks",
                terminal: "Terminal",
                heads: ["DISK", "SIZE", "TEMP", "BUSY", "READ", "WRITE"],
                server: "Server",
                unconfigured: "No server configured: this board has not been told which one to watch",
                unsupported: "This image has no SSH client yet, so it cannot watch a server",
                no_network: "the board is not on a network",
                unreachable: "nothing answered",
                authentication: "wrong user or password",
                host_key: "the host key changed",
                protocol: "no shared protocol",
            },
        }
    }

    /// What to say about a fault, in this language.
    fn fault(&self, fault: NasFault) -> &'static str {
        match fault {
            NasFault::NoNetwork => self.no_network,
            NasFault::Unreachable => self.unreachable,
            NasFault::Authentication => self.authentication,
            NasFault::HostKeyChanged => self.host_key,
            NasFault::Protocol => self.protocol,
            NasFault::Unsupported => self.unsupported,
        }
    }

    /// What to say about a form that will not save, in this language.
    fn wrong(&self, wrong: Wrong) -> &'static str {
        match wrong {
            Wrong::Host => self.wrong_host,
            Wrong::User => self.wrong_user,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_hal::sim::SimNas;
    use pomelo_hal::traits::NasBackend;

    /// One machine, as the form would have written it.
    fn credentials(host: &str) -> NasCredentials {
        NasCredentials {
            host: host.to_string(),
            port: 22,
            user: String::from("nasstat"),
            password: String::from("hunter2"),
            interval_secs: 5,
            host_key: None,
        }
    }

    /// A board told about one machine and opened the way the launcher opens it: on the list.
    fn monitor() -> NasMonitor {
        let board = Arc::new(Board::simulated());
        board.nas().remember(&credentials("192.168.1.10")).unwrap();

        NasMonitor::new(board)
    }

    /// A board told about one machine that is *not* there: the simulator switched off, so that both of its
    /// answers are failures — the look cannot connect and the list's check cannot reach the port.
    ///
    /// Written out at length because a board is a composition root's decision and a test is a composition
    /// root: there is no `Board::simulated` with one backend chosen, and the one that has to be chosen here
    /// is the NAS's. See [`pomelo_hal::sim::SimNas::unreachable`].
    fn monitor_of_a_machine_that_is_off() -> NasMonitor {
        use pomelo_hal::sim::{
            SimAudio, SimHttp, SimImu, SimInput, SimMic, SimPower, SimStorage, SimSystem, SimWeb,
            SimWifi,
        };

        let mut nas = SimNas::unreachable();
        nas.remember(&credentials("192.168.1.10")).unwrap();

        let board = Arc::new(Board::from_backends(
            Box::new(SimPower::new()),
            Box::new(SimWifi::new()),
            Box::new(SimAudio::new()),
            Box::new(SimMic::new()),
            Box::new(SimImu::new()),
            Box::new(SimInput::new()),
            Box::new(SimStorage::new()),
            Box::new(SimWeb::new()),
            Box::new(SimSystem::new()),
            Box::new(nas),
            Box::new(SimHttp::new()),
        ));

        NasMonitor::new(board)
    }

    /// Types `text` into the field the form is on, a character at a time — which is what the panel's own
    /// keyboard does.
    fn type_into(monitor: &mut NasMonitor, field: Field, text: &str) {
        monitor.update(Message::Focus(field));

        for character in text.chars() {
            monitor.update(Message::Key(KeyAction::Char(character)));
        }
    }

    /// The app opens on the list, and a row is what puts a machine on the monitoring page.
    ///
    /// The whole of the shape this app grew: a board can be pointed at several machines, so the page
    /// says *which* one it is about before it says anything about it.
    #[test]
    fn the_app_opens_on_the_list_and_a_row_opens_the_machine() {
        let mut monitor = monitor();

        assert_eq!(monitor.screen(), Screen::List);
        assert_eq!(monitor.devices().len(), 1);
        assert!(monitor.watched().is_none(), "and nothing is being looked at yet");

        monitor.update(Message::Open(0));

        assert_eq!(monitor.screen(), Screen::Monitor);
        assert_eq!(monitor.watched().unwrap().host, "192.168.1.10");
        // The look is asked for as the page opens and the simulator answers inside the call, so the
        // first frame of the monitoring page is a machine that has already answered.
        assert_eq!(monitor.reading().status, NasStatus::Online);
        assert_eq!(monitor.reading().disks.len(), 3);
        assert!(
            monitor.seen("192.168.1.10").is_some(),
            "and the row of the list remembers what it saw"
        );
        let _ = monitor.view();

        // Back leaves the machine in the list, and the row still saying what it said.
        monitor.update(Message::Back);

        assert_eq!(monitor.screen(), Screen::List);
        assert_eq!(monitor.devices().len(), 1);
        assert!(monitor.seen("192.168.1.10").is_some());
        let _ = monitor.view();
    }

    /// The page draws from a reading, and a reading arrives from the backend rather than from a frame:
    /// the app pulls on every tick, asks on the interval — and on the priming beat while the reading has
    /// no rates in it, which is the one thing asked for off the interval. See [`PRIME`].
    #[test]
    fn a_tick_pulls_every_time_and_asks_on_the_interval() {
        let mut monitor = monitor();
        monitor.update(Message::Open(0));

        // A machine's first look has no rates in it, so the app is on its priming beat: a tick inside
        // *that* pulls and does not ask again, which is the whole of why the app has an `asked` at all.
        assert!(monitor.reading().cpu.is_none(), "one look cannot know the CPU");
        let start = monitor.asked.unwrap();
        monitor.tick(start + Duration::from_millis(500));

        assert_eq!(
            monitor.asked,
            Some(start),
            "no second look inside the priming beat"
        );

        // A second later it asks for the second look — the one with the rates in it. The pull is the
        // *next* tick, because this tick's ask is answered after it.
        monitor.tick(start + PRIME);

        assert_ne!(monitor.asked, Some(start), "the second look is asked for");

        let second = monitor.asked.unwrap();
        monitor.tick(second + Duration::from_millis(100));

        assert!(
            monitor.reading().cpu.is_some(),
            "and the second look is what brings the CPU back, off the priming beat"
        );

        // From there the interval takes over: a tick inside it pulls and does not ask.
        monitor.tick(second + Duration::from_secs(1));

        assert_eq!(monitor.asked, Some(second), "no second look inside the interval");

        // And once the interval is up, it asks — and what it has on screen has an age, because a rate
        // that arrived five seconds ago is five seconds old.
        monitor.tick(second + Duration::from_secs(6));

        assert_ne!(monitor.asked, Some(second), "a look when the interval is up");
        assert!(monitor.reading().age().is_some());

        let _ = monitor.view();
    }

    /// A tick on the list asks for the machines to be *checked* and never for a look: a look is a session
    /// with one machine, the list is about all of them, and a page that opened a session per row would be
    /// the reason nobody left it open. See [`NasBackend::check`].
    #[test]
    fn the_list_checks_the_machines_and_never_looks_at_one() {
        let mut monitor = monitor();
        let start = monitor.asked.unwrap();

        // What the rows are drawn from: the checks. A machine the simulator has been told about answers
        // them, and one it has never heard of has nothing to say — which is not the same as "down".
        assert_eq!(monitor.board.nas().answering("192.168.1.10"), Some(true));
        assert_eq!(monitor.board.nas().answering("10.0.0.1"), None);

        monitor.tick(start + Duration::from_secs(600));

        assert_eq!(
            monitor.asked,
            Some(start),
            "the list does not start the look's clock"
        );
        assert_eq!(
            monitor.reading().status,
            NasStatus::Connecting,
            "and nothing was looked at: the machine nobody has opened is still waiting for its first look"
        );
        let _ = monitor.view();

        // A machine that is switched off answers neither question: the check finds no port, and a look at
        // it fails to connect.
        let mut off = monitor_of_a_machine_that_is_off();

        assert_eq!(off.board.nas().answering("192.168.1.10"), Some(false));

        off.update(Message::Open(0));

        assert_eq!(
            off.reading().status,
            NasStatus::Offline(NasFault::Unreachable)
        );

        // And back on the list the row it goes to says so, in red: the reading is what is known about a
        // machine this session has *opened*, and it outlives the page that opened it.
        off.update(Message::Back);

        assert_eq!(off.screen(), Screen::List);
        assert!(off.seen("192.168.1.10").is_some());
        let _ = off.view();
    }

    /// What a row says about a machine, in every case a list has to get right: one that is there, one that
    /// is not, one whose *login* is what is wrong with it, and one nobody has knocked on yet.
    ///
    /// See [`device_status`], which is the decision this is the test of — the reading and the check say
    /// different things and one of them has to win.
    #[test]
    fn a_row_says_whether_the_machine_is_there() {
        let inks = style::status_ink(ThemeMode::Dark);
        let muted = style::muted(ThemeMode::Dark);
        let words = Words::of(Language::Chinese);

        let status = |known: Option<&NasStatus>, reached: Option<bool>| {
            device_status(known, reached, inks, muted, &words)
        };

        // Nobody has looked at it, so the check is all there is — and its two answers are the two words.
        assert_eq!(
            status(None, Some(true)),
            (inks.online(), words.online.to_string())
        );
        assert_eq!(
            status(None, Some(false)),
            (inks.offline(), words.offline.to_string())
        );

        // Before the first check has come back it is 检测中…, which is not 离线: a machine nobody has
        // knocked on is not a machine that did not answer.
        assert_eq!(status(None, None), (muted, words.checking.to_string()));

        // A reading that arrived says the machine answered *when it was taken* — and it stands as the
        // answer until a check disagrees with it, a check being about now.
        assert_eq!(status(Some(&NasStatus::Online), None).1, words.online);
        assert_eq!(status(Some(&NasStatus::Online), Some(true)).1, words.online);
        assert_eq!(status(Some(&NasStatus::Online), Some(false)).1, words.offline);

        // A session that failed says more than a port that answered, whatever the port says: a machine
        // whose password was changed is up, and the row has to be the thing somebody has to fix.
        let wrong_password = NasStatus::Offline(NasFault::Authentication);

        assert_eq!(
            status(Some(&wrong_password), Some(true)),
            (inks.offline(), words.authentication.to_string())
        );
        assert_eq!(status(Some(&wrong_password), Some(false)).1, words.authentication);

        // And a session still coming up is its own state, in the ink that is neither answer.
        assert_eq!(
            status(Some(&NasStatus::Connecting), Some(true)),
            (inks.waiting(), words.connecting.to_string())
        );

        // Every one of those words is in both languages and none of them is the same as another, which is
        // what a list of one-word answers needs to be worth reading at all.
        for language in [Language::Chinese, Language::English] {
            let words = Words::of(language);

            assert_ne!(words.online, words.offline, "{language:?}");
            assert_ne!(words.online, words.checking, "{language:?}");
            assert_ne!(words.offline, words.checking, "{language:?}");
        }
    }

    /// Typing a machine into the form and saving puts it in the list, and the fields the form does not
    /// ask about come out as the defaults the credentials file documents.
    #[test]
    fn the_form_adds_a_machine() {
        let mut monitor = monitor();

        monitor.update(Message::Add);
        assert_eq!(monitor.screen(), Screen::Form);

        type_into(&mut monitor, Field::Host, "192.168.1.20");

        // Saving with no account is refused, and the form says which half of the login is missing.
        monitor.update(Message::Save);

        assert_eq!(monitor.screen(), Screen::Form, "the form stays up");
        assert_eq!(monitor.form.wrong, Some(Wrong::User));

        type_into(&mut monitor, Field::User, "nasstat");
        monitor.update(Message::Save);

        assert_eq!(monitor.screen(), Screen::List);
        assert_eq!(monitor.devices().len(), 2);
        assert_eq!(monitor.devices()[1].host, "192.168.1.20");
        assert_eq!(monitor.devices()[1].user, "nasstat");
        assert_eq!(monitor.devices()[1].port, 22, "the port is the file's default");
        assert_eq!(monitor.devices()[1].interval_secs, 5);
        let _ = monitor.view();
    }

    /// A form opened on a machine already in the list writes that entry rather than a second one, and
    /// the delete on it drops the entry it was opened on.
    #[test]
    fn the_form_corrects_and_deletes() {
        let mut monitor = monitor();

        monitor.update(Message::EditDevice(0));

        assert_eq!(monitor.screen(), Screen::Form);
        assert_eq!(monitor.form.host, "192.168.1.10", "filled in with what is there");
        assert!(monitor.form.editing.is_some(), "and it is a correction");

        // Retyping the password, which is what a form is usually opened for.
        monitor.update(Message::Focus(Field::Password));

        while !monitor.form.password.is_empty() {
            monitor.update(Message::Key(KeyAction::Backspace));
        }
        monitor.update(Message::Key(KeyAction::Char('n')));
        monitor.update(Message::Save);

        assert_eq!(monitor.screen(), Screen::List);
        assert_eq!(monitor.devices().len(), 1, "corrected, not added a second time");
        assert_eq!(monitor.devices()[0].password, "n");

        // And the same machine, dropped — through the form, which is the list's other way into it. Opened
        // first, so that what the delete takes away is a machine this page was actually on.
        monitor.update(Message::Open(0));
        monitor.update(Message::EditDevice(0));
        monitor.update(Message::Delete);

        assert_eq!(monitor.screen(), Screen::List);
        assert!(monitor.devices().is_empty());
        assert!(
            monitor.watched().is_none(),
            "and the page has nothing left to be about"
        );
        assert_eq!(monitor.reading().status, NasStatus::Unconfigured);
        let _ = monitor.view();
    }

    /// A board told about two machines, which is the shortest list a row's buttons can be told apart on.
    ///
    /// With one machine in it, "the row that was pressed" and "the machine the page is on" are the same
    /// machine, and a test of the difference between them passes whichever of the two the code reads.
    fn monitor_of_two_machines() -> NasMonitor {
        let board = Arc::new(Board::simulated());
        board.nas().remember(&credentials("192.168.1.10")).unwrap();
        board.nas().remember(&credentials("192.168.1.20")).unwrap();

        NasMonitor::new(board)
    }

    /// A row's pencil opens the form on *that* row's machine, without opening a session with it first.
    ///
    /// Which is the reason the pencil is on the row at all. Correcting a machine is what somebody does
    /// when its credentials are wrong — a password changed on the box, an account renamed — and opening a
    /// session with it is the one thing wrong credentials will not allow. While there was a pencil in the
    /// monitoring page's header as well, that one meant opening the machine, which meant a session, which
    /// meant it could not be done; the header's is gone and this is the only one left.
    ///
    /// Two machines, and the page on the second: the row's pencil has to open the *first* one, which is
    /// not the machine the page is on.
    #[test]
    fn a_rows_pencil_opens_the_form_on_that_row() {
        let mut monitor = monitor_of_two_machines();

        monitor.update(Message::Open(1));
        assert_eq!(monitor.watched().unwrap().host, "192.168.1.20");
        monitor.update(Message::Back);
        assert_eq!(monitor.screen(), Screen::List);

        monitor.update(Message::EditDevice(0));

        assert_eq!(monitor.screen(), Screen::Form);
        assert_eq!(monitor.form.host, "192.168.1.10", "the row that was pressed");
        assert_eq!(monitor.form.user, "nasstat");
        assert!(monitor.form.editing.is_some(), "and it is a correction");
        assert_eq!(monitor.devices().len(), 2, "and opening it wrote nothing down");
        let _ = monitor.view();
    }

    /// A corrected address rewrites the machine the form was opened on instead of adding a second one
    /// beside it.
    ///
    /// The case a row's pencil exists for — an address typed wrong, or a box that moved — and the one the
    /// form got wrong for as long as it only ever called `remember`. The backend keys its list by host, so
    /// an address the list does not have yet is a *new* machine: the typo stayed in the list beside its own
    /// correction, two rows for one box and one of them permanently offline. `Form::editing` is the
    /// address that has to go, and it is what the note on `Form` means by a correction being an update.
    #[test]
    fn a_corrected_address_rewrites_that_machine() {
        let mut monitor = monitor();

        monitor.update(Message::EditDevice(0));
        assert_eq!(monitor.form.host, "192.168.1.10");

        // Retyped the way the panel's keyboard retypes a field: cleared, then a character at a time.
        monitor.update(Message::Focus(Field::Host));

        while !monitor.form.host.is_empty() {
            monitor.update(Message::Key(KeyAction::Backspace));
        }
        type_into(&mut monitor, Field::Host, "192.168.1.11");
        monitor.update(Message::Save);

        assert_eq!(monitor.screen(), Screen::List);
        assert_eq!(monitor.devices().len(), 1, "corrected, not added beside it");
        assert_eq!(monitor.devices()[0].host, "192.168.1.11");
        assert_eq!(
            monitor.devices()[0].user, "nasstat",
            "and the rest of the machine came with it"
        );
        let _ = monitor.view();
    }

    /// A row's bin asks before it drops anything, and 取消 is a real way out of the question.
    ///
    /// The row sits in a list somebody is scrolling with a thumb, and what it would drop is an address, an
    /// account and a password nobody has written down anywhere else — so a bin that dropped the machine
    /// would be a row where a slipped thumb has already done it. The files app's `Prompt` is the same
    /// trade made the same way.
    #[test]
    fn a_rows_bin_asks_before_it_drops_a_machine() {
        let mut monitor = monitor();

        monitor.update(Message::DeleteDevice(0));

        assert_eq!(
            monitor.confirming(),
            Some("192.168.1.10"),
            "the row asks, and names the machine it is asking about"
        );
        assert_eq!(monitor.screen(), Screen::List, "and the list stays up");
        assert_eq!(monitor.devices().len(), 1, "and nothing has been dropped yet");
        let _ = monitor.view();

        // 取消: the question goes, the machine stays.
        monitor.update(Message::CancelDelete);

        assert!(monitor.confirming().is_none());
        assert_eq!(monitor.devices().len(), 1, "a question answered no is not a machine dropped");
        let _ = monitor.view();

        // Asking again and answering yes is what drops it.
        monitor.update(Message::DeleteDevice(0));
        monitor.update(Message::ConfirmDelete);

        assert!(monitor.confirming().is_none());
        assert!(monitor.devices().is_empty());
        let _ = monitor.view();
    }

    /// The question stays about the machine somebody pointed at, and not about whatever is in that row by
    /// the time it is answered.
    ///
    /// The reason `confirming` holds an address rather than a place in the list. A machine dropped through
    /// the form takes its row with it and every row below moves up by one, so an index asked about before
    /// that lands one machine off afterwards — and the wrong box is the one that goes.
    #[test]
    fn the_question_follows_the_machine_and_not_the_row() {
        let mut monitor = monitor_of_two_machines();

        monitor.update(Message::DeleteDevice(1));
        assert_eq!(monitor.confirming(), Some("192.168.1.20"));

        // The first machine goes the long way round, through the form, which pulls the second row up into
        // the place the question was asked in.
        monitor.update(Message::Open(0));
        monitor.update(Message::EditDevice(0));
        monitor.update(Message::Delete);

        assert_eq!(monitor.devices().len(), 1);
        assert_eq!(monitor.devices()[0].host, "192.168.1.20", "the row moved up");

        monitor.update(Message::ConfirmDelete);

        assert!(
            monitor.devices().is_empty(),
            "the machine that was asked about went, not the one that took its row"
        );
    }

    /// A board that cannot watch a NAS says so, and it is a different screen from a board nobody has
    /// finished setting up.
    ///
    /// Both states end in the same place — no cards — and want different sentences: one is a setting
    /// somebody has to fill in and the other is a firmware somebody has to flash. The device's
    /// placeholder backend is the second (`pomelo-hal-esp32`'s `nas`), and this is that backend's shape
    /// built here, because a board is a composition root's decision and a test is a composition root.
    #[test]
    fn a_board_that_cannot_do_it_says_which_it_is() {
        use pomelo_hal::error::HalError;
        use pomelo_hal::sim::{
            SimAudio, SimHttp, SimImu, SimInput, SimMic, SimPower, SimStorage, SimSystem, SimWeb,
            SimWifi,
        };
        use pomelo_hal::traits::NasBackend;

        /// The device's placeholder, in one place: it refuses the setting and answers a reading that
        /// says "not this board", rather than one that says "nothing configured".
        struct Refusing;

        impl NasBackend for Refusing {
            fn devices(&self) -> Vec<NasCredentials> {
                Vec::new()
            }

            fn watching(&self) -> Option<NasCredentials> {
                None
            }

            fn remember(&mut self, _credentials: &NasCredentials) -> Result<(), HalError> {
                Err(HalError::NotSupported)
            }

            fn forget(&mut self, _host: &str) -> Result<(), HalError> {
                Err(HalError::NotSupported)
            }

            fn refresh(&mut self) -> Result<(), HalError> {
                Err(HalError::NotSupported)
            }

            fn reading(&self) -> NasReading {
                NasReading {
                    status: NasStatus::Offline(NasFault::Unsupported),
                    ..NasReading::default()
                }
            }
        }

        let board = Arc::new(Board::from_backends(
            Box::new(SimPower::new()),
            Box::new(SimWifi::new()),
            Box::new(SimAudio::new()),
            Box::new(SimMic::new()),
            Box::new(SimImu::new()),
            Box::new(SimInput::new()),
            Box::new(SimStorage::new()),
            Box::new(SimWeb::new()),
            Box::new(SimSystem::new()),
            Box::new(Refusing),
            Box::new(SimHttp::new()),
        ));

        let monitor = NasMonitor::new(board);

        assert_eq!(
            monitor.reading().status,
            NasStatus::Offline(NasFault::Unsupported)
        );
        assert!(monitor.watched().is_none(), "and nothing was taken");
        let _ = monitor.view();

        // And the two sentences are two sentences, in both languages.
        for language in [Language::Chinese, Language::English] {
            let words = Words::of(language);
            assert_ne!(words.unsupported, words.unconfigured, "{language:?}");
            assert_eq!(words.fault(NasFault::Unsupported), words.unsupported);

            // Every fault has words of its own, and none of them is empty.
            for fault in [
                NasFault::NoNetwork,
                NasFault::Unreachable,
                NasFault::Authentication,
                NasFault::HostKeyChanged,
                NasFault::Protocol,
                NasFault::Unsupported,
            ] {
                assert!(!words.fault(fault).is_empty(), "{fault:?} {language:?}");
            }
        }

    }

    /// Every head fits the column it names, and the columns fit the panel with room for the names.
    ///
    /// The arithmetic the task viewer's table needed and did not have at first: fixed columns are a
    /// promise about the widest thing that goes in them, and the two ways to break it are a head wider
    /// than its column and a total wider than the panel.
    #[test]
    fn the_columns_fit_the_panel_and_their_heads_fit_in_them() {
        // The width estimate, taken from the code the cells are actually *cut* by rather than written
        // out again here — see [`format::width_of`], which is where it lives and why. A test that
        // measured by its own copy of the arithmetic is a test that can pass while the panel clips.
        use crate::format::width_of;

        // The one size this table is laid out for, [`FONT_BASE`]. At the base, eight columns of
        // Chinese-labelled numbers fit in 480 px: the widths below are the room the panel really has,
        // and the checks that follow are what keep the columns from clipping.
        {
            let sizes = style::Sizes::new();

            // Every head, and the widest value under it.
            let widest = ["存储空间 1", "500G", "38°C", "100%", "8.5M/s", "9.8M/s"];

            for language in [Language::Chinese, Language::English] {
                let words = Words::of(language);

                for (index, head) in words.heads.iter().enumerate() {
                    let Some(share) = DISK_WIDTHS[index] else {
                        continue;
                    };
                    let room = share * sizes.text;

                    assert!(
                        width_of(head, sizes.small) <= room,
                        "{language:?}: 「{head}」 needs {} px of a {room} px column",
                        width_of(head, sizes.small)
                    );
                    assert!(
                        width_of(widest[index], sizes.text) <= room,
                        "{} needs {} px of a {room} px column",
                        widest[index],
                        width_of(widest[index], sizes.text)
                    );
                }
            }

            // What the fixed columns leave for the disks' identity, by the page's own arithmetic: the
            // scrollbar's width is taken off the top because it is taken off the panel, and the gaps
            // between columns are room the text never gets.
            //
            // The floor is what the column really has to hold: the NAS's own word for the disk — 存储空间 1,
            // five full-width characters — and not the model under it, which is as long as the drive is.
            let room = identity_room(sizes);
            let held = width_of("存储空间 1", sizes.text);

            assert!(
                room >= held,
                "the identity column is left {room} px for a label that wants {held} px"
            );

            // And the two lines that really go in it — the disk's use above, its name and model below —
            // are *cut* to that room, which is the thing this test exists for. The longest of each is a
            // real machine's: `mmcblk0boot0` is a kernel name and `WDC WD40EFRX-68N32N0` a model. The
            // check is on what [`format::fit`] returns, so it is the same cut the panel gets.
            for (text, size) in [
                ("系统安装", sizes.text),
                ("存储空间 1", sizes.text),
                ("/boot", sizes.text),
                ("mmcblk0boot0", sizes.small),
                ("sda · Samsung SSD 870 EVO 500GB", sizes.small),
                ("mmcblk0boot0 · WDC WD40EFRX-68N32N0", sizes.small),
            ] {
                let line = format::fit(text, room, size);

                assert!(
                    width_of(&line, size) <= room,
                    "「{line}」 needs {} px of a {room} px identity column",
                    width_of(&line, size)
                );
            }

            // The model is still *there* where the column has room for any of one: a column that cut
            // everything to an ellipsis would pass the check above and tell nobody anything.
            for (device, model) in [
                ("sda", "Samsung SSD 870 EVO 500GB"),
                ("nvme0n1", "WDC WD40EFRX-68N32N0"),
            ] {
                let line = identity_detail(device, model, room, sizes.small);

                assert!(
                    line.contains(" · "),
                    "「{line}」 has the name and none of the model"
                );
            }

            // A machine that reports no model — an eMMC, usually — is not left with a separator and
            // nothing after it.
            assert_eq!(
                identity_detail("mmcblk0boot0", "", room, sizes.small),
                format::fit("mmcblk0boot0", room, sizes.small)
            );
        }
    }

    /// The two tabs are the two halves of one page, and switching is a message rather than a rebuild.
    #[test]
    fn the_tabs_switch() {
        let mut monitor = monitor();
        monitor.update(Message::Open(0));

        assert_eq!(monitor.tab(), Tab::Overview);

        monitor.update(Message::Show(Tab::Disks));
        assert_eq!(monitor.tab(), Tab::Disks);
        let _ = monitor.view();
    }

    /// The terminal entry is the page *asking*, and nothing the page does: which app is on screen is the
    /// launcher's business — see [`Message::Terminal`] — so the tap leaves the page exactly as it was.
    /// What it does leave behind is the answer the launcher reaches in for: the machine the header is
    /// about. A standalone monitor, with no launcher above it, is where the tap really does nothing.
    #[test]
    fn the_terminal_entry_leaves_the_page_alone_and_leaves_the_machine_for_the_launcher() {
        let mut monitor = monitor();
        monitor.update(Message::Open(0));
        monitor.update(Message::Show(Tab::Disks));

        monitor.update(Message::Terminal);

        assert_eq!(monitor.screen(), Screen::Monitor, "the page did not move");
        assert_eq!(monitor.tab(), Tab::Disks, "nor did its half of it");
        assert_eq!(
            monitor.watched().map(|device| device.host.as_str()),
            Some("192.168.1.10"),
            "and the machine it is about is what the launcher opens a session on"
        );
        // The header carries the entry, which is drawn from this page's own view with no launcher in it.
        let _ = monitor.view();
    }
}
