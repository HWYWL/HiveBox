//! The task viewer — the `htop` of this board — **a standard iced program**.
//!
//! Not a program that owns a loop: [`TaskViewer`] is a state with an `update` and a `view`, and
//! `main.rs` — a handful of lines — is what runs it in a window. On the board the launcher hosts it
//! like every other app, and the app cannot tell the difference.
//!
//! # What it is looking at
//!
//! The tasks the *scheduler* is running, and this is the interesting distinction: the apps this
//! interface hosts are not tasks. They are structs the launcher owns and draws, sharing one stack;
//! the tasks on this page are what the scheduler switches between, with stacks that can run out. A
//! list of "all the apps" would be a list of five things nobody can act on; a list of tasks is the
//! one place where a board about to fall over says so — a task with two hundred bytes of stack left is
//! a task that faults the day something calls a function deeper.
//!
//! Which is why the columns are the ones they are: the *state* (is it making progress?), the
//! *priority* (who goes next?), the stack left at its worst (is it about to die?) and the core. The
//! numbers a `top` on a desktop spends half its row on — CPU percent, resident memory, virtual size —
//! are not here because this platform cannot measure them honestly: per-task run time needs a
//! run-time-stats clock the port does not keep, and the memory a task holds is not something a
//! FreeRTOS task records.
//!
//! # Why the page looks like `htop`, and speaks this interface's language
//!
//! Because that is the whole request, and because a shape someone already knows is worth more than a
//! nicer one they have to learn: meters at the top, a table under them. The *shape* is `htop`'s, down to
//! the row of column heads above the rows.
//!
//! Every word on it follows the language the system is set to: the heads (任务 / 状态 / 优先级 / 空闲栈 /
//! 核心, or `TASK S PRI FREE CORE`), the meter labels (内存 / 外部内存, or `MEM` / `PSRAM`), the header's
//! summary, and the state column, which is one character of whichever language is up — 运 / 阻 / 挂 / 退,
//! or the four letters `htop` prints for the same four states, `R` / `S` / `T` / `Z`. See [`columns`]
//! and [`format::state_char`].
//!
//! The first version of this page got that wrong, and paid for it twice. It copied `htop`'s English
//! heads whatever the system was set to, arguing that they are the names of things in a scheduler. They
//! are that, but they are also labels on a screen, and a table that switched languages halfway down the
//! page read as somebody else's — so the heads became the system's, and *then* the widths became a
//! language question: 优先级 needs three full-width characters where `PRI` needs three narrow ones, and a
//! column has to fit whichever of the two is up.
//!
//! 外部内存 and not the acronym, which is where this page parts company with the settings app: that app's
//! memory page is read by someone looking the part up, and this page by someone glancing at a panel.
//! Both are right for where they are.
//!
//! # Readings, and the clock they arrive on
//!
//! The page holds a [`Reading`] and never reads the board from `view`: a frame must not cost a
//! scheduler pause, and [`SystemBackend::tasks`] is the one reading on this board that does. So the
//! board is read when it is *told* a second has passed — [`Message::Tick`] — and the app throttles
//! that itself, because the clock it is woken by is whatever the platform has: the launcher's
//! one-a-second pump on the board, and a window's frames where there is no such pump. See
//! [`TaskViewer::tick`].
//!
//! [`SystemBackend::tasks`]: pomelo_hal::SystemBackend::tasks

mod format;
pub mod style;

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::widget::{column, container, row, scrollable, text, Column, Row, Space};
use iced::{Alignment, Color, Element, Length, Padding, Subscription, Theme};

use pomelo_hal::{Board, MemoryInfo, TaskInfo, TaskState};
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

/// How long between readings, however often the platform says "now".
///
/// One second, which is what a person watching a task list wants and what the board can afford: the
/// read holds the scheduler still for a moment, and once a second is nothing next to a frame.
pub const REFRESH: Duration = Duration::from_secs(1);

/// What the viewer reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// Time has passed: `now` is when the platform said so.
    ///
    /// The clock and not a bare "tick", because the platforms differ: the board's pump sends this once
    /// a second, and a window's frame arrives sixty times that often. A message that carried nothing
    /// would leave the app unable to tell the two apart, and the throttle in [`TaskViewer::tick`] is
    /// what makes them the same app.
    Tick(Instant),
}

/// The task viewer.
pub struct TaskViewer {
    board: Arc<Board>,
    /// The last reading, and what the page draws.
    reading: Reading,
    /// When the board was last read, for the throttle in [`TaskViewer::tick`].
    last_read: Option<Instant>,
    preferences: SystemPreferences,
}

/// What the board said, the last time it was asked.
///
/// A struct and not fields on the viewer, because it is written all at once: a reading is taken as one
/// look at the board, and a page that mixed a fresh task list with a stale heap would be a page
/// showing a moment that never happened.
#[derive(Debug, Default)]
struct Reading {
    tasks: Vec<TaskInfo>,
    memory: Option<MemoryInfo>,
    psram: Option<MemoryInfo>,
    uptime: Option<Duration>,
}

impl TaskViewer {
    /// The viewer, having looked at the board once.
    pub fn new(board: Arc<Board>) -> Self {
        let mut viewer = Self {
            board,
            reading: Reading::default(),
            last_read: None,
            preferences: SystemPreferences::default(),
        };

        viewer.refresh();

        // The throttle starts where the first reading did: a viewer built a moment ago has looked a
        // moment ago, and the frames that follow must not be read as "a second has passed".
        viewer.last_read = Some(Instant::now());

        viewer
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The tasks of the last reading, as the page draws them.
    ///
    /// Read by the tests and by the host; a page gets them through [`TaskViewer::view`].
    pub fn tasks(&self) -> &[TaskInfo] {
        &self.reading.tasks
    }

    /// Looks at the board again.
    ///
    /// Every reading is allowed to fail on its own. A board that could not say how full its PSRAM is
    /// still has a task list worth drawing, and a page that hid all of it behind one failed read would
    /// lose the thing it exists for.
    pub fn refresh(&mut self) {
        let system = self.board.system();

        self.reading = Reading {
            // A board built without the trace facility answers with an empty list rather than an
            // error, so this keeps that distinction: an empty `Vec` is "no tasks to show", and only a
            // real error is folded into one.
            tasks: system.tasks().unwrap_or_default(),
            memory: system.memory().ok(),
            psram: system.psram().ok().flatten(),
            uptime: system.uptime().ok(),
        };
    }

    /// Time has passed: look again, if a second has passed since the last look.
    ///
    /// The throttle is the whole of what makes one app out of two platforms. `uxTaskGetSystemState`
    /// holds the scheduler still while it walks the task list, so reading it per frame would cost the
    /// board a moment of scheduling sixty times a second — and the frame is not this app's to refuse:
    /// it is what the window has.
    pub fn tick(&mut self, now: Instant) {
        let due = self
            .last_read
            .is_none_or(|last| now.duration_since(last) >= REFRESH);

        if due {
            self.last_read = Some(now);
            self.refresh();
        }
    }

    /// One message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.tick(now),
        }
    }

    /// The theme: the palette the platform paints the page on.
    ///
    /// The same two palettes the launcher and the settings app use, so that the colour this page is
    /// drawn on does not change when the page does — and so the panel's background behind it is the
    /// same dark every other screen is.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: Color::from_rgb8(20, 22, 38),
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
    /// actually running, and its own pump is what sends [`Message::Tick`] here — a second, on those
    /// platforms, is the platform's business. A *window* has no such pump, so the app takes the frames
    /// it is offered and throttles them itself; see [`TaskViewer::tick`].
    pub fn subscription(&self) -> Subscription<Message> {
        iced::window::frames().map(Message::Tick)
    }

    /// The page: a header, the meters and the table.
    ///
    /// No line under the table, and its absence is deliberate rather than an omission: it used to carry
    /// the refresh interval and what the `*` marks, which is one line of a screen spent telling a reader
    /// something they can see (the table fills itself in every second) or ask about (the marks are on the
    /// row of the task drawing the page). What the table's own columns cannot say is worth a row; a
    /// footnote is not.
    pub fn view(&self) -> Element<'_, Message> {
        let theme = self.preferences.theme;
        let sizes = style::Sizes::of(self.preferences.font_tier);
        let ink = style::ink(theme);
        let muted = style::muted(theme);

        let page = column![
            self.header(sizes, ink, muted),
            self.meters(sizes, theme, ink, muted),
            self.table(sizes, theme, ink, muted),
        ]
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

    /// How many of the last reading's tasks are making progress.
    ///
    /// The one number in a task list worth having at a glance, which is why `htop` puts it in its
    /// header: everything else in the list is a task *waiting* for something, and a board where that
    /// count has gone to zero is a board that has stopped doing anything while looking perfectly busy.
    fn running(&self) -> usize {
        self.reading
            .tasks
            .iter()
            .filter(|task| task.state == TaskState::Running)
            .count()
    }

    /// The line at the top: the icon, how many tasks there are, how many of them are moving, and how long
    /// the board has been up.
    ///
    /// A heading and not a title, because this page has no name of its own to draw — the desktop tiles
    /// it with a name, and a second copy of it here would be a name written down twice. What a reader
    /// wants at the top of a task list is the size of it, and how much of it is moving.
    fn header(&self, sizes: style::Sizes, ink: Color, muted: Color) -> Element<'_, Message> {
        let language = self.preferences.language;
        let (tasks, running) = (self.reading.tasks.len(), self.running());

        let summary = match language {
            Language::Chinese => format!("任务 {tasks} · 运行 {running}"),
            Language::English => format!("tasks {tasks} · {running} running"),
        };

        let uptime = self.reading.uptime.map_or_else(
            || String::from("--:--:--"),
            format::clock_of,
        );

        row![
            text(Icon::MONITOR_HEART.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.small)
                .color(muted),
            Space::new().width(Length::Fixed(style::CELL_PAD * 2.0)),
            text(summary).size(sizes.small).color(ink),
            Space::new().width(Length::Fill),
            text(uptime).size(sizes.small).color(muted),
        ]
        .align_y(Alignment::Center)
        .into()
    }

    /// The meters: the board's two memory pools, as bars.
    ///
    /// Bars and not the settings app's rings, and two of them rather than three: `htop`'s header is a
    /// row of bars, the whole point is that a reader knows how to read one, and the two pools here are
    /// exactly the two questions worth asking of this board — the internal heap this program runs out
    /// of, and the external RAM it goes and gets when it wants a framebuffer.
    ///
    /// Their labels follow the interface's language, like every other word on the page — the same rule
    /// [`columns`] applies to the table's heads.
    fn meters(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
    ) -> Element<'_, Message> {
        let (heap, external) = match self.preferences.language {
            Language::Chinese => ("内存", "外部内存"),
            Language::English => ("MEM", "PSRAM"),
        };

        Column::with_children(vec![
            meter(heap, self.reading.memory, sizes, theme, ink, muted),
            // The external pool written out rather than by its acronym, which is what the label column
            // was widened for: this page is read on a panel by whoever is holding the box, and 外部内存
            // says which of the two pools it is without asking them to know what `PSRAM` stands for. The
            // part's own spelling is where a reader goes looking for it — `Octal-SPI` on the settings
            // app's memory page.
            meter(external, self.reading.psram, sizes, theme, ink, muted),
        ])
        .spacing(style::ROW_GAP)
        .width(Length::Fill)
        .into()
    }

    /// The list: a head row, then one row per task, in the order the scheduler gave them.
    ///
    /// Scrollable, and that is not a courtesy: a busy board runs more tasks than fit on this panel, and
    /// a list that silently stopped at the bottom would be a list that misreported how many there are.
    fn table(
        &self,
        sizes: style::Sizes,
        theme: ThemeMode,
        ink: Color,
        muted: Color,
    ) -> Element<'_, Message> {
        let language = self.preferences.language;
        let mut rows = Column::new()
            .spacing(style::ROW_GAP)
            .width(Length::Fill);

        rows = rows.push(head_row(sizes, language, muted));

        for task in &self.reading.tasks {
            rows = rows.push(task_row(task, sizes, language, theme, ink, muted));
        }

        // The scrollbar's width is kept clear rather than drawn over, and it is kept clear *here* so that
        // the heads and the rows are narrowed together: iced paints the bar along the content's right
        // edge, which is where the last column ends — and was where 核心's head lost its second half.
        scrollable(container(rows).padding(Padding {
            right: style::SCROLLBAR,
            ..Padding::ZERO
        }))
        .height(Length::Fill)
        .into()
    }
}

/// One meter: what it is, how full, and the two numbers behind that.
///
/// A missing reading draws its label and a dash rather than a bar at zero: a pool nobody could measure
/// is not an empty pool, and a bar is the shape that says a number is there.
fn meter(
    label: &str,
    reading: Option<MemoryInfo>,
    sizes: style::Sizes,
    theme: ThemeMode,
    ink: Color,
    muted: Color,
) -> Element<'static, Message> {
    let label: Element<'static, Message> = container(text(label.to_string()).size(sizes.small).color(muted))
        .width(Length::Fixed(sizes.meter_label))
        .into();

    let Some(reading) = reading else {
        return row![label, text("—").size(sizes.small).color(muted)]
            .align_y(Alignment::Center)
            .into();
    };

    let share = reading.used_percent() / 100.0;
    let used = bytes_of(reading.used_bytes());
    let total = bytes_of(reading.total_bytes);

    let bar = bar(share, sizes, theme);
    let numbers = format!("{:.0}%  {used} / {total}", reading.used_percent());

    row![
        label,
        bar,
        Space::new().width(Length::Fixed(style::CELL_PAD * 2.0)),
        text(numbers).size(sizes.small).color(ink),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// The bar itself: the filled part, the empty part, both rounded as one.
///
/// `FillPortion` and not a measured width, because the two shares are the one thing this bar has to say
/// and the layout engine already knows what to do with them — and a portion of *zero* is a portion the
/// engine reads as "no share at all", which is why both halves are clamped to at least one.
fn bar(share: f32, sizes: style::Sizes, theme: ThemeMode) -> Element<'static, Message> {
    let filled = (share * 100.0).round().clamp(1.0, 99.0) as u16;
    let empty = 100 - filled;

    let segment = |portion: u16, color: Color| -> Element<'static, Message> {
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
        segment(filled, style::meter_fill(share)),
        segment(empty, style::track(theme)),
    ])
    .width(Length::Fill)
    .into()
}

/// One column of the table: what its head says, how wide it is, and which end its text sits on.
///
/// Not called `Column`: that name belongs to the widget these are laid out in. A field of a table is a
/// column of it, and this is the description one is built from.
///
/// The table is built from these and from nothing else — the head row and every value row are the same
/// five cells with different words in them — which is what keeps a head over its own column. The first
/// version of this page laid the head out on its own, and it drifted on the very first frame: the cell
/// meant to hold the name column's head was given no width at all, so the heads packed themselves
/// against the left edge while the rows put their numbers against the right.
#[derive(Debug, Clone, Copy)]
struct Field {
    /// What the head says.
    head: &'static str,
    /// How wide the column is, in multiples of the body text size — or `None` for the one column that
    /// takes whatever is left, which is the task names and only ever the task names.
    ///
    /// Multiples rather than pixels, because the whole table scales with the font tier: a column
    /// measured in pixels would clip its own head the moment the system's text size went up. See
    /// [`style::Sizes::text`].
    width: Option<f32>,
    /// Which end the text lines up on: digits at the right, so that a column of them can be read down,
    /// and names at the left.
    align: Alignment,
}

/// The table's columns, in the language the system is set to.
///
/// Five, in the order a reader asks their questions: *what* is it, *is it making progress*, *who runs
/// next*, *how much stack has it left at its worst*, *where*. A function rather than a constant because
/// the heads are *text*: the shape of the table is the same in both languages and the words in it are
/// the reader's. The widths are the same for both, sized for the wider of the two heads.
///
/// What sets a column's floor is its head, whose width is a fact about the language rather than about
/// the state of the board: three full-width characters for 优先级 and 空闲栈, and `PRI` / `FREE` in four
/// narrow Latin glyphs — which is why these are chosen against the Chinese heads (see the test that
/// holds both to their column) and why `S` and not `STATE` is the state column's English head.
fn columns(language: Language) -> [Field; 5] {
    let (task, state, priority, stack, core) = match language {
        Language::Chinese => ("任务", "状态", "优先级", "空闲栈", "核心"),
        Language::English => ("TASK", "S", "PRI", "FREE", "CORE"),
    };

    [
        Field {
            head: task,
            width: None,
            align: Alignment::Start,
        },
        Field {
            head: state,
            width: Some(1.9),
            align: Alignment::Center,
        },
        Field {
            head: priority,
            width: Some(3.2),
            align: Alignment::End,
        },
        Field {
            head: stack,
            width: Some(3.2),
            align: Alignment::End,
        },
        Field {
            head: core,
            width: Some(2.6),
            align: Alignment::End,
        },
    ]
}

/// One row of the table: its five columns, with these values in them.
///
/// The head and the value rows are both made here, which is the whole point of it being a function: a
/// head laid out separately from the rows under it is a head that can drift, and on this table it did.
/// Nothing else in this file decides a column's width or which end it lines up on.
fn table_row(
    sizes: style::Sizes,
    language: Language,
    values: [String; 5],
    colors: [Color; 5],
    size: f32,
) -> Element<'static, Message> {
    let columns = columns(language);

    let cells = columns
        .iter()
        .zip(values)
        .zip(colors)
        .map(|((column, value), color)| {
            let cell = container(text(value).size(size).color(color)).align_x(column.align);

            let cell: Element<'static, Message> = match column.width {
                Some(width) => cell.width(Length::Fixed(sizes.text * width)).into(),
                None => cell.width(Length::Fill).into(),
            };

            cell
        });

    Row::with_children(cells)
        .spacing(style::CELL_PAD)
        .align_y(Alignment::Center)
        .into()
}

/// The column heads: the same five columns as the rows under them, with the heads' own words in them.
fn head_row(sizes: style::Sizes, language: Language, muted: Color) -> Element<'static, Message> {
    table_row(
        sizes,
        language,
        columns(language).map(|column| column.head.to_string()),
        [muted; 5],
        sizes.small,
    )
}

/// One row of the table.
///
/// The numbers are right-aligned and the names are not, which is what makes a column of them readable
/// without a monospaced face: this panel has one font, and digits of different widths line up on their
/// last character rather than their first. Which end each column's text sits on lives in [`columns`],
/// because the head above it has to sit on the same one.
fn task_row(
    task: &TaskInfo,
    sizes: style::Sizes,
    language: Language,
    theme: ThemeMode,
    ink: Color,
    muted: Color,
) -> Element<'static, Message> {
    let name = if task.current {
        // The task this page is being drawn by: the one row a reader can place, because it is *this*.
        format!("{} *", task.name)
    } else {
        task.name.clone()
    };

    let core = task.core.map_or_else(|| String::from("-"), |core| core.to_string());

    table_row(
        sizes,
        language,
        [
            name,
            format::state_char(task.state, language).to_string(),
            task.priority.to_string(),
            format::stack_of(task.stack_free_bytes),
            core,
        ],
        [
            ink,
            style::state_ink(task.state, theme),
            ink,
            muted,
            muted,
        ],
        sizes.text,
    )
}

/// A byte count as the shortest thing that still means it: `8.5M`, `512K`, `912`.
///
/// The meter's numbers, not the table's: a meter says how big a pool is, which is at least megabytes,
/// while a stack column is one task's margin, which is often bytes.
fn bytes_of(bytes: u64) -> String {
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;

    let bytes = bytes as f64;

    if bytes >= M {
        format!("{:.1}M", bytes / M)
    } else if bytes >= K {
        format!("{:.0}K", bytes / K)
    } else {
        format!("{bytes:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A viewer built on the simulated board has a reading, and it is the board's tasks.
    #[test]
    fn the_viewer_reads_the_board_it_was_given() {
        let viewer = TaskViewer::new(Arc::new(Board::simulated()));

        assert!(!viewer.tasks().is_empty(), "the simulator runs tasks");
        assert!(
            viewer.reading.memory.is_some(),
            "and has a heap to draw a meter from"
        );
        assert!(viewer.reading.psram.is_some(), "and external RAM");
        assert!(viewer.reading.uptime.is_some());

        let _ = viewer.view();
    }

    /// The throttle: a tick that comes too soon does not read the board, and one a second later does.
    ///
    /// This is the whole of what keeps a window's sixty frames a second from becoming sixty scheduler
    /// pauses, so it is the one piece of behaviour in this app worth pinning down.
    #[test]
    fn a_tick_reads_at_most_once_a_second() {
        let mut viewer = TaskViewer::new(Arc::new(Board::simulated()));
        let start = Instant::now();

        // The construction's own read is what puts the throttle where it is: nothing is due yet.
        let boot = viewer.last_read.expect("the first look is recorded");
        assert!(
            start.duration_since(boot) < REFRESH,
            "a fresh viewer has just looked"
        );

        viewer.tick(boot + Duration::from_millis(16));
        assert_eq!(
            viewer.last_read,
            Some(boot),
            "a frame sixteen milliseconds later is not a reason to hold the scheduler still"
        );

        viewer.tick(boot + REFRESH);
        assert_eq!(
            viewer.last_read,
            Some(boot + REFRESH),
            "a second later is"
        );
    }

    /// The table and the meters fit the panel at every font tier, in both languages, with room kept
    /// clear on the right for the list's scrollbar.
    ///
    /// This is the test the page needed and did not have. Two things went wrong without it: the head row
    /// was laid out apart from the rows under it (see [`table_row`], now the only thing either is built
    /// by), and the columns were checked against the panel's *full* width — while iced paints the
    /// scrollbar along the content's right edge, so the last column arrived on the panel with its head
    /// cut in half. Both are arithmetic, which is what makes them worth checking by hand.
    #[test]
    fn the_table_and_the_meters_fit_the_panel() {
        use pomelo_widgets::preferences::FontSizeTier;

        /// How wide `text` is, as a column lays it out.
        ///
        /// An estimate, and a deliberately pessimistic one for what it is used on: a full-width character
        /// of this interface is one em, a capital or a digit measurably under two thirds of one. An
        /// estimate is the wrong tool for deciding a pixel and the right tool for catching 「three Chinese
        /// characters in a column sized for two」, which is the mistake worth guarding.
        fn width_of(text: &str, size: f32) -> f32 {
            text.chars()
                .map(|c| if c.is_ascii() { size * 0.62 } else { size })
                .sum()
        }

        let tiers = [
            FontSizeTier::ExtraSmall,
            FontSizeTier::Small,
            FontSizeTier::Standard,
            FontSizeTier::Large,
        ];

        for tier in tiers {
            let sizes = style::Sizes::of(tier);

            for language in [Language::Chinese, Language::English] {
                let columns = columns(language);

                // Every head fits over its own column, and so does the widest value that goes under it.
                // The values are the same in both languages — a number, a byte count, one character — so
                // they are the *same* list for both; what changes between the two is the head above them,
                // and the state column's single character, which is why they are checked per language.
                let widest = ["main *", "运", "29", "13.5K", "-"];

                for (column, widest) in columns.iter().zip(widest) {
                    let Some(width) = column.width else {
                        continue;
                    };
                    let room = width * sizes.text;

                    assert!(
                        width_of(column.head, sizes.small) <= room,
                        "{tier:?} {language:?}: 「{}」 needs {} px of a {room} px column",
                        column.head,
                        width_of(column.head, sizes.small)
                    );
                    assert!(
                        width_of(widest, sizes.text) <= room,
                        "{tier:?}: {widest} needs {} px of a {room} px column",
                        width_of(widest, sizes.text)
                    );
                }

                // And what the fixed columns leave for the names is a name's worth. The scrollbar is
                // subtracted because it is subtracted on the panel: see [`style::SCROLLBAR`].
                let fixed: f32 = columns
                    .iter()
                    .filter_map(|column| column.width)
                    .map(|width| width * sizes.text)
                    .sum();
                let gaps = style::CELL_PAD * (columns.len() - 1) as f32;
                let names = style::PANEL - style::MARGIN * 2.0 - style::SCROLLBAR - fixed - gaps;

                // The longest name this board has is `board_pwr_mon` — thirteen characters — and a
                // lowercase Latin character measures half the body size on the panel. Half, and not the
                // two thirds above: that figure is for capitals and digits, and using it here would be
                // this test arguing with a ruler.
                let longest = 13.0 * 0.5 * sizes.text;

                assert!(
                    names >= longest,
                    "{tier:?} {language:?}: {names} px of name column for a name that takes {longest} px"
                );
            }

            // The meter's label column, which is the one measurement that is not a table column: it has
            // to hold the longest label the page can write, 外部内存, in the size the labels are set in.
            for label in ["外部内存", "PSRAM"] {
                assert!(
                    width_of(label, sizes.small) <= sizes.meter_label,
                    "{tier:?}: {label} needs {} px of a {} px label column",
                    width_of(label, sizes.small),
                    sizes.meter_label
                );
            }
        }
    }

    /// A byte count is shortened at the unit it is at, and stays exact below a kilobyte.
    #[test]
    fn a_byte_count_is_short_and_readable() {
        assert_eq!(bytes_of(0), "0");
        assert_eq!(bytes_of(912), "912");
        assert_eq!(bytes_of(512 * 1024), "512K");
        assert_eq!(bytes_of(8 * 1024 * 1024), "8.0M");
        assert_eq!(bytes_of(8_519_680), "8.1M");
    }
}
