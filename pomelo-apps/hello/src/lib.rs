//! The "hello" signature, drawn through iced's canvas — **a standard iced program**.
//!
//! A port of the app that used to live in this directory, which drew it with the framework this
//! repository no longer has. Everything that is not about a framework — the curves, the canonical
//! size, the rainbow and where it runs — is the same data, and it is [`artwork`]; what differed was
//! the vocabulary. The original was a `CustomPainter` holding a pixmap it accumulated the stroke
//! into; this one is a [`canvas::Program`] that states the stroke as geometry and lets the renderer
//! work out what changed.
//!
//! That difference is the point of the port, and it is measurable. A canvas frame adds the pieces
//! that are already finished as *equal* commands, so the recorded renderer's damage diff pairs them
//! with themselves and reports only the piece still growing; the original had to keep its own pixmap
//! and repaint the tip into it by hand. The one thing this port gives up is the background
//! dithering, which `canvas` has nowhere to put: see the canvas module.
//!
//! # The clock
//!
//! [`Hello::subscription`] is the clock: while the animation is running it subscribes to
//! `iced::window::frames()`, which iced has and this board answers, so the same program animates in
//! a window and on the panel. Each message carries the `Instant` the platform read when it drew that
//! frame, and the animation is a function of it — so the app never reads a clock of its own, and a
//! frame that arrives late moves the animation further instead of being lost.
//!
//! Two consequences of stating it as a subscription instead of as "am I animating":
//!
//! * the cycle starts on the **first frame after the app is told to run**, not when it is built. A
//!   launcher builds this app at boot and opens it when a finger arrives, which can be minutes
//!   later; a clock started at construction would be over before the signature was ever on screen
//!   (that is exactly what the board showed: a pale wash, no stroke, for as long as the app was
//!   open);
//! * the subscription is *empty* once the animation has finished, so a finished signature costs
//!   nothing: no frames, no damage, no wakeups. That is what the host's `is_animating` used to say,
//!   and now the app says it by answering nothing.

mod artwork;
pub mod canvas;
pub mod stroke;
pub mod style;
pub mod timing;

use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::widget::{mouse_area, Canvas};
use iced::{Element, Length, Subscription, Theme};

use stroke::Stroke;

use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};


/// What the animation reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// The platform drew a frame, at the instant it says.
    Tick(Instant),
    /// A touch on the picture, which starts it again from the beginning.
    Restart,
}

/// The signature, and where in its cycle it is.
pub struct Hello {
    stroke: Stroke,
    /// When the current cycle started, or `None` while the clock has not started.
    ///
    /// It is set from the first message the platform sends, not when the app is built — see the
    /// module documentation for the bug that difference is.
    started: Option<Instant>,
    /// The instant of the last frame the platform drew.
    ///
    /// Kept so that the animation is a function of state rather than of `Instant::now()`: nothing in
    /// this file reads a clock, which is what makes the animation reproducible in a test.
    now: Option<Instant>,
    /// The active system preferences.
    preferences: SystemPreferences,
}

impl Default for Hello {
    fn default() -> Self {
        Self::new()
    }
}

impl Hello {
    /// A signature that has not been drawn yet, and whose clock has not started.
    pub fn new() -> Self {
        Self {
            stroke: Stroke::new(),
            started: None,
            now: None,
            preferences: SystemPreferences::default(),
        }
    }

    /// Returns the system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The current theme mode.
    pub fn theme_mode(&self) -> ThemeMode {
        self.preferences.theme
    }

    /// Sets the theme mode.
    pub fn set_theme_mode(&mut self, theme: ThemeMode) {
        self.preferences.theme = theme;
    }

    /// The app's subscriptions: one message per frame, while there is a reason to draw one.
    ///
    /// `iced::window::frames()` is "the platform drew a frame" — the same subscription animates a
    /// window and this panel, because the platform is what produces it either way.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.running() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The theme: the wash's own starting colour, so that the instant between clearing a damaged
    /// region and painting the signature over it is not visible.
    pub fn theme(&self) -> Theme {
        let theme_mode = self.theme_mode();
        if theme_mode.is_light() {
            Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::wash_start_for(theme_mode),
                    ..Palette::LIGHT
                },
            )
        } else {
            Theme::custom(
                "Pomelo",
                Palette {
                    background: style::wash_start_for(theme_mode),
                    ..Palette::DARK
                },
            )
        }
    }

    /// Reacts to one message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => self.advance(now),
            Message::Restart => self.restart(),
        }
    }

    /// Describes the interface for the current state.
    pub fn view(&self) -> Element<'_, Message> {
        mouse_area(
            Canvas::new(canvas::Signature {
                stroke: &self.stroke,
                progress: self.progress(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .on_press(Message::Restart)
        .into()
    }

    /// How far along the animation is, in `0.0..=1.0`. Read by the host and by the tests.
    pub fn progress(&self) -> f32 {
        timing::progress_after(self.elapsed())
    }

    /// The stroke being drawn, for a caller that wants to know what is in it.
    pub fn stroke(&self) -> &Stroke {
        &self.stroke
    }

    /// How long the current cycle has been running, in seconds.
    ///
    /// Zero while the clock has not started, which is what keeps `progress` at zero until the first
    /// frame: an app that has never been drawn is a blank canvas, not a finished picture.
    fn elapsed(&self) -> f32 {
        match (self.started, self.now) {
            (Some(started), Some(now)) => now.saturating_duration_since(started).as_secs_f32(),
            _ => 0.0,
        }
    }

    /// Whether the animation still wants a frame a tick.
    ///
    /// An animation that has not started does: that first frame is what starts its clock. Nothing is
    /// waiting on it, because a host asks this about the app it has on screen and about no other.
    pub fn running(&self) -> bool {
        match self.started {
            None => true,
            Some(_) => timing::running_after(self.elapsed()),
        }
    }

    /// Reads the platform's clock into the animation, starting it if this is the first frame.
    pub fn advance(&mut self, now: Instant) {
        self.started.get_or_insert(now);
        self.now = Some(now);
    }

    /// Starts the animation again from the beginning.
    ///
    /// The clock is *unset* rather than reset to this moment: the cycle starts on the next frame,
    /// which is the frame that draws the blank canvas. Restarting from the moment of the touch
    /// instead would leave the app believing that however long ago the last frame was is time the
    /// new cycle had already been running for.
    pub fn restart(&mut self) {
        self.started = None;
        self.now = None;
    }

    /// Puts the animation `seconds` past its start, as if that much time had gone by, and starts its
    /// clock if it had not started.
    ///
    /// The clock is the one thing an animation cannot be asked to be deterministic about, so this is
    /// how a caller — a test, or a host with its own idea of time — says where it wants to be.
    pub fn seek(&mut self, seconds: f32, now: Instant) {
        let seconds = seconds.max(0.0);

        self.started = Some(now - Duration::from_secs_f32(seconds));
        self.now = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_that_has_not_been_drawn_yet_wants_a_frame() {
        // Nothing may keep the loop awake on its behalf -- the host asks only about the app it has
        // on screen -- so saying yes here is what starts the animation at all.
        let hello = Hello::new();

        assert!(hello.running());
        assert_eq!(hello.progress(), 0.0);
    }

    #[test]
    fn the_clock_starts_on_the_first_frame_and_not_at_construction() {
        // A launcher builds this app at boot and opens it when a finger arrives, so the app can be
        // older than its whole cycle before anyone sees it. Nothing may have moved by then.
        let mut hello = Hello::new();
        let opened = Instant::now();
        let much_later = opened + Duration::from_secs(600);

        assert!(hello.running(), "it has not started, so it cannot be over");
        assert_eq!(hello.progress(), 0.0, "and it has not been drawn");

        hello.advance(much_later);

        assert_eq!(
            hello.progress(),
            0.0,
            "the first frame is the one that starts the clock, and it is blank"
        );

        hello.advance(much_later + Duration::from_millis(700));

        assert!(hello.progress() > 0.0, "and then it draws");
    }

    #[test]
    fn the_animation_is_a_function_of_the_frames_it_is_given() {
        // Never a clock of its own: the same three instants produce the same three pictures, which
        // is what lets a test -- and the panel-side one -- say where the animation is.
        let mut hello = Hello::new();
        let start = Instant::now();

        hello.advance(start);
        assert_eq!(hello.progress(), 0.0);

        hello.advance(start + Duration::from_secs_f32(timing::DELAY + timing::CYCLE / 2.0));
        assert!((hello.progress() - 0.5).abs() < 0.001);

        hello.advance(start + Duration::from_secs_f32(timing::DELAY + timing::CYCLE));
        assert!((hello.progress() - 1.0).abs() < 0.001);
    }

    #[test]
    fn a_restart_puts_it_back_at_the_beginning_running() {
        let mut hello = Hello::new();
        hello.seek(timing::DELAY + timing::CYCLE + 1.0, Instant::now());

        assert!(!hello.running(), "the cycle is over");
        assert!(hello.progress() > 0.99);

        hello.restart();

        assert!(hello.running(), "and it wants frames again");
        assert_eq!(hello.progress(), 0.0, "blank until the next frame draws");
    }

    #[test]
    fn a_restart_after_a_long_pause_does_not_jump_to_the_end() {
        // The failure this guards: `restart` used to reset the clock to *now*, so a signature
        // restarted a minute after the last frame was a minute into its cycle before it drew
        // anything.
        let mut hello = Hello::new();
        let start = Instant::now();

        hello.advance(start);
        hello.advance(start + Duration::from_secs_f32(timing::DELAY + timing::CYCLE));

        assert!(hello.progress() > 0.99, "the picture is finished");

        hello.restart();
        hello.advance(start + Duration::from_secs(600));

        assert_eq!(
            hello.progress(),
            0.0,
            "the cycle starts at the frame after the restart, not 600 s ago"
        );
        assert!(hello.running());
    }

    #[test]
    fn the_animation_stops_asking_for_frames_when_it_is_over() {
        let mut hello = Hello::new();
        let start = Instant::now();
        let finished = Duration::from_secs_f32(timing::DELAY + timing::CYCLE);

        hello.advance(start);
        hello.advance(start + finished);

        assert!(
            hello.running(),
            "the finished picture is held for a moment before the app stops asking"
        );

        hello.advance(start + finished + Duration::from_secs_f32(timing::SETTLE + 0.01));

        assert!(!hello.running(), "and then there is nothing left to draw");
    }

    #[test]
    fn the_theme_can_be_switched() {
        let mut hello = Hello::new();
        assert_eq!(hello.theme_mode(), ThemeMode::Dark);

        hello.set_theme_mode(ThemeMode::Light);
        assert_eq!(hello.theme_mode(), ThemeMode::Light);

        let dark_theme = {
            let mut h = Hello::new();
            h.set_theme_mode(ThemeMode::Dark);
            h.theme()
        };
        let light_theme = hello.theme();
        assert_ne!(dark_theme.palette().background, light_theme.palette().background);
    }

    #[test]
    fn preferences_roundtrip() {
        let mut hello = Hello::new();
        let mut prefs = SystemPreferences::default();
        prefs.theme = ThemeMode::Light;
        hello.set_preferences(prefs);

        assert_eq!(hello.preferences(), prefs);
        assert_eq!(hello.theme_mode(), ThemeMode::Light);
    }
}
