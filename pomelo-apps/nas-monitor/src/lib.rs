//! The NAS monitor — another computer's numbers, on this panel — **a standard iced program**.
//!
//! Not a program that owns a loop: [`NasMonitor`] is a state with an `update` and a `view`, and
//! `main.rs` — a handful of lines — is what runs it in a window. On the board the launcher hosts it like
//! every other app, and the app cannot tell the difference.
//!
//! # What it is looking at
//!
//! A machine across the network: a NAS on the same LAN, watched over the board's Wi-Fi so that
//! "is the box all right" is answerable from the desk rather than from a browser on another machine.
//! Which is why the page is laid out like the NAS's own dashboard and not like a form: cards for the
//! things a person checks (the CPU, the memory, the network, the disks), and a table for the disks
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
//! The rings. The NAS's own dashboard draws them and this page draws bars, which is a decision worth
//! naming: the settings app's rings live in *its* crate, and a second copy of that drawing code is
//! exactly what `pomelo-widgets` exists to prevent. Moving it there is a small piece of work for two
//! apps instead of one, and it is not done. Also not here: the screen that *enters* the address and the
//! password. The app reads what the backend was given ([`NasMonitor::watched`]); writing it down is
//! [`NasBackend::remember`][pomelo_hal::traits::NasBackend::remember] and the screen that calls it.

mod format;
pub mod style;

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::widget::{column, container, row, scrollable, text, Column, Row, Space};
use iced::{Alignment, Color, Element, Length, Subscription, Theme};

use pomelo_hal::{Board, NasCredentials, NasFault, NasReading, NasStatus};
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

/// How long between looks at the NAS, when the credentials file does not say.
///
/// Five seconds: slow enough that the NAS's own disks are not the thing being measured, and fast enough
/// that somebody watching a copy of a file sees the numbers move. See
/// [`NasCredentials::interval`][pomelo_hal::NasCredentials::interval], which is where a person's own
/// answer is read from.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// Which half of the page is up.
///
/// Two and not one, because the two things a person wants are read differently: the cards are looked
/// *at* (a glance, from across a room) and the disk table is read (a row at a time, sitting down). On a
/// 480 px panel trying to do both at once would give neither enough room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Disks,
}

/// What the monitor reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// Time has passed: `now` is when the platform said so. See the crate documentation for what this
    /// does at which rate.
    Tick(Instant),
    /// A finger on one of the two tabs.
    Show(Tab),
}

/// The NAS monitor.
pub struct NasMonitor {
    board: Arc<Board>,
    /// The last reading this app has *seen*, which is the backend's snapshot as of the last tick.
    ///
    /// Held rather than asked for in `view`, for the reason the task viewer holds its reading: a frame
    /// costs a lock and a clone either way, and a state that is read once per tick is a state the tests
    /// can drive by hand.
    reading: NasReading,
    /// The machine the board is watching, as the backend has it. `None` until somebody points the board
    /// at one, and read once — a setting that changed under a running app is a setting nobody has a
    /// screen for yet.
    ///
    /// Read for the machine's name in the header and for the interval between looks; whether the board
    /// can watch a NAS *at all* is not this field's business — that arrives as
    /// [`NasFault::Unsupported`] in the reading, because it is the backend that knows.
    watched: Option<NasCredentials>,
    tab: Tab,
    /// When the board was last asked to look, for the cadence in
    /// [`NasMonitor::tick`].
    asked: Option<Instant>,
    preferences: SystemPreferences,
}

impl NasMonitor {
    /// The monitor, having asked the board what it is watching and taken its first look.
    pub fn new(board: Arc<Board>) -> Self {
        let watched = board.nas().watching();

        // The first look is asked for here rather than waiting for a tick: a page opened onto a machine
        // that is up should not spend its first interval blank. A board that cannot look at all answers
        // with a reading that says so, which is what the view draws.
        let _ = board.nas().refresh();

        let mut monitor = Self {
            board,
            reading: NasReading::default(),
            watched,
            tab: Tab::Overview,
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

    /// The last reading, as the page draws it. Read by the tests and by a host; a page gets it through
    /// [`NasMonitor::view`].
    pub fn reading(&self) -> &NasReading {
        &self.reading
    }

    /// The NAS this board is watching, if any.
    pub fn watched(&self) -> Option<&NasCredentials> {
        self.watched.as_ref()
    }

    /// Which half of the page is up.
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
        }
    }

    /// One frame's worth of clock: pull what has arrived, and ask for a new look when it is time.
    ///
    /// Two rates in one message, because the platform only sends one: the *pull* is every tick, so that
    /// an answer that arrived between two ticks is on screen at the next one, and the *ask* is on the
    /// interval. A first tick asks immediately — the constructor already has, and this is what keeps a
    /// window that has been open for a while from waiting out a stale interval.
    pub fn tick(&mut self, now: Instant) {
        self.pull();

        let due = match self.asked {
            Some(asked) => now.duration_since(asked) >= self.interval(),
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
        let sizes = style::Sizes::of(self.preferences.font_tier);
        let ink = style::ink(theme);
        let muted = style::muted(theme);
        let words = Words::of(self.preferences.language);

        let body: Element<'_, Message> = match &self.reading.status {
            // The board cannot do this at all. Not "the NAS is down" and not "nothing is set up":
            // nothing a person does to the NAS changes this screen, so it says what would.
            NasStatus::Offline(NasFault::Unsupported) => notice(&words.unsupported, sizes, muted),
            // Nothing to watch, which is a board somebody has not finished setting up.
            NasStatus::Unconfigured => notice(&words.unconfigured, sizes, muted),
            _ => match self.tab {
                Tab::Overview => self.overview(sizes, theme, ink, muted, &words),
                Tab::Disks => self.disks(sizes, ink, muted, &words),

            },
        };

        let page = column![self.header(sizes, theme, ink, muted, &words), body]
            .spacing(style::GAP)
            .padding(style::MARGIN)
            .width(Length::Fill)
            .height(Length::Fill);

        container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(style::surface(theme).into()),
                ..container::Style::default()
            })
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
            .unwrap_or_else(|| String::from("NAS"));

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

        let mut line = Row::with_children(vec![identity.into()])
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

        let tabs = row![
            self.tab_button(Tab::Overview, words.overview, sizes, ink, muted),
            self.tab_button(Tab::Disks, words.disks, sizes, ink, muted),
        ]
        .spacing(style::CELL_PAD);

        column![line, tabs].spacing(style::GAP * 0.6).into()
    }

    /// One of the two tabs: the page's own name on a button that says whether it is the one showing.
    fn tab_button(
        &self,
        tab: Tab,
        label: &str,
        sizes: style::Sizes,
        ink: Color,
        muted: Color,
    ) -> Element<'static, Message> {
        let showing = self.tab == tab;
        let accent = style::network();

        container(text(label.to_string()).size(sizes.small).color(if showing {
            ink
        } else {
            muted
        }))
        .padding([style::CELL_PAD * 0.6, style::CELL_PAD * 2.0])
        .style(move |_theme| container::Style {
            background: showing.then(|| accent.into()),
            border: iced::Border {
                radius: (sizes.small * 0.8).into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
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
                .unwrap_or_else(|| String::from("—")),
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
                .unwrap_or_else(|| String::from("—")),
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

        let read: u64 = self.reading.disks.iter().map(|disk| disk.read_per_sec).sum();
        let written: u64 = self
            .reading
            .disks
            .iter()
            .map(|disk| disk.write_per_sec)
            .sum();

        let network_card = card(
            words.network,
            network
                .map(|network| format::rate(network.received_per_sec))
                .unwrap_or_else(|| String::from("—")),
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

        let disk_card = card(
            words.disks_speed,
            format::rate(read),
            format!("{} {}", words.written, format::rate(written)),
            None,
            style::disk(),
            sizes,
            theme,
            ink,
            muted,
        );

        Column::with_children(vec![
            Row::with_children(vec![cpu_card, memory_card])
                .spacing(style::GAP)
                .into(),
            Row::with_children(vec![network_card, disk_card])
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

        for disk in &self.reading.disks {
            // The identity cell is two lines: the NAS's own word for what the disk is for, and under it
            // the kernel's name and the model. The model is what makes this cell wide, and a column of
            // its own would take the width of every other column put together — so it goes *under* the
            // use rather than beside it, which is what the eye reads anyway.
            let identity: Element<'_, Message> = Column::with_children(vec![
                cell(&disk.purpose, sizes.text, ink),
                cell(
                    &format!(
                        "{} · {}",
                        disk.device,
                        format::shorten(&disk.model, MODEL_CHARS)
                    ),
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
                            .unwrap_or_else(|| String::from("—")),
                        sizes.text,
                        ink,
                    ),
                    cell(
                        &disk
                            .temperature_c
                            .map(format::temperature)
                            .unwrap_or_else(|| String::from("—")),
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

    /// Reads the backend's snapshot into this state.
    fn pull(&mut self) {
        self.reading = self.board.nas().reading();
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

/// How much of a drive's model fits under its use, in characters. See [`format::shorten`], which is
/// what makes this a width rather than a hope.
const MODEL_CHARS: usize = 16;

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
/// A card with no share draws no bar, which is how the two cards whose subject is a *rate* — the
/// network and the disks — are told apart from the two whose subject is a share of a whole. A bar for
/// "8.4M/s" would be a bar with nothing to be full of.
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
    cpu: &'static str,
    memory: &'static str,
    network: &'static str,
    disks_speed: &'static str,
    received: &'static str,
    sent: &'static str,
    written: &'static str,
    temperature: &'static str,
    uptime: &'static str,
    connecting: &'static str,
    overview: &'static str,
    disks: &'static str,
    heads: [&'static str; 6],
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
                cpu: "CPU",
                memory: "内存",
                network: "网络",
                disks_speed: "硬盘读写",
                received: "接收",
                sent: "发送",
                written: "写入",
                temperature: "温度",
                uptime: "本次运行",
                connecting: "正在连接…",
                overview: "概览",
                disks: "硬盘",
                heads: ["硬盘", "容量", "温度", "繁忙", "读取", "写入"],
                unconfigured: "还没有配置 NAS：这台板子还不知道要看哪一台",
                unsupported: "本版固件还没有 SSH 客户端，暂时看不了 NAS",
                no_network: "板子没连上网络",
                unreachable: "连不上那台机器",
                authentication: "账号或密码不对",
                host_key: "主机密钥变了",
                protocol: "双方谈不到一起（协议不匹配）",
            },
            Language::English => Self {
                language,
                cpu: "CPU",
                memory: "Memory",
                network: "Network",
                disks_speed: "Disks",
                received: "in",
                sent: "out",
                written: "written",
                temperature: "temp",
                uptime: "up",
                connecting: "connecting…",
                overview: "Overview",
                disks: "Disks",
                heads: ["DISK", "SIZE", "TEMP", "BUSY", "READ", "WRITE"],
                unconfigured: "No NAS configured: this board has not been told which one to watch",
                unsupported: "This image has no SSH client yet, so it cannot watch a NAS",
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board watching the simulator's NAS, built the way a desktop test builds one.
    fn monitored() -> NasMonitor {
        let board = Arc::new(Board::simulated());

        board
            .nas()
            .remember(&NasCredentials {
                host: String::from("192.168.1.10"),
                port: 22,
                user: String::from("nasstat"),
                password: String::from("hunter2"),
                interval_secs: 5,
                host_key: None,
            })
            .unwrap();

        NasMonitor::new(board)
    }

    /// The page draws from a reading, and a reading arrives from the backend rather than from a frame:
    /// the app pulls on every tick and asks only on the interval.
    #[test]
    fn a_tick_pulls_every_time_and_asks_on_the_interval() {
        let mut monitor = monitored();

        // Opening the page asks for a look, and this simulator answers inside the call — so the state a
        // window shows on its first frame is a machine that has already answered.
        assert_eq!(monitor.reading().status, NasStatus::Online);
        assert_eq!(monitor.reading().disks.len(), 3);
        let _ = monitor.view();

        // A tick inside the interval pulls and does not ask again: one look per interval, not one per
        // frame, which is the whole of why the app has an `asked` at all.
        let start = monitor.asked.unwrap();
        monitor.tick(start + Duration::from_secs(1));

        assert_eq!(monitor.asked, Some(start), "no second look inside the interval");

        // And once the interval is up, it asks.
        monitor.tick(start + Duration::from_secs(6));

        assert_ne!(monitor.asked, Some(start), "a look when the interval is up");
        assert!(monitor.reading().age().is_some());

        let _ = monitor.view();
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
            SimAudio, SimImu, SimInput, SimMic, SimPower, SimStorage, SimSystem, SimWeb, SimWifi,
        };
        use pomelo_hal::traits::NasBackend;

        /// The device's placeholder, in one place: it refuses the setting and answers a reading that
        /// says "not this board", rather than one that says "nothing configured".
        struct Refusing;

        impl NasBackend for Refusing {
            fn watching(&self) -> Option<NasCredentials> {
                None
            }

            fn remember(&mut self, _credentials: &NasCredentials) -> Result<(), HalError> {
                Err(HalError::NotSupported)
            }

            fn forget(&mut self) -> Result<(), HalError> {
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
        use pomelo_widgets::preferences::FontSizeTier;

        /// How wide `text` is at `size`: one em per full-width character, under two thirds of one per
        /// narrow one. An estimate, and deliberately pessimistic — see the task viewer's note.
        ///
        /// `°` counts as narrow, and it is the reason this clause exists: it is not ASCII and it is not
        /// a full-width character either, and a `38°C` charged a whole em for its degree sign is a check
        /// that fails on a temperature that fits.
        fn width_of(text: &str, size: f32) -> f32 {
            text.chars()
                .map(|c| if c.is_ascii() || c == '°' { size * 0.62 } else { size })
                .sum()
        }

        // The three tiers this table is laid out for, which is not all four of them: at the largest,
        // eight columns of Chinese-labelled numbers do not fit in 480 px whatever the widths are — the
        // heads alone take a third of the panel — and a table that pretends otherwise would clip rather
        // than say so. What the largest tier needs is fewer columns, which is a decision about the page
        // and not about the arithmetic, so it is not made here.
        for tier in [
            FontSizeTier::ExtraSmall,
            FontSizeTier::Small,
            FontSizeTier::Standard,
        ] {
            let sizes = style::Sizes::of(tier);

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
                        "{tier:?} {language:?}: 「{head}」 needs {} px of a {room} px column",
                        width_of(head, sizes.small)
                    );
                    assert!(
                        width_of(widest[index], sizes.text) <= room,
                        "{tier:?}: {} needs {} px of a {room} px column",
                        widest[index],
                        width_of(widest[index], sizes.text)
                    );
                }
            }

            // What the fixed columns leave for the disks' identity, with the scrollbar's width taken
            // off the top because it is taken off the panel: see [`style::SCROLLBAR`].
            //
            // The floor is what the column really has to hold: the NAS's own word for the disk — 存储空间 1,
            // five full-width characters at the small size — and not the model under it, which is as long
            // as the drive is and is cut to fit instead.
            let held = width_of("存储空间 1", sizes.text);
            let fixed: f32 = DISK_WIDTHS.iter().flatten().map(|share| share * sizes.text).sum();
            let gaps = style::CELL_PAD * (DISK_WIDTHS.len() - 1) as f32;
            let identity = style::PANEL - style::MARGIN * 2.0 - style::SCROLLBAR - fixed - gaps;

            assert!(
                identity >= held,
                "{tier:?}: the identity column is left {identity} px for a label that wants {held} px"
            );
        }
    }

    /// The two tabs are the two halves of one page, and switching is a message rather than a rebuild.
    #[test]
    fn the_tabs_switch() {
        let mut monitor = monitored();

        assert_eq!(monitor.tab(), Tab::Overview);

        monitor.update(Message::Show(Tab::Disks));
        assert_eq!(monitor.tab(), Tab::Disks);
        let _ = monitor.view();
    }
}
