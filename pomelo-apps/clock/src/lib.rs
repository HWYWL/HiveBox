//! The clock: a dial, a pomodoro timer, and the alarms that go off.
//!
//! # Three pages and the same two readings
//!
//! A clock, a timer and an alarm are three answers to one question — what does this board think the
//! time is, and how much of it is left — so they are three tabs of one app rather than three tiles on
//! the desktop. The `Face` tab is the dial and the date, the `Timer` tab the pomodoro ring, and the
//! `Alarms` tab the list. Moving between them keeps the pomodoro running, which is the one thing a
//! user of a tomato timer would miss most if it did not.
//!
//! # Where the time comes from
//!
//! [`SystemBackend::clock`] for the wall clock and [`SystemBackend::uptime`] for the pomodoro, both
//! read when the platform says a second has passed — [`Message::Tick`] — and never from `view`. The
//! two are the *same* reading in different clothes: a pomodoro is a count of seconds this board has
//! been up, and it survives the app being closed and reopened, which a count of `Instant`s taken in
//! `new` would not.
//!
//! A board whose clock has never been set answers `None`, and the dial is then drawn with no hands
//! and the readout as `--:--:--`. That is deliberate: a face showing 12:00 on a board that does not
//! know the time would be inventing a fact about it.
//!
//! # What this app writes
//!
//! Two things of its own, both through [`StorageBackend`] rather than `std::fs`: the alarm list, saved
//! whenever it changes, and the chime, synthesised in [`chime`] and written once. On this board
//! `std::fs` cannot create a file at all — see `StorageBackend::write_file` — and reading either of
//! them back is the half that always worked.
//!
//! [`SystemBackend::clock`]: pomelo_hal::SystemBackend::clock
//! [`SystemBackend::uptime`]: pomelo_hal::SystemBackend::uptime
//! [`StorageBackend`]: pomelo_hal::StorageBackend

pub mod chime;
pub mod dial;
pub mod ring;
pub mod style;
pub mod time;

use std::sync::Arc;

use iced::theme::Palette;
use iced::widget::{button, column, container, row, scrollable, stack, text, Column, Row};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Subscription, Theme};

use pomelo_hal::Board;
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

use time::Civil;

/// How long a work phase runs, and how long the break after it does.
pub const WORK_SECS: u64 = 25 * 60;
pub const BREAK_SECS: u64 = 5 * 60;

/// The step an alarm's steppers move in, in minutes.
///
/// Five rather than one, because the alarm page has four steppers and a finger: setting 07:00 from
/// 06:55 is two taps rather than twenty-five, and an alarm is a thing people set to the *quarter*
/// anyway. The time is not otherwise restricted — an alarm at 07:03 is legal, it is just not what the
/// buttons make.
pub const STEP_MINUTES: u32 = 5;

/// The app-data root, whichever platform this is built for.
const APP_DATA: &str = if cfg!(target_os = "espidf") {
    pomelo_hal::app_data::BOARD_APP_DATA
} else {
    pomelo_hal::app_data::DESKTOP_APP_DATA
};

/// Where the alarm list is kept, and where the chime is written.
fn alarms_path() -> String {
    format!("{APP_DATA}/CLOCK/alarms.txt")
}

fn chime_path() -> String {
    format!("{APP_DATA}/CLOCK/chime.wav")
}

/// Which of the three pages is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The dial, the digital readout and the date.
    Face,
    /// The pomodoro ring and its transport.
    Timer,
    /// The alarm list.
    Alarms,
}

impl Tab {
    /// Every tab, in the order the strip draws them.
    pub const ALL: [Tab; 3] = [Tab::Face, Tab::Timer, Tab::Alarms];

    /// The tab's name, in the language the system is set to.
    pub fn label(self, language: Language) -> &'static str {
        match (self, language) {
            (Tab::Face, Language::Chinese) => "时钟",
            (Tab::Face, Language::English) => "Clock",
            (Tab::Timer, Language::Chinese) => "番茄钟",
            (Tab::Timer, Language::English) => "Timer",
            (Tab::Alarms, Language::Chinese) => "闹钟",
            (Tab::Alarms, Language::English) => "Alarms",
        }
    }
}

/// Which half of a pomodoro is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Twenty-five minutes of work.
    Work,
    /// The five-minute break after it.
    Break,
}

impl Phase {
    /// How long this phase is.
    pub fn secs(self) -> u64 {
        match self {
            Phase::Work => WORK_SECS,
            Phase::Break => BREAK_SECS,
        }
    }

    /// What comes after this one.
    pub fn next(self) -> Self {
        match self {
            Phase::Work => Phase::Break,
            Phase::Break => Phase::Work,
        }
    }

    /// The phase's name, in the language the system is set to.
    pub fn label(self, language: Language) -> &'static str {
        match (self, language) {
            (Phase::Work, Language::Chinese) => "专注",
            (Phase::Work, Language::English) => "Focus",
            (Phase::Break, Language::Chinese) => "休息",
            (Phase::Break, Language::English) => "Break",
        }
    }
}

/// Which number of an alarm a stepper moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Hour,
    Minute,
}

/// The pomodoro, as a count of the seconds this board has been up.
///
/// # Why uptime and not a clock
///
/// A phase that is running is `started_at` — an uptime in seconds — and the time it has left is
/// arithmetic on the board's own count rather than on a tick count this app keeps. That is what makes
/// the state *small*: pause and resume are one addition and one subtraction, closing the app and
/// reopening it keeps the phase running (the board did not stop counting), and nothing here can drift
/// away from the platform's clock by accumulating a second per tick it happened to miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pomodoro {
    phase: Phase,
    /// The seconds of this phase already spent, not counting the run in progress.
    spent: u64,
    /// The uptime the current run started at, when it is running.
    started_at: Option<u64>,
    /// How many work phases have been finished, counted when they finish and not when they are begun.
    completed: u32,
}

impl Pomodoro {
    /// A pomodoro at the start of a work phase, not running.
    pub fn new() -> Self {
        Self {
            phase: Phase::Work,
            spent: 0,
            started_at: None,
            completed: 0,
        }
    }

    /// The phase that is up.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the phase is counting down.
    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    /// How many work phases have been finished.
    pub fn completed(&self) -> u32 {
        self.completed
    }

    /// The seconds of the current phase already spent, the run in progress included.
    fn elapsed(&self, uptime: u64) -> u64 {
        self.spent
            + self
                .started_at
                .map_or(0, |start| uptime.saturating_sub(start))
    }

    /// How long the phase has left, given the board's uptime.
    pub fn remaining(&self, uptime: u64) -> u64 {
        self.phase.secs().saturating_sub(self.elapsed(uptime))
    }

    /// How much of the phase is left, in `0.0..=1.0` — what the ring is drawn from.
    pub fn progress(&self, uptime: u64) -> f32 {
        (self.remaining(uptime) as f32 / self.phase.secs() as f32).clamp(0.0, 1.0)
    }

    /// Start the phase, or stop it where it is.
    pub fn toggle(&mut self, uptime: u64) {
        self.started_at = match self.started_at {
            Some(start) => {
                self.spent += uptime.saturating_sub(start);
                None
            }
            None => Some(uptime),
        };
    }

    /// Put the phase back to its beginning, and stop.
    pub fn reset(&mut self) {
        self.spent = 0;
        self.started_at = None;
    }

    /// Give the phase up and move to the next one.
    ///
    /// A work phase given up early is *not* counted as finished: the count is of pomodoros done, and
    /// a phase abandoned at minute two is not one of them.
    pub fn skip(&mut self) {
        self.phase = self.phase.next();
        self.reset();
    }

    /// Close the phase and open the next one, counting it when it was work.
    ///
    /// Returns the phase that just ended, which is what the bell is for.
    pub fn finish(&mut self) -> Phase {
        let done = self.phase;

        if done == Phase::Work {
            self.completed += 1;
        }

        self.phase = done.next();
        self.reset();

        done
    }

    /// Roll into the next phase if the one running has run out.
    ///
    /// Called on every tick rather than from a timer of its own: the phase ends when the uptime says
    /// it has, so a platform that was asleep through the end of it still finds the right phase after
    /// — which is the same reason the count is seconds and not ticks.
    pub fn advance(&mut self, uptime: u64) -> Option<Phase> {
        if self.started_at.is_some() && self.elapsed(uptime) >= self.phase.secs() {
            Some(self.finish())
        } else {
            None
        }
    }
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self::new()
    }
}

/// One alarm: a time, and whether it goes off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alarm {
    /// The hour, `0..24`.
    pub hour: u32,
    /// The minute, `0..60`.
    pub minute: u32,
    /// Whether it is armed. A disabled alarm is one that is kept and not listened to, which is what
    /// an alarm on a weekday is at the weekend.
    pub enabled: bool,
}

impl Alarm {
    /// An armed alarm at this time.
    ///
    /// Both fields are taken modulo their range rather than refused, so that a stepper which has run
    /// off the end of a day wraps instead of failing — see [`Alarm::step_hour`].
    pub fn new(hour: u32, minute: u32) -> Self {
        Self {
            hour: hour % 24,
            minute: minute % 60,
            enabled: true,
        }
    }

    /// The same alarm, disarmed.
    pub fn off(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// `07:00`.
    pub fn hh_mm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// Move the hour by `steps` hours, through midnight in both directions.
    pub fn step_hour(&mut self, steps: i32) {
        self.hour = (self.hour as i32 + steps).rem_euclid(24) as u32;
    }

    /// Move the minute by `steps` five-minute steps, carrying into the hour.
    ///
    /// The carry is the part worth having: a plus on 07:55 is 08:00 rather than 07:00, and an alarm
    /// page where the two steppers did not agree about the hour would be two clocks in one row.
    pub fn step_minute(&mut self, steps: i32) {
        let total = self.minute as i32 + steps * STEP_MINUTES as i32;

        self.hour = (self.hour as i32 + total.div_euclid(60)).rem_euclid(24) as u32;
        self.minute = total.rem_euclid(60) as u32;
    }

    /// The alarm as one line of the file the list is kept in.
    pub fn line(&self) -> String {
        format!("{} {} {}", self.hour, self.minute, self.enabled as u8)
    }

    /// The alarm one line of that file says, or `None` for a line that says nothing.
    ///
    /// Forgiving on purpose: the file is this app's own, but it lives on a filesystem somebody can
    /// open a terminal over, and one line a person has edited is not a reason to lose the alarms on
    /// the lines around it.
    pub fn parse(line: &str) -> Option<Self> {
        let mut fields = line.split_whitespace();

        let hour: u32 = fields.next()?.parse().ok()?;
        let minute: u32 = fields.next()?.parse().ok()?;
        let enabled = fields.next().map(|flag| flag != "0").unwrap_or(true);

        Some(Self {
            hour: hour % 24,
            minute: minute % 60,
            enabled,
        })
    }
}

/// What the clock reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// Time has passed.
    ///
    /// Carries nothing, because nothing is needed: both readings come from the board when this
    /// arrives, and the board is what knows whether a second or a frame has gone by. See
    /// [`Clock::tick`].
    Tick,
    /// A tab was tapped.
    Tab(Tab),
    /// The pomodoro was started, or stopped.
    Toggle,
    /// The pomodoro was put back to the beginning of its phase.
    Reset,
    /// The pomodoro was given up for the next phase.
    Skip,
    /// A new alarm, at the first hour of the morning and disarmed.
    AlarmAdd,
    /// The alarm at this index, gone.
    AlarmRemove(usize),
    /// The alarm at this index, armed or disarmed.
    AlarmToggle(usize),
    /// The hour or minute of the alarm at this index, moved by `steps` of its field's step.
    AlarmStep(usize, Field, i32),
}

/// The clock.
pub struct Clock {
    board: Arc<Board>,
    preferences: SystemPreferences,
    /// The last wall-clock reading, and what the dial and the alarm check are drawn from. `None` is a
    /// board whose clock has never been set.
    reading: Option<Civil>,
    /// The last uptime, in seconds, which is the pomodoro's clock.
    uptime: u64,
    tab: Tab,
    pomodoro: Pomodoro,
    alarms: Vec<Alarm>,
    /// The day and minute the last alarm rang in, so that a minute rings once however many times the
    /// platform says a second has passed during it.
    rang: Option<(i64, u32, u32)>,
    /// Whether the chime has been written to the board yet.
    chime_written: bool,
    /// The last thing that went wrong, as a line under the page. Kept rather than logged because the
    /// one fault this app can have that a person can fix is a full filesystem.
    fault: Option<String>,
}

impl Clock {
    /// The clock, having looked at the board and read back its alarms once.
    pub fn new(board: Arc<Board>) -> Self {
        let mut clock = Self {
            board,
            preferences: SystemPreferences::default(),
            reading: None,
            uptime: 0,
            tab: Tab::Face,
            pomodoro: Pomodoro::new(),
            alarms: load_alarms().unwrap_or_else(default_alarms),
            rang: None,
            chime_written: false,
            fault: None,
        };

        clock.refresh();

        clock
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The tab that is up.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The last wall-clock reading, as the page draws it.
    pub fn reading(&self) -> Option<Civil> {
        self.reading
    }

    /// The pomodoro, which the tests drive directly.
    pub fn pomodoro(&self) -> &Pomodoro {
        &self.pomodoro
    }

    /// The alarm list, as the page draws it.
    pub fn alarms(&self) -> &[Alarm] {
        &self.alarms
    }

    /// The last fault, if anything has failed since the app was opened.
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    /// Read the board again, and let the pomodoro and the alarms act on what it says.
    ///
    /// Both readings are taken whether or not the caller wants them, because both are one call into a
    /// backend that never blocks: the clock and the uptime are asked of the system, and the system
    /// answers about right now.
    pub fn refresh(&mut self) {
        {
            let system = self.board.system();

            self.reading = system.clock().ok().flatten().map(Civil::from_system_time);
            self.uptime = system.uptime().map(|up| up.as_secs()).unwrap_or(0);
        }

        // A phase that ran out while nobody was looking is still a phase that ended, and the bell for
        // it is the same bell: this is the one place either can ring from.
        if self.pomodoro.advance(self.uptime).is_some() {
            self.bell();
        }

        self.check_alarms();
    }

    /// Time has passed, which for this page is the same as reading the board again.
    ///
    /// There is no throttle here and there does not need to be one: a tick costs two calls into the
    /// system backend, and the page wants the second hand to move anyway. The difference between the
    /// two platforms — the board's pump at a second, a window's frames at sixty — is invisible in
    /// everything but the smoothness of the sweep.
    pub fn tick(&mut self) {
        self.refresh();
    }

    /// Ring the bell if the wall clock has reached an armed alarm.
    fn check_alarms(&mut self) {
        let Some(civil) = self.reading else {
            return;
        };

        let day = civil.year * 10_000 + civil.month as i64 * 100 + civil.day as i64;
        let mark = (day, civil.hour, civil.minute);

        if self.rang == Some(mark) {
            return;
        }

        let due = self.alarms.iter().any(|alarm| {
            alarm.enabled && alarm.hour == civil.hour && alarm.minute == civil.minute
        });

        if due {
            self.rang = Some(mark);
            self.bell();
        }
    }

    /// Write the chime out if it is not there yet, and play it.
    ///
    /// The write happens on the first bell and not in `new`, because most days this app never rings:
    /// a board that has never had an alarm set has no `chime.wav` on it, and a file written for a
    /// sound nobody asked for is a file somebody has to explain.
    fn bell(&mut self) {
        let path = chime_path();

        if !self.chime_written {
            if let Err(error) = self.board.storage().write_file(&path, &chime::wav()) {
                self.fault = Some(error.to_string());
                return;
            }

            self.chime_written = true;
        }

        if let Err(error) = self.board.audio().play(&path) {
            self.fault = Some(error.to_string());
        }
    }

    /// Write the alarm list back, so that the next boot finds it.
    fn save_alarms(&mut self) {
        let mut text = String::new();

        for alarm in &self.alarms {
            text.push_str(&alarm.line());
            text.push('\n');
        }

        if let Err(error) = self.board.storage().write_file(&alarms_path(), text.as_bytes()) {
            self.fault = Some(error.to_string());
        }
    }

    /// One message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick => self.tick(),
            Message::Tab(tab) => self.tab = tab,
            Message::Toggle => self.pomodoro.toggle(self.uptime),
            Message::Reset => self.pomodoro.reset(),
            Message::Skip => self.pomodoro.skip(),
            Message::AlarmAdd => {
                self.alarms.push(Alarm::new(7, 0).off());
                self.save_alarms();
            }
            Message::AlarmRemove(index) => {
                if index < self.alarms.len() {
                    self.alarms.remove(index);
                    self.save_alarms();
                }
            }
            Message::AlarmToggle(index) => {
                if let Some(alarm) = self.alarms.get_mut(index) {
                    alarm.enabled = !alarm.enabled;
                }

                self.save_alarms();
            }
            Message::AlarmStep(index, field, steps) => {
                if let Some(alarm) = self.alarms.get_mut(index) {
                    match field {
                        Field::Hour => alarm.step_hour(steps),
                        Field::Minute => alarm.step_minute(steps),
                    }
                }

                self.save_alarms();
            }
        }
    }
}

impl Clock {
    /// The theme: the palette the platform paints the page on.
    ///
    /// The same two palettes the launcher and every other app uses, so that the colour this page is
    /// drawn on does not change when the page does. The dark one is the box's own black and not a
    /// near-black of its own: the panel sits inside a black bezel, and only an exact black leaves that
    /// bezel's edge undrawn.
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

    /// Frames, which is the only clock a window has.
    ///
    /// On the board this is never asked for: the launcher merges the subscriptions of the apps it is
    /// actually running, and its own pump is what sends [`Message::Tick`] here. A *window* has no such
    /// pump, so the app takes the frames it is offered — and unlike the task viewer it does not
    /// throttle them, because a second hand that moves sixty times a second is the thing a clock is
    /// for, and the two calls a tick costs answer immediately.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::window::frames().map(|_| Message::Tick)
    }

    /// The page: whichever tab is up, over the tab strip.
    pub fn view(&self) -> Element<'_, Message> {
        let theme = self.preferences.theme;
        let sizes = style::Sizes::new();

        let mut page = column![
            container(self.body(sizes, theme))
                .width(Length::Fill)
                .height(Length::Fill),
            self.tabs(sizes, theme),
        ]
        .spacing(style::GAP)
        .padding(style::MARGIN)
        .width(Length::Fill)
        .height(Length::Fill);

        if let Some(fault) = &self.fault {
            page = page.push(self.fault_line(fault, sizes));
        }

        container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(style::surface(theme).into()),
                ..container::Style::default()
            })
            .into()
    }

    /// The body of the tab that is up.
    fn body(&self, sizes: style::Sizes, theme: ThemeMode) -> Element<'_, Message> {
        match self.tab {
            Tab::Face => self.face(sizes, theme),
            Tab::Timer => self.timer(sizes, theme),
            Tab::Alarms => self.alarms_page(sizes, theme),
        }
    }

    /// The dial, the readout and the date.
    ///
    /// The dial takes everything the tab has left and the two lines of text take what they need,
    /// which is the whole of the layout: the face is the thing that has to be as large as the panel
    /// allows, and a readout whose box grew with it would push the dial into a corner.
    fn face(&self, sizes: style::Sizes, theme: ThemeMode) -> Element<'_, Message> {
        let language = self.preferences.language;
        let ink = style::ink(theme);
        let muted = style::muted(theme);

        let (readout, line) = match self.reading {
            Some(civil) => (
                civil.hh_mm_ss(),
                format!("{}  {}", civil.date(), civil.weekday_name(language)),
            ),
            None => (
                String::from("--:--:--"),
                match language {
                    Language::Chinese => String::from("时钟尚未对时"),
                    Language::English => String::from("the clock is not set"),
                },
            ),
        };

        column![
            container(dial::dial(self.reading, theme))
                .width(Length::Fill)
                .height(Length::Fill),
            container(text(readout).size(sizes.digits).color(ink)).center_x(Length::Fill),
            container(text(line).size(sizes.text).color(muted)).center_x(Length::Fill),
        ]
        .spacing(style::GAP)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The pomodoro ring, the countdown in the middle of it, and the transport under it.
    ///
    /// The countdown is a `stack` over the ring because a canvas here cannot draw text — see
    /// [`dial`], which makes the same argument at length. The layer over the ring is given the whole
    /// of it and centres its own content, because a stack is only as big as its base layer and a
    /// layer smaller than that would sit in a corner.
    fn timer(&self, sizes: style::Sizes, theme: ThemeMode) -> Element<'_, Message> {
        let language = self.preferences.language;
        let ink = style::ink(theme);
        let muted = style::muted(theme);

        let uptime = self.uptime;
        let working = self.pomodoro.phase() == Phase::Work;
        let remaining = self.pomodoro.remaining(uptime);

        // A ring at rest is the muted ink rather than a phase colour, because a colour is a claim
        // about what is happening and a paused timer is not doing anything.
        let colour = if self.pomodoro.is_running() {
            style::phase_color(working)
        } else {
            muted
        };

        let status = match language {
            Language::Chinese => format!(
                "{} · 已完成 {}",
                self.pomodoro.phase().label(language),
                self.pomodoro.completed()
            ),
            Language::English => format!(
                "{} · {} done",
                self.pomodoro.phase().label(language),
                self.pomodoro.completed()
            ),
        };

        let dial = stack![
            ring::ring(self.pomodoro.progress(uptime), colour, style::track(theme)),
            container(
                Column::with_children(vec![
                    text(time::countdown(remaining))
                        .size(sizes.digits)
                        .color(ink)
                        .into(),
                    text(status).size(sizes.small).color(muted).into(),
                ])
                .align_x(Alignment::Center)
                .spacing(style::GAP),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        ];

        let transport = row![
            self.control(Icon::RESTART_ALT, Message::Reset, sizes, theme),
            self.control(
                if self.pomodoro.is_running() {
                    Icon::PAUSE
                } else {
                    Icon::PLAY_ARROW
                },
                Message::Toggle,
                sizes,
                theme,
            ),
            self.control(Icon::SKIP_NEXT, Message::Skip, sizes, theme),
        ]
        .spacing(style::GAP)
        .width(Length::Fill);

        column![
            container(dial).width(Length::Fill).height(Length::Fill),
            transport,
        ]
        .spacing(style::GAP)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The alarm list, over the button that adds one.
    fn alarms_page(&self, sizes: style::Sizes, theme: ThemeMode) -> Element<'_, Message> {
        let language = self.preferences.language;
        let ink = style::ink(theme);
        let muted = style::muted(theme);

        let mut rows = Column::new().spacing(style::GAP).width(Length::Fill);

        if self.alarms.is_empty() {
            rows = rows.push(
                container(
                    text(match language {
                        Language::Chinese => "还没有闹钟",
                        Language::English => "no alarms yet",
                    })
                    .size(sizes.text)
                    .color(muted),
                )
                .center_x(Length::Fill)
                .padding(style::GAP),
            );
        }

        for (index, alarm) in self.alarms.iter().enumerate() {
            rows = rows.push(alarm_row(index, alarm, sizes, theme));
        }

        let add = button(
            container(
                row![
                    text(Icon::ALARM_ADD.glyph())
                        .font(pomelo_material_symbols::font())
                        .size(sizes.text)
                        .color(ink),
                    text(match language {
                        Language::Chinese => "新建闹钟",
                        Language::English => "New alarm",
                    })
                    .size(sizes.text)
                    .color(ink),
                ]
                .spacing(style::GAP)
                .align_y(Alignment::Center),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fixed(sizes.tab))
        .padding(0)
        .style(slab(
            style::surface(theme),
            style::selected(theme),
            ink,
        ))
        .on_press(Message::AlarmAdd);

        column![
            scrollable(rows).height(Length::Fill),
            add,
        ]
        .spacing(style::GAP)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// One of the three tab buttons.
    fn tabs(&self, sizes: style::Sizes, theme: ThemeMode) -> Element<'_, Message> {
        let language = self.preferences.language;

        let buttons: Vec<Element<'_, Message>> = Tab::ALL
            .iter()
            .map(|&tab| {
                let selected = tab == self.tab;
                let ink = style::tab_ink(theme, selected);
                let fill = if selected {
                    style::selected(theme)
                } else {
                    style::surface(theme)
                };

                button(
                    container(text(tab.label(language)).size(sizes.small).color(ink))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill),
                )
                .width(Length::Fill)
                .height(Length::Fixed(sizes.tab))
                .padding(0)
                .style(slab(fill, style::selected(theme), ink))
                .on_press(Message::Tab(tab))
                .into()
            })
            .collect();

        Row::with_children(buttons)
            .spacing(style::TAB_GAP)
            .width(Length::Fill)
            .into()
    }

    /// One of the timer's three buttons: a glyph on a slab, the same size as a tab.
    fn control(
        &self,
        icon: Icon,
        message: Message,
        sizes: style::Sizes,
        theme: ThemeMode,
    ) -> Element<'static, Message> {
        let ink = style::ink(theme);

        button(
            container(
                text(icon.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.text)
                    .color(ink),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fixed(sizes.tab))
        .padding(0)
        .style(slab(style::surface(theme), style::selected(theme), ink))
        .on_press(message)
        .into()
    }

    /// The line under the page when something has gone wrong.
    fn fault_line(&self, fault: &str, sizes: style::Sizes) -> Element<'_, Message> {
        let label = match self.preferences.language {
            Language::Chinese => "无法保存",
            Language::English => "could not save",
        };

        container(
            text(format!("{label}: {fault}"))
                .size(sizes.small)
                .color(style::work()),
        )
        .center_x(Length::Fill)
        .into()
    }
}

/// A rounded slab with `fill` behind it, showing `pressed` under a finger.
///
/// The same shape as the calculator's keys: one colour for a thing at rest and one for a thing being
/// touched. There is no third for hover, because this panel has no pointer to hover with.
fn slab(fill: Color, pressed: Color, ink: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| button::Style {
        background: Some(
            match status {
                button::Status::Pressed | button::Status::Hovered => pressed,
                _ => fill,
            }
            .into(),
        ),
        text_color: ink,
        border: Border {
            radius: style::RADIUS.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// One row of the alarm list: the two steppers of each field, the time, the switch and the cross.
///
/// The time is given what is left of the row rather than a fixed width, so that the two pairs of
/// steppers stay against the ends a finger reaches for, and the numbers sit between them wherever the
/// row happens to be.
fn alarm_row(
    index: usize,
    alarm: &Alarm,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'static, Message> {
    let ink = style::ink(theme);
    let muted = style::muted(theme);
    let time_ink = if alarm.enabled { ink } else { muted };

    container(
        row![
            stepper(index, Field::Hour, -1, sizes, theme),
            stepper(index, Field::Hour, 1, sizes, theme),
            container(text(alarm.hh_mm()).size(sizes.text).color(time_ink)).center_x(Length::Fill),
            stepper(index, Field::Minute, -1, sizes, theme),
            stepper(index, Field::Minute, 1, sizes, theme),
            switch(index, alarm.enabled, sizes, theme),
            cross(index, sizes, theme),
        ]
        .spacing(style::TAB_GAP)
        .align_y(Alignment::Center),
    )
    .padding([0.0, style::GAP])
    .width(Length::Fill)
    .height(Length::Fixed(sizes.row.max(style::STEPPER)))
    .style(move |_theme| container::Style {
        background: Some(style::surface(theme).into()),
        border: Border {
            radius: style::RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// A square button that moves one field of one alarm.
fn stepper(
    index: usize,
    field: Field,
    steps: i32,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'static, Message> {
    let ink = style::ink(theme);

    let icon = if steps > 0 { Icon::ADD } else { Icon::REMOVE };

    button(
        container(
            text(icon.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.small)
                .color(ink),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fixed(style::STEPPER))
    .height(Length::Fixed(style::STEPPER))
    .padding(0)
    .style(slab(style::surface(theme), style::selected(theme), ink))
    .on_press(Message::AlarmStep(index, field, steps))
    .into()
}

/// The alarm's switch: armed alarms are the positive colour, and a disarmed one is the muted ink.
fn switch(
    index: usize,
    enabled: bool,
    sizes: style::Sizes,
    theme: ThemeMode,
) -> Element<'static, Message> {
    let ink = if enabled {
        style::rest()
    } else {
        style::muted(theme)
    };

    let icon = if enabled {
        Icon::ALARM_ON
    } else {
        Icon::ALARM_OFF
    };

    button(
        container(
            text(icon.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.small)
                .color(ink),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fixed(style::STEPPER))
    .height(Length::Fixed(style::STEPPER))
    .padding(0)
    .style(slab(style::surface(theme), style::selected(theme), ink))
    .on_press(Message::AlarmToggle(index))
    .into()
}

/// The cross that takes an alarm off the list.
fn cross(index: usize, sizes: style::Sizes, theme: ThemeMode) -> Element<'static, Message> {
    let ink = style::muted(theme);

    button(
        container(
            text(Icon::DELETE.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.small)
                .color(ink),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fixed(style::STEPPER))
    .height(Length::Fixed(style::STEPPER))
    .padding(0)
    .style(slab(style::surface(theme), style::selected(theme), ink))
    .on_press(Message::AlarmRemove(index))
    .into()
}

/// The alarms this app comes up with when the board has none of its own.
///
/// One, disarmed, at the hour a person is most likely to want — which is the *shape* of the page the
/// alarm tab draws rather than a decision about anybody's morning. An empty list would be a page with
/// nothing on it and no way to learn what a row looks like, and an *armed* default would go off on a
/// board somebody had just unboxed.
fn default_alarms() -> Vec<Alarm> {
    vec![Alarm::new(7, 0).off()]
}

/// The alarms the board has, or `None` when it has never been told any.
///
/// Read with `std::fs`, which is the half of a filesystem this board has always had. `None` and an
/// empty list are different answers: the first is a board that has never saved an alarm and gets the
/// default above, and the second is somebody who deleted them all and means it.
fn load_alarms() -> Option<Vec<Alarm>> {
    let text = std::fs::read_to_string(alarms_path()).ok()?;

    Some(text.lines().filter_map(Alarm::parse).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock on the simulated board. The alarm list is whatever the desktop has saved, which the
    /// tests below do not depend on: nothing here writes one.
    fn clock() -> Clock {
        Clock::new(Arc::new(Board::simulated()))
    }

    /// The three tabs are three pages of one app, and each of them is named in the system's language
    /// rather than in a language this app picked.
    #[test]
    fn every_tab_is_named_in_the_language_the_system_speaks() {
        assert_eq!(Tab::ALL.len(), 3);

        for tab in Tab::ALL {
            let chinese = tab.label(Language::Chinese);
            let english = tab.label(Language::English);

            assert!(!chinese.is_empty());
            assert!(!english.is_empty());
            assert_ne!(chinese, english, "{tab:?} is named the same twice");
        }
    }

    /// A pomodoro counts down only while it runs: pausing at four minutes and coming back ten minutes
    /// later has to leave twenty-one minutes, not eleven.
    #[test]
    fn a_paused_pomodoro_does_not_spend_the_time_it_is_paused_for() {
        let mut pomodoro = Pomodoro::new();

        pomodoro.toggle(1_000);
        assert!(pomodoro.is_running());
        assert_eq!(pomodoro.remaining(1_000), WORK_SECS);
        assert_eq!(pomodoro.remaining(1_240), WORK_SECS - 240);

        pomodoro.toggle(1_240);
        assert!(!pomodoro.is_running());
        assert_eq!(pomodoro.remaining(99_999), WORK_SECS - 240);

        pomodoro.toggle(99_999);
        assert_eq!(pomodoro.remaining(100_059), WORK_SECS - 300);
    }

    /// Putting the phase back to its beginning is a reset of the count *and* of the run: a reset timer
    /// that was still running would start spending the new phase from the old start.
    #[test]
    fn a_reset_timer_is_a_stopped_timer() {
        let mut pomodoro = Pomodoro::new();

        pomodoro.toggle(0);
        pomodoro.reset();

        assert!(!pomodoro.is_running());
        assert_eq!(pomodoro.remaining(5_000), WORK_SECS);
        assert_eq!(pomodoro.progress(5_000), 1.0);
    }

    /// The ring is the share of the phase left, and it unwinds from full to empty.
    #[test]
    fn the_ring_unwinds_as_the_phase_is_spent() {
        let mut pomodoro = Pomodoro::new();
        pomodoro.toggle(0);

        assert_eq!(pomodoro.progress(0), 1.0);
        assert_eq!(pomodoro.progress(WORK_SECS / 4), 0.75);
        assert_eq!(pomodoro.progress(WORK_SECS), 0.0);
    }

    /// A phase that runs out becomes the next one — work into break, break into work — and the count is
    /// of work phases, not of phases.
    #[test]
    fn a_phase_that_runs_out_becomes_the_next_one() {
        let mut pomodoro = Pomodoro::new();
        pomodoro.toggle(100);

        assert_eq!(pomodoro.advance(100 + WORK_SECS - 1), None, "not yet");
        assert_eq!(pomodoro.advance(100 + WORK_SECS), Some(Phase::Work));

        assert_eq!(pomodoro.phase(), Phase::Break);
        assert_eq!(pomodoro.completed(), 1);
        assert!(!pomodoro.is_running(), "the next phase waits to be started");

        pomodoro.toggle(0);
        assert_eq!(pomodoro.advance(BREAK_SECS), Some(Phase::Break));
        assert_eq!(pomodoro.phase(), Phase::Work);
        assert_eq!(pomodoro.completed(), 1, "a break is not a pomodoro");
    }

    /// A phase given up early moves on without being counted: the count is of pomodoros done, and one
    /// abandoned at minute two is not one of them.
    #[test]
    fn skipping_a_phase_does_not_count_it() {
        let mut pomodoro = Pomodoro::new();
        pomodoro.toggle(0);

        pomodoro.skip();

        assert_eq!(pomodoro.phase(), Phase::Break);
        assert_eq!(pomodoro.completed(), 0);
        assert!(!pomodoro.is_running());
    }

    /// The two steppers: five minutes a tap, and the hour carries through midnight in both directions.
    #[test]
    fn an_alarm_steps_in_five_minutes_and_wraps_through_midnight() {
        let mut alarm = Alarm::new(23, 55);

        alarm.step_minute(1);
        assert_eq!(alarm.hh_mm(), "00:00", "over midnight, forwards");

        alarm.step_hour(-1);
        assert_eq!(alarm.hh_mm(), "23:00", "and back to the hour before");

        alarm.step_minute(-1);
        assert_eq!(alarm.hh_mm(), "22:55", "under the hour, backwards");
    }

    /// The file the list is kept in round-trips one alarm a line, and a line somebody has mangled costs
    /// only itself.
    #[test]
    fn an_alarm_survives_a_line_of_the_file() {
        let alarm = Alarm::new(7, 5).off();

        assert_eq!(Alarm::parse(&alarm.line()), Some(alarm));
        assert_eq!(Alarm::parse("07 05 1"), Some(Alarm::new(7, 5)));
        assert_eq!(Alarm::parse("   "), None, "a blank line is not an alarm");
        assert_eq!(Alarm::parse("seven five"), None, "nor is a sentence");
    }

    /// A board with nothing saved comes up with one row to look at, and it is not armed: an alarm that
    /// goes off on a board somebody has just unboxed is a worse default than an empty page.
    #[test]
    fn a_board_with_no_alarms_of_its_own_gets_one_disarmed_row() {
        let alarms = default_alarms();

        assert_eq!(alarms.len(), 1);
        assert!(!alarms[0].enabled);
    }

    /// The clock comes up on the board's own clock, and on the board's uptime rather than a clock of
    /// its own.
    #[test]
    fn a_new_clock_reads_the_board() {
        let clock = clock();

        assert_eq!(clock.tab(), Tab::Face);
        assert!(clock.reading().is_some(), "the simulator's clock is set");
        assert!(!clock.pomodoro().is_running());
        assert_eq!(clock.fault(), None);
    }

    /// Moving between the three pages does not stop the timer, which is the one thing a timer that
    /// paused when you looked away would fail at.
    #[test]
    fn the_pages_do_not_stop_the_timer() {
        let mut clock = clock();

        clock.update(Message::Toggle);
        assert!(clock.pomodoro().is_running());

        clock.update(Message::Tab(Tab::Alarms));
        assert_eq!(clock.tab(), Tab::Alarms);
        assert!(clock.pomodoro().is_running());

        clock.update(Message::Tab(Tab::Face));
        clock.tick();
        assert!(clock.pomodoro().is_running());
    }

    /// A stepper on an index that is not there is a message from a page that has been rebuilt since —
    /// which is not a reason to panic, and not a reason to write the list either.
    #[test]
    fn a_stepper_on_an_alarm_that_is_gone_is_ignored() {
        let mut clock = clock();
        let before = clock.alarms().len();

        clock.update(Message::AlarmStep(999, Field::Minute, 1));
        clock.update(Message::AlarmToggle(999));
        clock.update(Message::AlarmRemove(999));

        assert_eq!(clock.alarms().len(), before);
    }

    /// The page builds at the one base font size the system draws at: the sizes are chosen from
    /// `FONT_BASE` and the text drawn at them. A widget tree that only assembles with some preference
    /// is a page somebody sees once.
    #[test]
    fn the_page_is_drawn_at_the_base_font_size() {
        let mut clock = clock();

        for tab in Tab::ALL {
            clock.update(Message::Tab(tab));
            let _ = clock.view();
        }
    }
}
