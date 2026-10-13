//! The overview: the whole box on one screen.
//!
//! Every other app on this board is about one thing. This one is about the board, and it is the page
//! somebody opens when they cannot remember *which* of those things is wrong: the charge, the radio,
//! the two filesystems, the memory, and the machine next door, in the order they matter.
//!
//! # The readings are read, the drawing is a function of them
//!
//! [`Readings`] is gathered by one pass over the board's backends ([`Readings::of`]) and the page is
//! drawn from *that*, never from the board. So the drawing can be tested against readings written out
//! by hand — including the ones a simulated board will never produce, like a server that answered and
//! then went away — and the reading can be tested against a simulated board with no window in sight.
//!
//! # Only the two slow questions are asked twice
//!
//! A card in the slot is an SDMMC transaction and a NAS is an SSH login, so neither is asked on every
//! tick: [`Readings::again`] *starts* both when the page opens and when the button at the top is
//! tapped, and the answers arrive at the next read. Everything else is a lock and a read.
//!
//! # The page is told when to look
//!
//! On the board there is no timer to ask for: the launcher's own pump sends [`Message::Tick`] once a
//! second, and only while this is the page that is up. A *window* has frames instead —
//! [`Overview::subscription`] — and those come sixty times a second, so [`Overview::tick`] throws
//! away everything inside [`SAMPLE`].

mod format;
mod read;
pub mod style;

pub use read::{Battery, Readings};

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, container, row, scrollable, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Subscription, Theme};

use pomelo_hal::{Board, MemoryInfo, NasFault, NasReading, NasStatus, VolumeInfo, WifiState};
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

/// How close together two looks at the board may be.
///
/// The board's pump ticks once a second, so this never bites there; a window's frames are sixty a
/// second, and this is the number that turns them into about one look a second. Just under a second
/// rather than exactly one, so the pump's steady beat is never thrown away by a few milliseconds of
/// jitter.
pub const SAMPLE: Duration = Duration::from_millis(900);

/// What the app reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// Time has passed: read the board again.
    ///
    /// Carries the moment it arrived, because a window's frames are sixty a second and this page has
    /// no business taking five locks that often — see [`Overview::tick`].
    Tick(Instant),
    /// A finger asked the two slow backends to look again: the card in the slot, and the server.
    CheckAgain,
}

/// The overview.
pub struct Overview {
    board: Arc<Board>,
    preferences: SystemPreferences,
    /// Everything the page is drawn from. Replaced whole on every read, which is what makes the
    /// drawing a function of a value rather than of a board.
    readings: Readings,
    /// When the board was last read. `None` until the first read, which is how the first tick is
    /// never thrown away however soon it follows the app being opened.
    last: Option<Instant>,
    /// Whether a look has been asked for and its answer has not been read yet.
    ///
    /// The button says so while it is true. It is not a spinner and not an animation: it is the
    /// honest statement that the answer on screen is the old one, and it goes away when a read
    /// actually happens.
    looking: bool,
}

impl Overview {
    /// A page for this board, read once and then asked to look again.
    ///
    /// It reads before it asks, so the first frame has whatever the backends already knew rather than
    /// five dashes: on the board the storage backend has usually been probed by the time the launcher
    /// has drawn a tile, and the NAS keeps the last answer it was given.
    pub fn new(board: Arc<Board>) -> Self {
        let readings = Readings::of(&board);
        Readings::again(&board);

        Self {
            board,
            preferences: SystemPreferences::default(),
            readings,
            last: None,
            looking: true,
        }
    }

    /// The page as it was last read.
    pub fn readings(&self) -> &Readings {
        &self.readings
    }

    /// Whether a fresh look is still on its way.
    pub fn looking(&self) -> bool {
        self.looking
    }

    /// The system's settings, which the page takes its colours and its words from.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// The system told the app what it is set to.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// Handle one message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.tick(now),
            Message::CheckAgain => self.check(),
        }
    }

    /// Read the board, unless a look happened a moment ago.
    ///
    /// The throttle is not a courtesy to the backends, which are locks over values already in memory;
    /// it is because at sixty frames a second "read the board" runs sixty times per repaint, and a
    /// page that reads five mutexes per frame is a page that drops frames on a board that has none to
    /// spare.
    pub fn tick(&mut self, now: Instant) {
        if let Some(last) = self.last {
            if now.saturating_duration_since(last) < SAMPLE {
                return;
            }
        }

        self.last = Some(now);
        self.readings = Readings::of(&self.board);
        // The look asked for by `check` has now been read, whatever it turned out to be: this is the
        // moment the page stops saying "checking".
        self.looking = false;
    }

    /// Ask the two slow backends to look again, keeping the readings until they answer.
    ///
    /// The order matters and is the whole trick: ask first, then read. The read that follows is the
    /// old answer, which is why `looking` stays true — the new one shows up at the next [`tick`],
    /// which is what keeps a two-second filesystem probe off the frame that asked for it.
    ///
    /// [`tick`]: Overview::tick
    pub fn check(&mut self) {
        Readings::again(&self.board);
        self.readings = Readings::of(&self.board);
        self.looking = true;
    }

    /// The theme: the two palettes every other page on this box is drawn on.
    ///
    /// The dark one is the box's own black rather than a near-black of its own, for the reason the
    /// clock gives: the panel sits inside a black bezel and only an exact black leaves that edge
    /// undrawn. The page's *own* colours are in [`style`], which is where a card's slab and a bar's
    /// green come from; these two are what everything the page does not colour is painted on.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: ThemeMode::Dark.background(),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: Color::from_rgb8(242, 242, 247),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// How the page hears about time passing.
    ///
    /// Only a window uses this. On the board the launcher's pump is the clock, and it is a better one:
    /// it ticks while this page is up and stops when it is not.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::window::frames().map(|_| Message::Tick(Instant::now()))
    }
}

// ---- parts of the page ----

/// One level bar: a track, and a fill that is a *share* of it.
///
/// The fill is a fraction of the track's own width rather than a width in pixels, which is why the
/// same bar is right at every font tier and in any container it is put in.
fn bar(share: f32, fill: Color, theme: ThemeMode) -> Element<'static, Message> {
    // Thousandths of the track, as portions of a row. The number is a `u16` where 1000 is the whole
    // bar, so the fill and the space left over always add up to the full width and the bar cannot
    // disagree with the percentage printed above it by a rounding step.
    let portion = (share.clamp(0.0, 1.0) * 1000.0).round() as u16;

    let mut contents = Row::new().width(Length::Fill).height(Length::Fill);

    if portion > 0 {
        contents = contents.push(
            container(Space::new())
                .width(Length::FillPortion(portion))
                .height(Length::Fill)
                .style(move |_theme| capsule(fill)),
        );
    }

    if portion < 1000 {
        contents = contents.push(Space::new().width(Length::FillPortion(1000 - portion)));
    }

    container(contents)
        .width(Length::Fill)
        .height(Length::Fixed(style::BAR))
        .style(move |_theme| capsule(style::track(theme)))
        .into()
}

/// A filled, rounded rectangle: the shape a bar's track and its fill are both made of.
fn capsule(colour: Color) -> container::Style {
    container::Style {
        background: Some(colour.into()),
        border: Border {
            radius: style::BAR_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// One card: its title, and whatever the card has to say under it.
fn card<'a>(
    title: &'a str,
    icon: Icon,
    tint: Color,
    body: Element<'a, Message>,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let heading = row![
        text(icon.glyph())
            .font(pomelo_material_symbols::font())
            .size(sizes.text)
            .color(tint),
        text(title).size(sizes.text).color(style::muted(theme)),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .height(Length::Fixed(sizes.header));

    container(Column::with_children(vec![heading.into(), body]).spacing(style::GAP / 2.0))
        .width(Length::Fill)
        .padding(style::GAP)
        .style(move |_theme| container::Style {
            background: Some(style::card(theme).into()),
            border: Border {
                radius: style::RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A label on the left and a value on the right, on one line.
fn labelled<'a>(
    label: &'a str,
    value: String,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    row![
        text(label).size(sizes.small).color(style::muted(theme)),
        Space::new().width(Length::Fill),
        text(value).size(sizes.small).color(style::ink(theme)),
    ]
    .width(Length::Fill)
    .into()
}

/// The secondary line under a reading: its facts, separated by a middle dot, or a dash when the board
/// gave none.
///
/// One string and not a row of labels, so that the parts close up when a fact is missing instead of
/// leaving a hole where the volt reading would have been.
fn facts_line(facts: &[String]) -> String {
    if facts.is_empty() {
        format::missing()
    } else {
        facts.join(" · ")
    }
}

// ---- what the page says ----

/// Everything the page writes, in the language the system is set to.
///
/// Every field is a `&'static str` so that a card can be built from a borrowed `Words` without the
/// strings outliving it: the elements the page returns cannot borrow this struct, because it is a
/// local of `view` and the page outlives it.
struct Words {
    title: &'static str,
    check: &'static str,
    battery: &'static str,
    charging: &'static str,
    no_gauge: &'static str,
    wifi: &'static str,
    connected: &'static str,
    connecting: &'static str,
    scanning: &'static str,
    disconnected: &'static str,
    storage: &'static str,
    internal: &'static str,
    card_slot: &'static str,
    no_card: &'static str,
    no_volume: &'static str,
    memory: &'static str,
    heap: &'static str,
    psram: &'static str,
    no_pool: &'static str,
    server: &'static str,
    online: &'static str,
    unconfigured: &'static str,
    checking: &'static str,
    cpu: &'static str,
    network: &'static str,
    uptime: &'static str,
    disks: &'static str,
    received: &'static str,
    sent: &'static str,
    no_network: &'static str,
    unreachable: &'static str,
    authentication: &'static str,
    host_key: &'static str,
    protocol: &'static str,
    unsupported: &'static str,
}

impl Words {
    fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                title: "概览",
                check: "重新检测",
                battery: "电池",
                charging: "充电中",
                no_gauge: "没有电量计",
                wifi: "无线网络",
                connected: "已连接",
                connecting: "连接中",
                scanning: "搜索中",
                disconnected: "未连接",
                storage: "存储",
                internal: "内部存储",
                card_slot: "存储卡",
                no_card: "未插入",
                no_volume: "未挂载",
                memory: "内存",
                heap: "堆",
                psram: "PSRAM",
                no_pool: "没有这块内存",
                server: "服务器",
                online: "在线",
                unconfigured: "未配置",
                checking: "检测中",
                cpu: "处理器",
                network: "网络",
                uptime: "运行时间",
                disks: "硬盘",
                received: "收",
                sent: "发",
                no_network: "没有网络接口",
                unreachable: "无法连接",
                authentication: "密钥被拒绝",
                host_key: "主机密钥不认识",
                protocol: "说不通",
                unsupported: "本机没有 SSH",
            },
            Language::English => Self {
                title: "Overview",
                check: "Check again",
                battery: "Battery",
                charging: "charging",
                no_gauge: "no fuel gauge",
                wifi: "Wi-Fi",
                connected: "Connected",
                connecting: "Connecting",
                scanning: "Scanning",
                disconnected: "Disconnected",
                storage: "Storage",
                internal: "Internal",
                card_slot: "SD card",
                no_card: "none",
                no_volume: "not mounted",
                memory: "Memory",
                heap: "Heap",
                psram: "PSRAM",
                no_pool: "no such pool",
                server: "Server",
                online: "Online",
                unconfigured: "Not set up",
                checking: "Checking",
                cpu: "CPU",
                network: "Network",
                uptime: "Uptime",
                disks: "Disks",
                received: "rx",
                sent: "tx",
                no_network: "no network stats",
                unreachable: "unreachable",
                authentication: "key rejected",
                host_key: "host key is not the one we know",
                protocol: "does not understand us",
                unsupported: "this box has no SSH client",
            },
        }
    }

    /// What a fault reads as. The message the host sent is not shown: it is a paragraph, this is a
    /// card, and *which* of the six things went wrong is the part a person acts on.
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

    fn nas(&self, status: &NasStatus) -> &'static str {
        match status {
            NasStatus::Unconfigured => self.unconfigured,
            NasStatus::Connecting => self.checking,
            NasStatus::Online => self.online,
            NasStatus::Offline(fault) => self.fault(*fault),
        }
    }
}

// ---- the page ----

impl Overview {
    /// The panel, drawn from the readings the app is holding.
    ///
    /// Nothing here touches the board: every number comes out of `self.readings`, so the drawing is
    /// testable against readings written out by hand and cannot disagree with what a test asserts.
    pub fn view(&self) -> Element<'_, Message> {
        let theme = self.preferences.theme;
        let sizes = style::Sizes::of(self.preferences.font_tier);
        let words = Words::of(self.preferences.language);

        let cards = Column::with_children(vec![
            self.battery_card(sizes, theme, &words),
            self.wifi_card(sizes, theme, &words),
            self.storage_card(sizes, theme, &words),
            self.memory_card(sizes, theme, &words),
            self.server_card(sizes, theme, &words),
        ])
        .spacing(style::GAP)
        .width(Length::Fill);

        let page = Column::with_children(vec![
            self.header(sizes, theme, &words),
            // No scrollbar: it is drawn over the right edge of the cards rather than beside them, and
            // its rail has no corner to it, so it squares off the right side of every card and runs
            // the last ten pixels to the edge of the panel. The panel has a finger and not a pointer.
            scrollable(cards)
                .direction(Direction::Vertical(Scrollbar::hidden()))
                .width(Length::Fill)
                .into(),
        ])
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

    /// The title, and the one control on the page.
    fn header(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        row![
            text(words.title).size(sizes.text).color(style::ink(theme)),
            Space::new().width(Length::Fill),
            self.check_button(sizes, theme, words),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
    }

    /// The button that asks the slow backends to look again.
    ///
    /// It says "checking" while a look is on its way, which is not an animation: the readings on
    /// screen are the ones from *before* the tap until the answer arrives, and a control that looked
    /// idle until then would be a control that lied about what the page was showing.
    fn check_button(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        let (label, tint) = if self.looking {
            (words.checking, style::warn())
        } else {
            (words.check, style::muted(theme))
        };

        button(
            row![
                text(Icon::REFRESH.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.small)
                    .color(tint),
                text(label).size(sizes.small).color(tint),
            ]
            .spacing(style::GAP / 2.0)
            .align_y(Alignment::Center),
        )
        .padding([6.0, 12.0])
        .on_press(Message::CheckAgain)
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Pressed | button::Status::Hovered => style::card(theme),
                _ => Color::TRANSPARENT,
            };

            button::Style {
                background: Some(background.into()),
                text_color: style::ink(theme),
                border: Border {
                    radius: style::RADIUS.into(),
                    ..Border::default()
                },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
    }

    /// The charge: the one number worth reading from across a desk.
    fn battery_card(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        let Some(battery) = self.readings.battery else {
            return card(
                words.battery,
                Icon::BATTERY_FULL,
                style::muted(theme),
                text(words.no_gauge)
                    .size(sizes.small)
                    .color(style::muted(theme))
                    .into(),
                sizes,
                theme,
            );
        };

        let percent = battery.percent as f32;
        let tint = style::charge(percent);

        let mut facts = Vec::new();
        if let Some(millivolts) = battery.voltage_mv {
            facts.push(format::volts(millivolts));
        }
        if let Some(celsius) = battery.temperature_c {
            facts.push(format::degrees(celsius));
        }
        if battery.charging {
            facts.push(words.charging.to_string());
        }

        let body = Column::with_children(vec![
            text(format::percent(percent))
                .size(sizes.reading)
                .color(tint)
                .into(),
            bar(format::fraction(percent), tint, theme),
            text(facts_line(&facts))
                .size(sizes.small)
                .color(style::muted(theme))
                .into(),
        ])
        .spacing(style::GAP / 2.0)
        .width(Length::Fill);

        card(
            words.battery,
            Icon::BATTERY_FULL,
            tint,
            body.into(),
            sizes,
            theme,
        )
    }

    /// The radio: whose network, and how well.
    fn wifi_card(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        let wifi = &self.readings.wifi;

        let (state, tint) = match wifi.state {
            WifiState::Connected => (words.connected, style::accent()),
            WifiState::Connecting => (words.connecting, style::warn()),
            WifiState::Scanning => (words.scanning, style::warn()),
            WifiState::Disconnected => (words.disconnected, style::muted(theme)),
        };

        // A connected board is named by the network it is on; one that is not is named by what it is
        // doing instead, because an SSID it is not associated with says nothing.
        let headline = if wifi.state == WifiState::Connected && !wifi.ssid.is_empty() {
            wifi.ssid.clone()
        } else {
            state.to_string()
        };

        let mut facts = Vec::new();
        if wifi.state == WifiState::Connected {
            if !wifi.ip.is_empty() {
                facts.push(wifi.ip.clone());
            }
            facts.push(format!("{}/4", wifi.signal_bars()));
        }

        let body = Column::with_children(vec![
            text(headline).size(sizes.text).color(tint).into(),
            text(facts_line(&facts))
                .size(sizes.small)
                .color(style::muted(theme))
                .into(),
        ])
        .spacing(style::GAP / 2.0)
        .width(Length::Fill);

        card(words.wifi, Icon::WIFI, tint, body.into(), sizes, theme)
    }

    /// The two filesystems: the one in the flash, and the one in the slot.
    fn storage_card(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        let readings = &self.readings;

        let body = Column::with_children(vec![
            volume_row(
                words.internal,
                readings.internal(),
                words.no_volume,
                sizes,
                theme,
            ),
            volume_row(
                words.card_slot,
                readings.card(),
                words.no_card,
                sizes,
                theme,
            ),
        ])
        .spacing(style::GAP)
        .width(Length::Fill);

        card(
            words.storage,
            Icon::STORAGE,
            style::accent(),
            body.into(),
            sizes,
            theme,
        )
    }

    /// The heap, and the pool beside it.
    fn memory_card(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        let body = Column::with_children(vec![
            memory_row(words.heap, self.readings.heap, words.no_pool, sizes, theme),
            memory_row(words.psram, self.readings.psram, words.no_pool, sizes, theme),
        ])
        .spacing(style::GAP)
        .width(Length::Fill);

        card(
            words.memory,
            Icon::MEMORY,
            style::accent(),
            body.into(),
            sizes,
            theme,
        )
    }

    /// The machine next door, as the last look found it.
    ///
    /// The status line is on every state of this card and the numbers are only on one, because they
    /// answer different questions: "is it there" is asked first, and a card that showed yesterday's
    /// CPU beside today's red *unreachable* would be worse than an empty one.
    fn server_card(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        words: &Words,
    ) -> Element<'_, Message> {
        let nas = &self.readings.nas;

        let tint = match nas.status {
            NasStatus::Online => style::good(),
            NasStatus::Connecting => style::warn(),
            NasStatus::Offline(_) => style::bad(),
            NasStatus::Unconfigured => style::muted(theme),
        };

        let mut lines: Vec<Element<'_, Message>> =
            vec![text(words.nas(&nas.status))
                .size(sizes.text)
                .color(tint)
                .into()];

        if nas.status == NasStatus::Online {
            lines.push(labelled(words.cpu, cpu_facts(nas), sizes, theme));
            lines.push(labelled(words.network, network_facts(nas, words), sizes, theme));

            if let Some(memory) = nas.memory {
                lines.push(labelled(
                    words.memory,
                    format!(
                        "{} / {}",
                        format::bytes(memory.used_bytes()),
                        format::bytes(memory.total_bytes)
                    ),
                    sizes,
                    theme,
                ));
            }

            if let Some(uptime) = nas.uptime {
                lines.push(labelled(words.uptime, format::duration(uptime), sizes, theme));
            }

            lines.push(labelled(words.disks, nas.disks.len().to_string(), sizes, theme));
        }

        let body = Column::with_children(lines)
            .spacing(style::GAP / 2.0)
            .width(Length::Fill);

        card(words.server, Icon::DNS, tint, body.into(), sizes, theme)
    }
}

/// The NAS's CPU as one line: its usage, and its temperature when the machine will say.
fn cpu_facts(nas: &NasReading) -> String {
    let Some(cpu) = nas.cpu else {
        return format::missing();
    };

    match cpu.temperature_c {
        Some(celsius) => format!(
            "{} · {}",
            format::percent(cpu.usage_percent),
            format::degrees(celsius)
        ),
        None => format::percent(cpu.usage_percent),
    }
}

/// What the NAS's network did, as one line.
///
/// The two rates are named `rx` and `tx` in both languages, because the abbreviations are what the
/// tools at the other end print and a translation of them would be a second name for one thing.
fn network_facts(nas: &NasReading, words: &Words) -> String {
    let Some(net) = nas.network else {
        return format::missing();
    };

    format!(
        "{} {}/s · {} {}/s",
        words.received,
        format::bytes(net.received_per_sec),
        words.sent,
        format::bytes(net.sent_per_sec)
    )
}

/// One filesystem on the storage card: its name, how full it is, and a bar.
///
/// An absent volume is a row and not nothing: a slot with no card in it is a fact about the box, and
/// the row that says so is the row the card's own numbers will appear in when it is put in.
fn volume_row<'a>(
    label: &'static str,
    volume: Option<&'a VolumeInfo>,
    empty: &'static str,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let Some(volume) = volume else {
        return row![
            text(label).size(sizes.small).color(style::muted(theme)),
            Space::new().width(Length::Fill),
            text(empty).size(sizes.small).color(style::muted(theme)),
        ]
        .width(Length::Fill)
        .into();
    };

    let share = volume.used_percent();

    Column::with_children(vec![
        row![
            text(label).size(sizes.small).color(style::muted(theme)),
            Space::new().width(Length::Fill),
            text(format!(
                "{} / {}",
                format::bytes(volume.used_bytes()),
                format::bytes(volume.total_bytes)
            ))
            .size(sizes.small)
            .color(style::ink(theme)),
        ]
        .width(Length::Fill)
        .into(),
        bar(format::fraction(share), style::usage(share), theme),
    ])
    .spacing(style::GAP / 2.0)
    .width(Length::Fill)
    .into()
}

/// One pool of memory: its name, how much is handed out, and a bar.
fn memory_row(
    label: &'static str,
    pool: Option<MemoryInfo>,
    empty: &'static str,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'static, Message> {
    let Some(pool) = pool else {
        return row![
            text(label).size(sizes.small).color(style::muted(theme)),
            Space::new().width(Length::Fill),
            text(empty).size(sizes.small).color(style::muted(theme)),
        ]
        .width(Length::Fill)
        .into();
    };

    let share = pool.used_percent();

    Column::with_children(vec![
        row![
            text(label).size(sizes.small).color(style::muted(theme)),
            Space::new().width(Length::Fill),
            text(format!(
                "{} / {}",
                format::bytes(pool.used_bytes()),
                format::bytes(pool.total_bytes)
            ))
            .size(sizes.small)
            .color(style::ink(theme)),
        ]
        .width(Length::Fill)
        .into(),
        bar(format::fraction(share), style::usage(share), theme),
    ])
    .spacing(style::GAP / 2.0)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    use pomelo_hal::{NasCpu, NasNetwork};
    use pomelo_widgets::preferences::FontSizeTier;

    /// An app on the simulated board, which answers every question this page asks.
    fn app() -> Overview {
        Overview::new(Arc::new(Board::simulated()))
    }

    /// Every setting the system can be in, drawn.
    #[test]
    fn the_page_draws_in_every_setting() {
        let mut app = app();

        for language in [Language::Chinese, Language::English] {
            for theme in [ThemeMode::Dark, ThemeMode::Light] {
                for font_tier in [
                    FontSizeTier::ExtraSmall,
                    FontSizeTier::Small,
                    FontSizeTier::Standard,
                    FontSizeTier::Large,
                ] {
                    app.set_preferences(SystemPreferences {
                        language,
                        theme,
                        font_tier,
                    });
                    let _ = app.view();
                }
            }
        }
    }

    /// Opening the page reads the board, so the first frame has readings rather than five dashes.
    #[test]
    fn opening_the_page_reads_the_board() {
        let app = app();

        assert!(app.readings().battery.is_some(), "the simulator has a gauge");
        assert!(app.readings().internal().is_some());
        assert!(app.readings().heap.is_some());
        assert!(
            app.looking(),
            "and it has asked the two slow backends to look"
        );
    }

    /// The first tick reads whatever the gap, and the ones inside [`SAMPLE`] do not.
    ///
    /// `looking` is the app's own record of whether a look it asked for has been read back, so it is
    /// the difference between a read and a skipped tick — which a person sees as the button saying
    /// "checking" for a second longer than it needed to.
    #[test]
    fn a_flood_of_ticks_reads_the_board_once() {
        let mut app = app();
        let start = Instant::now();

        app.tick(start);
        assert!(!app.looking(), "the first tick is never thrown away");

        app.check();
        assert!(app.looking(), "asking for a look leaves the old answer up");

        app.tick(start + Duration::from_millis(10));
        assert!(
            app.looking(),
            "a tick inside the sample is not worth five locks"
        );

        app.tick(start + SAMPLE);
        assert!(!app.looking(), "and one past it is");
    }

    /// A board that has not been told about a NAS says so, and that is not a failure.
    #[test]
    fn a_server_that_was_never_set_up_is_not_a_fault() {
        let app = app();
        let words = Words::of(Language::English);

        assert_eq!(app.readings().nas.status, NasStatus::Unconfigured);
        assert_eq!(words.nas(&app.readings().nas.status), "Not set up");
    }

    /// Every fault has words of its own, in both languages, and no two faults read the same.
    #[test]
    fn every_fault_has_its_own_words() {
        let faults = [
            NasFault::NoNetwork,
            NasFault::Unreachable,
            NasFault::Authentication,
            NasFault::HostKeyChanged,
            NasFault::Protocol,
            NasFault::Unsupported,
        ];

        for language in [Language::Chinese, Language::English] {
            let words = Words::of(language);
            let read: Vec<&str> = faults.iter().map(|fault| words.fault(*fault)).collect();

            for (index, one) in read.iter().enumerate() {
                assert!(!one.is_empty(), "{language:?}: a fault reads as nothing");

                for other in read.iter().skip(index + 1) {
                    assert_ne!(one, other, "{language:?}: two faults read the same");
                }
            }
        }
    }

    /// A server that answered and then went away is drawn as the fault, and the page survives it.
    #[test]
    fn a_server_that_went_away_says_why() {
        let mut app = app();
        app.readings.nas = NasReading {
            status: NasStatus::Offline(NasFault::Authentication),
            ..NasReading::default()
        };

        let words = Words::of(Language::English);
        assert_eq!(words.nas(&app.readings.nas.status), "key rejected");

        let _ = app.view();
    }

    /// A NAS that answered is read out: a CPU share and its temperature on one line, and two rates on
    /// the next.
    #[test]
    fn an_online_server_is_read_out() {
        let nas = NasReading {
            status: NasStatus::Online,
            cpu: Some(NasCpu {
                usage_percent: 12.4,
                temperature_c: Some(48.0),
            }),
            network: Some(NasNetwork {
                sent_per_sec: 1024,
                received_per_sec: 2048,
            }),
            uptime: Some(Duration::from_secs(2 * 86_400 + 4 * 3600)),
            ..NasReading::default()
        };

        assert_eq!(cpu_facts(&nas), "12% · 48°C");

        let words = Words::of(Language::English);
        let network = network_facts(&nas, &words);
        assert!(network.starts_with("rx "), "{network}");
        assert!(network.contains(" · tx "), "{network}");
        assert!(network.ends_with("/s"), "{network}");
    }

    /// A machine that will not report its temperature still gets its CPU read out, rather than a dash
    /// for the pair.
    #[test]
    fn a_server_without_a_thermometer_still_reports_its_cpu() {
        let nas = NasReading {
            cpu: Some(NasCpu {
                usage_percent: 7.0,
                temperature_c: None,
            }),
            ..NasReading::default()
        };

        assert_eq!(cpu_facts(&nas), "7%");
        assert_eq!(cpu_facts(&NasReading::default()), format::missing());
    }

    /// A board that has read nothing draws, and every row is a dash rather than a zero.
    #[test]
    fn a_page_that_has_read_nothing_still_draws() {
        let mut app = app();
        app.readings = Readings::default();

        let _ = app.view();
        assert_eq!(facts_line(&[]), format::missing());
    }

    /// The facts line closes up rather than leaving a hole where a missing fact would have been.
    #[test]
    fn the_facts_line_closes_up() {
        assert_eq!(facts_line(&[]), format::missing());
        assert_eq!(facts_line(&["4.10 V".to_string()]), "4.10 V");
        assert_eq!(
            facts_line(&["4.10 V".to_string(), "41°C".to_string()]),
            "4.10 V · 41°C"
        );
    }

    /// Checking again asks both slow backends and says so, and the readings stay up while it does.
    #[test]
    fn checking_again_keeps_the_readings_and_says_looking() {
        let mut app = app();
        app.tick(Instant::now());
        assert!(!app.looking());

        let before = app.readings().internal().map(|v| v.mount_point.clone());
        app.check();

        assert!(app.looking(), "the page says a fresh look is on its way");
        assert_eq!(
            app.readings().internal().map(|v| v.mount_point.clone()),
            before,
            "and shows the answer it already had rather than nothing"
        );
    }

    /// The battery and the disks are coloured by opposite readings of the same number, which is what
    /// keeps a flat cell and a full disk from being drawn the same way.
    #[test]
    fn the_battery_and_the_disks_disagree_about_what_is_bad() {
        assert!(app().readings().battery.is_some(), "the simulator has a gauge");

        assert_eq!(style::charge(95.0), style::good());
        assert_eq!(style::usage(95.0), style::bad());
        assert_ne!(
            style::charge(90.0),
            style::usage(90.0),
            "a full cell and a full disk are not the same news"
        );
    }
}
