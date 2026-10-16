//! The battery over time: its discharge curve, what is left of it, what it is keeping on disk, and how
//! warm the chip that manages it has got.
//!
//! # What the page is for
//!
//! Every other page on this box reads the battery *now* — the status bar's icon, the settings page's
//! battery card.
//! This is the page that reads it *over time*, and it exists because the three questions people
//! actually ask about a battery are all about a shape rather than a number. "Is it going down?" is a
//! slope. "How long have I got?" is that slope carried forward. "Is something wrong with it?" is a
//! curve that is not the shape it was yesterday, and none of the three can be answered from a single
//! reading however exact.
//!
//! So the page is a curve, an estimate drawn from it, a banner when the power path is running hot, and
//! an hour-by-hour record on disk so that the shape outlives the session that watched it.
//!
//! # What it cannot do, said out loud
//!
//! There is no background service on this board. The launcher's pump ticks while a page is up and stops
//! the moment it is not, so this app samples only while someone is looking at it: a line is written
//! when the page opens — which is why there is a [`Message::LogNow`] button to take one on demand —
//! and once an hour for as long as it is left open. A battery watched from ten to eleven has an hour's
//! curve; one that was never opened has nothing, and the page says how many lines the file has rather
//! than pretending the history is complete.

mod format;
mod sample;
mod style;
mod ui;

pub use sample::{Base, Sample};

use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use iced::theme::Palette;
use iced::{Color, Element, Subscription, Theme};

use pomelo_hal::Board;
use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};

/// How far apart two points of the curve may be.
///
/// A minute, because a discharge is read over hours and a point a second would be a pixel a point on a
/// panel this wide. The pump's beat is a second, so this is the number that thins sixty ticks into one
/// point — and the window the curve shows is [`HISTORY`] of them, which is two hours.
pub const PLOT: Duration = Duration::from_secs(60);

/// How far apart two lines of the log on disk may be.
///
/// An hour, which is the resolution a discharge curve is actually kept at: a day of history is
/// twenty-four lines, and a month of it fits inside the built-in partition with room to spare.
pub const LOG_EVERY: Duration = Duration::from_secs(3_600);

/// How many points the curve keeps: two hours at [`PLOT`], and the tail of a longer history the file
/// has when it is longer than that.
pub const HISTORY: usize = 120;

/// The app-data root, whichever platform this is built for.
///
/// The board's `/internal/AppData` on the device, and `.pomelo/AppData` under the working directory on
/// a desktop — the same pair the clock keeps its alarms in, so the two apps are one directory apart
/// rather than one design apart.
const APP_DATA: &str = if cfg!(target_os = "espidf") {
    pomelo_hal::app_data::BOARD_APP_DATA
} else {
    pomelo_hal::app_data::DESKTOP_APP_DATA
};

/// Where the hour's record is kept.
fn log_path() -> String {
    format!("{APP_DATA}/POWER/history.csv")
}

/// What the app reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// Time has passed: look at the gauge, and write a line if an hour has.
    ///
    /// Carries the moment it arrived, because a window's frames are sixty a second and this page has
    /// no business taking four readings that often — see [`PowerLog::tick`].
    Tick(Instant),
    /// A finger asked for a sample on the spot, which is the only way to fill the log without leaving
    /// the page open for an hour.
    LogNow,
}

/// The battery, over time.
pub struct PowerLog {
    board: Arc<Board>,
    preferences: SystemPreferences,
    /// What the log's timestamps are counted from on this run, and what a log written against a
    /// different zero is kept out of the curve for.
    base: Base,
    /// Where the hour's record is written. A field rather than [`log_path`] read at each write, so that
    /// a test can point the app at a scratch directory and be the only thing it touches.
    path: String,
    /// The curve: the newest [`HISTORY`] samples, oldest first.
    samples: Vec<Sample>,
    /// The newest look at the gauge, which is what the reading and the estimate are drawn from. `None`
    /// only when the power backend cannot answer at all.
    now: Option<Sample>,
    /// When the last point was added to the curve. `None` until the first tick, which is how the first
    /// look is never thrown away however soon it follows the app being opened.
    last_point: Option<Instant>,
    /// When the last line was written. `None` until the first tick, so that opening the page writes the
    /// sample it opened on.
    last_log: Option<Instant>,
    /// How many lines the log on disk holds, as this app last saw it.
    logged: usize,
    /// The last thing that went wrong writing the log, if anything has.
    fault: Option<String>,
}

impl PowerLog {
    /// A page for this board, read once and then asked to look again.
    ///
    /// It reads the log before it reads the gauge, so that the first frame draws the history the box
    /// already had rather than an empty curve that fills in over the next hour.
    pub fn new(board: Arc<Board>) -> Self {
        Self::at(board, log_path())
    }

    /// The same page, with its log somewhere else.
    ///
    /// This is the tests' door and nothing else's: a test that let the app write to the real path would
    /// be a test that wrote to the developer's own home directory, and the point of the app having a
    /// path at all is that where a log goes is a decision rather than a constant buried in a call.
    pub fn at(board: Arc<Board>, path: String) -> Self {
        let base = base_of(&board);
        let (samples, logged) = read_log(&path, base);

        let mut app = Self {
            board,
            preferences: SystemPreferences::default(),
            base,
            path,
            samples,
            now: None,
            last_point: None,
            last_log: None,
            logged,
            fault: None,
        };

        app.now = app.look();

        app
    }

    /// The system's settings, which the page takes its colours and its words from.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// The system told the app what it is set to.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The curve, oldest point first.
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The newest look at the gauge, when the power backend could answer.
    pub fn latest(&self) -> Option<Sample> {
        self.now
    }

    /// Where the hour's record is being kept.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// How many lines the log on disk holds.
    ///
    /// Counted from the file rather than from the curve, because the two are not the same thing: the
    /// curve is minutes and the log is hours, and a page that drew the curve's length as the log's
    /// would say it had recorded two hours after two minutes of watching.
    pub fn logged(&self) -> usize {
        self.logged
    }

    /// The last thing that went wrong writing the log, if anything has.
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    /// How fast the charge is falling, in percent a second, when the curve is long enough to say.
    pub fn rate(&self) -> Option<f64> {
        sample::drain_rate(&self.samples)
    }

    /// What is left of the battery, when there is a discharge to read it off.
    pub fn estimate(&self) -> Option<Duration> {
        let now = self.now?;

        sample::remaining(&self.samples, &now)
    }

    /// The PMIC's temperature, when the board could read one.
    pub fn temperature(&self) -> Option<f32> {
        self.now.and_then(|sample| sample.temperature_c)
    }

    /// Whether the power path is running hot enough to say so.
    pub fn hot(&self) -> bool {
        self.temperature().is_some_and(|celsius| celsius >= style::HOT_C)
    }

    /// Whether the power path is running warm enough to be worth a word.
    ///
    /// True while [`hot`](PowerLog::hot) is, because a hot chip is also a warm one, and the banner says
    /// the worse of the two.
    pub fn warm(&self) -> bool {
        self.temperature().is_some_and(|celsius| celsius >= style::WARM_C)
    }

    /// Handle one message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.tick(now),
            Message::LogNow => self.take(Instant::now()),
        }
    }

    /// Look at the gauge, and write down what it says when the moment has come.
    ///
    /// The throttle is not a courtesy to the backend, which is a lock over values already in memory; it
    /// is because at sixty frames a second "look at the gauge" runs sixty times per repaint, and a page
    /// that takes four readings per frame is a page that drops frames on a board that has none to
    /// spare.
    pub fn tick(&mut self, now: Instant) {
        let Some(sample) = self.look() else {
            return;
        };

        self.now = Some(sample);

        if self.point_due(now) {
            self.last_point = Some(now);
            self.plot(sample);
        }

        if self.log_due(now) {
            self.last_log = Some(now);
            self.write(sample);
        }
    }

    /// Take a sample on the spot: a point on the curve and a line in the log, whatever the clock says.
    ///
    /// The two gates are pulled forward as well, so that a finger taking a sample now does not make the
    /// app take another one a second later just because the minute had come round.
    pub fn take(&mut self, now: Instant) {
        let Some(sample) = self.look() else {
            return;
        };

        self.now = Some(sample);
        self.last_point = Some(now);
        self.last_log = Some(now);
        self.plot(sample);
        self.write(sample);
    }

    /// Whether the curve is due a point.
    fn point_due(&self, now: Instant) -> bool {
        match self.last_point {
            Some(last) => now.saturating_duration_since(last) >= PLOT,
            None => true,
        }
    }

    /// Whether the log is due a line.
    fn log_due(&self, now: Instant) -> bool {
        match self.last_log {
            Some(last) => now.saturating_duration_since(last) >= LOG_EVERY,
            None => true,
        }
    }

    /// Add a point to the curve, and let the oldest one go when the curve is full.
    ///
    /// The window is the *newest* [`HISTORY`], not the first: a curve that stopped growing would be a
    /// page that stopped telling the time an hour after it opened.
    fn plot(&mut self, sample: Sample) {
        self.samples.push(sample);

        if self.samples.len() > HISTORY {
            let excess = self.samples.len() - HISTORY;
            self.samples.drain(0..excess);
        }
    }

    /// Write a line to the log, header and all when it is the first.
    ///
    /// A failure is kept rather than swallowed: the one thing that can go wrong with a log is that it
    /// is not being written, and a page that drew a full curve while the file was unwritable would be a
    /// page that lied about the only thing it keeps.
    fn write(&mut self, sample: Sample) {
        let text = if self.logged == 0 {
            format!("{}{}", sample::header(self.base), sample::line(&sample))
        } else {
            sample::line(&sample)
        };

        let result = self.board.storage().append_file(&self.path, text.as_bytes());

        match result {
            Ok(()) => {
                self.logged += 1;
                self.fault = None;
            }
            Err(error) => self.fault = Some(error.to_string()),
        }
    }

    /// One look at the gauge, with the moment it was taken.
    ///
    /// Each backend is asked once and its lock released before the next is taken: the power backend is
    /// one lock taken four times rather than four locks held at once, which is what keeps this from
    /// being the deadlock this page's own first draft was.
    fn look(&self) -> Option<Sample> {
        let percent = self.board.power().battery_percent().ok()?;
        let charging = self.board.power().is_charging().unwrap_or(false);
        let voltage_mv = self.board.power().battery_voltage_mv().ok();
        let temperature_c = self.board.power().chip_temperature_c().ok();
        let at = self.now_seconds();

        Some(Sample {
            at,
            percent,
            voltage_mv,
            temperature_c,
            charging,
        })
    }

    /// What a sample's timestamp is counted from, on this run.
    ///
    /// The wall clock when something has told the board the time, and the uptime before that — the same
    /// choice [`Base`] records in the header, and the reason the two cannot be mixed in one file.
    fn now_seconds(&self) -> u64 {
        match self.base {
            Base::Uptime => uptime_seconds(&self.board),
            Base::Unix => {
                let wall = self.board.system().clock().ok().flatten();

                match wall.and_then(|time| time.duration_since(UNIX_EPOCH).ok()) {
                    Some(since) => since.as_secs(),
                    None => uptime_seconds(&self.board),
                }
            }
        }
    }

    /// The theme: the two palettes every other page on this box is drawn on.
    ///
    /// The dark one is the box's own black rather than a near-black of its own, for the reason the
    /// clock gives: the panel sits inside a black bezel and only an exact black leaves that edge
    /// undrawn. The page's *own* colours are in [`style`], which is where a card's slab and a curve's
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

    /// The panel, drawn from what the app is holding.
    ///
    /// The page is capped at the panel it is designed for and centred in whatever it is given, so that
    /// the same layout that fills a 480 px square sits in the middle of a desktop window instead of
    /// stretching a curve across a monitor.
    ///
    /// Nothing here touches the board: the drawing is [`ui`]'s, and every number it draws comes out of
    /// this struct, so a test can build a page out of a curve written by hand and assert on what the
    /// widgets say.
    pub fn view(&self) -> Element<'_, Message> {
        ui::view(self)
    }
}

// ---- the board, read as a value ----

/// Which zero this board's clock counts from.
fn base_of(board: &Board) -> Base {
    let clock = board.system().clock().ok().flatten();

    if clock.is_some() {
        Base::Unix
    } else {
        Base::Uptime
    }
}

/// The board's uptime in whole seconds, or nought when even that cannot be read.
fn uptime_seconds(board: &Board) -> u64 {
    board
        .system()
        .uptime()
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// The log on disk, as the curve and the line count the page draws.
///
/// A file under a different base is read and *not* kept: its timestamps are counted from another zero,
/// and a curve that joined the two would draw a cliff between two afternoons. The line count is still
/// taken from it, because "the file has four hundred lines in it" is true whichever zero they use, and
/// a page that hid the history it could not plot would be pretending the log was smaller than it is.
fn read_log(path: &str, base: Base) -> (Vec<Sample>, usize) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Vec::new(), 0);
    };

    let (written, samples) = sample::parse(&text);
    let logged = samples.len();

    if written != Some(base) {
        return (Vec::new(), logged);
    }

    let start = samples.len().saturating_sub(HISTORY);

    (samples[start..].to_vec(), logged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A log file of this test's own, under the system's temp directory.
    ///
    /// Named after the process and a counter so that two tests running at once cannot pick the same
    /// one, and created before anything is written so that the path has a directory to be in. The app
    /// is never pointed at the real `.pomelo/AppData` path in a test: that would be a test that wrote
    /// to the developer's own home directory.
    fn scratch(name: &str) -> String {
        static NEXT: AtomicU32 = AtomicU32::new(0);

        let dir = std::env::temp_dir().join(format!(
            "pomelo-power-log-{}-{}-{}",
            name,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");

        dir.join("history.csv")
            .to_str()
            .expect("a utf-8 scratch path")
            .to_string()
    }

    /// Take the scratch directory away again.
    fn clear(path: &str) {
        if let Some(dir) = std::path::Path::new(path).parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// A sample written by hand, for the tests that need a history rather than a board.
    fn sample_at(at: u64, percent: u8) -> Sample {
        Sample {
            at,
            percent,
            voltage_mv: Some(4_000),
            temperature_c: Some(38.0),
            charging: false,
        }
    }

    /// A sample's own line, and the header above it, for a file written by hand.
    fn written(base: Base, samples: &[Sample]) -> String {
        std::iter::once(sample::header(base))
            .chain(samples.iter().map(sample::line))
            .collect()
    }

    /// Opening the page writes the sample it opened on, and the next line is an hour away — a minute
    /// is a point on the curve and not a line in the log.
    #[test]
    fn a_line_is_written_when_the_page_opens_and_once_an_hour_after() {
        let path = scratch("cadence");
        let mut app = PowerLog::at(Arc::new(Board::simulated()), path.clone());

        let start = Instant::now();
        app.tick(start);

        assert_eq!(app.logged(), 1, "the sample the page opened on");
        assert_eq!(app.samples().len(), 1, "which is also the curve's first point");

        app.tick(start + PLOT);
        assert_eq!(app.samples().len(), 2, "a minute is a point");
        assert_eq!(app.logged(), 1, "and not a line");

        app.tick(start + LOG_EVERY - Duration::from_secs(1));
        assert_eq!(app.logged(), 1, "the hour is not up a second early");

        app.tick(start + LOG_EVERY);
        assert_eq!(app.logged(), 2, "an hour is a line");

        let text = std::fs::read_to_string(&path).expect("the log the page wrote");
        assert!(
            text.starts_with("# pomelo power log v1"),
            "the header leads the file: {text:?}"
        );
        assert_eq!(
            text.lines().count(),
            3,
            "one header and two samples, and no header in between"
        );

        clear(&path);
    }

    /// The curve slides rather than grows: two hours of points and the oldest falls off the back.
    #[test]
    fn the_curve_keeps_the_newest_of_it_and_lets_the_oldest_go() {
        let path = scratch("window");
        let mut app = PowerLog::at(Arc::new(Board::simulated()), path.clone());

        let start = Instant::now();
        for step in 0..(HISTORY as u32 + 30) {
            app.tick(start + PLOT * step);
        }

        assert_eq!(
            app.samples().len(),
            HISTORY,
            "the window is the newest two hours, not the first"
        );
        assert_eq!(
            app.logged(),
            3,
            "two and a half hours of watching is three lines: the one it opened on, and one an hour"
        );

        clear(&path);
    }

    /// The history the box already had is read back and drawn — and only the half of it under this
    /// run's own zero, because a curve that joined two zeros would have a cliff in it.
    #[test]
    fn a_history_is_read_back_but_only_in_the_same_base() {
        let path = scratch("reload");
        let board = Arc::new(Board::simulated());
        let base = base_of(&board);

        let history: Vec<Sample> = (0..5).map(|n| sample_at(1_750_000_000 + n * 3_600, 90 - n as u8)).collect();
        std::fs::write(&path, written(base, &history)).expect("a log written by an earlier run");

        let app = PowerLog::at(Arc::clone(&board), path.clone());
        assert_eq!(app.samples().len(), 5, "the history is the curve");
        assert_eq!(app.logged(), 5, "and the file is five lines");

        let other = match base {
            Base::Unix => Base::Uptime,
            Base::Uptime => Base::Unix,
        };
        std::fs::write(&path, written(other, &history)).expect("a log from a board that was not told the time");

        let app = PowerLog::at(board, path.clone());
        assert!(
            app.samples().is_empty(),
            "another zero is another afternoon, and not this curve"
        );
        assert_eq!(app.logged(), 5, "but the lines are still lines");

        clear(&path);
    }

    /// A log that cannot be written is reported: a page that drew a full curve while the file was
    /// unwritable would be lying about the only thing it keeps.
    #[test]
    fn a_log_that_cannot_be_written_is_reported_rather_than_swallowed() {
        let root = std::env::temp_dir().join(format!("pomelo-power-log-blocked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch directory");

        // A file where the directory would have to be: there is nowhere for the log to go.
        let wall = root.join("wall");
        std::fs::write(&wall, b"not a directory").expect("a file in the way");
        let path = wall
            .join("history.csv")
            .to_str()
            .expect("a utf-8 path")
            .to_string();

        let mut app = PowerLog::at(Arc::new(Board::simulated()), path);
        app.tick(Instant::now());

        assert!(app.latest().is_some(), "the gauge was still read");
        assert!(app.fault().is_some(), "and the page says the log is not being written");
        assert_eq!(app.logged(), 0, "with no line counted that was not written");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A finger can take a sample there and then, which is the only way to fill the log without leaving
    /// the page open for an hour.
    #[test]
    fn a_sample_can_be_taken_on_demand() {
        let path = scratch("ondemand");
        let mut app = PowerLog::at(Arc::new(Board::simulated()), path.clone());

        app.update(Message::Tick(Instant::now()));
        assert_eq!(app.logged(), 1);

        app.update(Message::LogNow);
        assert_eq!(app.logged(), 2, "and again as soon as a finger asks");
        assert_eq!(app.samples().len(), 2, "a line on demand is a point too");

        assert_eq!(
            std::fs::read_to_string(&path)
                .expect("the log")
                .lines()
                .count(),
            3,
            "one header and two samples"
        );

        clear(&path);
    }

    /// The page builds with a history, with a curve and with neither — the one thing a widget tree can
    /// be asked without a window.
    #[test]
    fn the_page_builds_before_and_after_it_has_anything_to_draw() {
        let path = scratch("builds");
        let mut app = PowerLog::at(Arc::new(Board::simulated()), path.clone());

        let _ = app.view();
        app.tick(Instant::now());
        let _ = app.view();

        clear(&path);
    }

    /// The base is the board's: a board that has been told the time logs against the wall clock, and
    /// one that has not logs against its own uptime.
    #[test]
    fn the_log_is_written_against_the_board_s_own_zero() {
        let board = Board::simulated();
        let base = base_of(&board);
        let told_the_time = board.system().clock().ok().flatten().is_some();

        assert_eq!(
            base,
            if told_the_time { Base::Unix } else { Base::Uptime },
            "which zero a log is written against is the board's answer and not the app's"
        );
    }

    /// The page takes its colours and its words from the system, and a page that never heard from it
    /// still draws.
    #[test]
    fn the_page_takes_its_settings_from_the_system() {
        let path = scratch("settings");
        let mut app = PowerLog::at(Arc::new(Board::simulated()), path.clone());

        let preferences = SystemPreferences {
            theme: ThemeMode::Light,
            language: pomelo_widgets::preferences::Language::Chinese,
            ..SystemPreferences::default()
        };

        app.set_preferences(preferences);

        assert_eq!(app.preferences().theme, ThemeMode::Light);
        let _ = app.view();

        clear(&path);
    }
}
