//! The music player, built from iced widgets — **a standard iced program**.
//!
//! The third app on this platform, and the first one whose frame is driven by time: a playing track
//! keeps the disc turning and the position bar advancing, so the app subscribes to
//! `iced::window::frames()` while — and only while — something is playing. That is the whole of the
//! animation, and [`Player::subscription`] is where it is stated: an app that has stopped has no
//! subscriptions, so a stopped player costs no frames at all, in a window or on the panel.
//!
//! The model is not in the widgets. It is [`model`], which the original player used too, because
//! two players that disagree about what "next track" means is a bug waiting for a user — exactly
//! the argument that keeps the calculators' arithmetic out of their widgets. What is here is the screen:
//! a list of what was found, and then the original's bands, in the original's proportions
//! (4 : 15 : 3 : 3) and colours.
//!
//! # Two screens, one app
//!
//! [`Page::Library`] is where the app opens and [`Page::NowPlaying`] is where a row in it leads, and
//! the app comes up on the list because a player that makes a sound the moment it opens is a player
//! making a sound nobody asked for. Back — the button on the playing screen, or the launcher's own —
//! is the step between the two: [`Player::go_back`] answers the launcher's question about whether the
//! press was the app's to keep, exactly as the settings app answers it about its own pages.
//!
//! # Where the board comes from
//!
//! [`Player::new`] takes the board it plays through, and that board is the caller's: the firmware
//! injects the ESP32-S3 one, this project's `main.rs` injects the HAL's desktop simulator, and a
//! test injects whichever it wants to observe. Nothing here decides which hardware it is running
//! on — a default board chosen inside the app is the one thing that would make it the same app on
//! two machines and hide the difference.

mod model;
pub mod style;

use std::sync::Arc;
use std::time::Instant;

use iced::theme::Palette;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::slider::Slider;
use iced::widget::{button, container, stack, text, Column, Row, Scrollable, Space};
use iced::{Alignment, Border, Color, Element, Length, Padding, Shadow, Subscription, Theme};

use pomelo_hal::wav::format_time;
use pomelo_hal::Board;
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};
pub use model::{Library, MusicPlayerModel, MusicTrack, Page, PlaybackStatus};
pub use style::SCREEN;


/// What the player reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// The platform drew a frame, at the instant it says.
    Tick(Instant),
    /// Back to the previous track.
    Previous,
    /// Play, pause, or — from a stop — start the current track.
    PlayPause,
    /// On to the next track.
    Next,
    /// Ten percent quieter.
    VolumeDown,
    /// Ten percent louder.
    VolumeUp,
    /// A finger has dragged the level to this percentage: heard, and not written down yet.
    ///
    /// From the slider rather than from the two buttons beside it, because they are different
    /// questions: a button is a step, and a step is a decision. This is the middle of one — see
    /// [`MusicPlayerModel::preview_volume`] — and [`Message::VolumeCommit`] is the end of it.
    VolumePreview(u8),
    /// The finger has let go of the level: this is the number the board keeps, and comes up at.
    VolumeCommit,
    /// Open the track at this index of the playlist — what a tap on a row in the list means.
    ///
    /// `Open` and not `Select`: choosing a track and playing it are one thing here, and the name says
    /// which way round the app reads them.
    Open(usize),
    /// Leave the playing screen for the list. Does nothing from the list, where the launcher's own
    /// back is the one that means something — see [`Player::go_back`].
    Back,
    /// A finger has dragged the progress bar to this second of the track: shown, and not played yet.
    ///
    /// The backend is not asked to go anywhere until the finger lets go ([`Message::Seek`]): a seek
    /// here is the file opened again further in, and once a frame that is a track made of restarts.
    Scrub(f32),
    /// The finger has let go of the progress bar: play from where it was left.
    Seek,
    /// Look for the card again — what a finger does on the empty screen.
    Rescan,
}

/// The player.
pub struct Player {
    model: MusicPlayerModel,
    /// The instant of the last frame the platform drew, so a frame can say how long the one before
    /// it took. `None` until the first one: a player that has never been drawn has no frame rate.
    last: Option<Instant>,
    preferences: SystemPreferences,
}

impl Player {
    /// The player, with the model scanning its own search directories for tracks.
    ///
    /// The board is the caller's: the firmware injects the ESP32-S3 one, a test injects the
    /// HAL's desktop simulator.
    pub fn new(board: Arc<Board>) -> Self {
        Self {
            model: MusicPlayerModel::new(board),
            last: None,
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

    /// The app's subscriptions: one message per frame, while something is playing.
    ///
    /// A paused track is not animating — the disc holds still and the position stops advancing —
    /// and neither is a stopped one, so both get an empty subscription and the loop sleeps.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.model.is_animating() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The theme: the palette and the background the platform paints behind the tree.
    pub fn theme(&self) -> Theme {
        // A solid background, not a wallpaper primitive -- see the launcher's theme for the
        // measurement that made this the rule: the compositor paints the background over the
        // damage rectangle only, while a full-screen primitive costs the whole screen every frame.
        let theme_mode = self.theme_mode();
        if theme_mode.is_light() {
            Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::background_for(theme_mode),
                    text: style::title_for(theme_mode),
                    ..Palette::LIGHT
                },
            )
        } else {
            Theme::custom(
                "Pomelo",
                Palette {
                    background: style::background_for(theme_mode),
                    text: style::title_for(theme_mode),
                    ..Palette::DARK
                },
            )
        }
    }

    /// Reacts to one message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(now) => {
                // How long the frame before this one took, which is what the disc turns by. The
                // first frame has no predecessor, so it advances by nothing.
                let elapsed = self
                    .last
                    .replace(now)
                    .map_or(0.0, |last| (now - last).as_secs_f32());

                self.model.tick(elapsed);
            }
            Message::Previous => self.model.prev_track(),
            Message::PlayPause => self.model.toggle_play_pause(),
            Message::Next => self.model.next_track(),
            Message::VolumeDown => self.model.volume_down(),
            Message::VolumeUp => self.model.volume_up(),
            // The drag and its release, from the level's slider: heard as it moves, written down when
            // it stops.
            Message::VolumePreview(volume) => self.model.preview_volume(volume),
            Message::VolumeCommit => self.model.commit_volume(),
            Message::Open(index) => self.model.open_track(index),
            // The button on the playing screen. From the list there is nothing to go back to, and the
            // press is dropped: the launcher's back is what leaves the app, and it never routes
            // through here.
            Message::Back => {
                self.model.go_back();
            }
            // The same bargain for the progress bar: the finger moves it, and letting go is what the
            // backend is told.
            Message::Scrub(position) => self.model.scrub(position),
            Message::Seek => self.model.commit_seek(),
            Message::Rescan => self.model.refresh_playlist(),
        }
    }

    /// What the title band shows: the current track's name, or why there is none.
    ///
    /// Read by the host and by the tests.
    pub fn title(&self) -> String {
        match self.model.current_track() {
            Some(track) => track.title.clone(),
            None if self.model.playlist.is_empty() => self.empty_notice(),
            None => "No track selected".to_string(),
        }
    }

    /// What the player says when it has nothing to play.
    ///
    /// An empty slot is not an empty folder, and this is the only place the difference can be said:
    /// "no audio" on a box with no card sends a person looking for a file that was never the
    /// problem.
    pub fn empty_notice(&self) -> String {
        let chinese = self.preferences.language == Language::Chinese;

        if self.model.playlist.is_empty() && self.model.wants_card() {
            return if chinese { "请插入SD卡" } else { "Insert an SD card" }.to_string();
        }

        if chinese { "没有音频" } else { "No audio" }.to_string()
    }

    /// The line under it: where the music is meant to go, and what to do once it is there.
    ///
    /// The folder is spelled out rather than described. It is the one thing a person cannot guess,
    /// and it is the whole of the setup: a card, and a `music` folder on it.
    pub fn empty_hint(&self) -> String {
        let chinese = self.preferences.language == Language::Chinese;

        match self.model.library_dir() {
            Some(dir) if chinese => format!("把音乐放入 {} 后轻触此处重新扫描", dir),
            Some(dir) => format!("Put music in {}, then tap here", dir),
            None if chinese => "插入存储卡后轻触此处重新扫描".to_string(),
            None => "Insert a card, then tap here".to_string(),
        }
    }

    /// Whether the player is asking for a card.
    pub fn wants_card(&self) -> bool {
        self.model.wants_card()
    }

    /// The folder the music is read from, or `None` for an empty slot.
    pub fn library_dir(&self) -> Option<&str> {
        self.model.library_dir()
    }

    /// Looks for the card again — the action behind a tap on an empty screen.
    pub fn rescan(&mut self) {
        self.model.refresh_playlist();
    }

    /// Which screen is showing.
    ///
    /// Read by the tests, and by anything that wants to know whether the player is on its list or on
    /// the track.
    pub fn page(&self) -> Page {
        self.model.page
    }

    /// Leaves the playing screen for the list: `true` when the press was the app's to take.
    ///
    /// The launcher asks this before it backgrounds the app. From the playing screen the answer is
    /// yes and the list comes up; from the list it is no, and the press becomes the launcher's own.
    pub fn go_back(&mut self) -> bool {
        self.model.go_back()
    }

    /// What the list page calls itself.
    pub fn library_title(&self) -> String {
        let chinese = self.preferences.language == Language::Chinese;

        if chinese {
            "音乐库".to_string()
        } else {
            "Music library".to_string()
        }
    }

    /// How many tracks were found, in words.
    ///
    /// Worded and not a bare number, because a lone digit in the corner of a header is one nobody can
    /// place — and worded per language, because "1 tracks" is the kind of thing that makes a finished
    /// screen look like it was left half-done.
    pub fn track_count(&self) -> String {
        let chinese = self.preferences.language == Language::Chinese;
        let count = self.model.playlist.len();

        if chinese {
            return format!("{count} 首");
        }

        if count == 1 {
            "1 track".to_string()
        } else {
            format!("{count} tracks")
        }
    }

    /// Whether a track is playing, paused or stopped.
    pub fn status(&self) -> PlaybackStatus {
        self.model.status
    }

    /// Whether a track is playing, which is the same question [`Player::subscription`] asks.
    ///
    /// Read by the host and by the tests. It is the model's answer, and the app states it twice on
    /// purpose: once here for a caller that wants to know, and once as the subscription that makes
    /// the frames happen.
    pub fn is_animating(&self) -> bool {
        self.model.is_animating()
    }

    /// How far the disc has turned, in degrees. Read by the tests that pin the rate.
    pub fn rotation_angle(&self) -> f32 {
        self.model.rotation_angle
    }

    /// The volume, `0..=100`.
    pub fn volume(&self) -> u8 {
        self.model.volume
    }

    /// How far into the current track playback is, in seconds.
    pub fn position_secs(&self) -> f32 {
        self.model.position_secs
    }

    /// The current track's length, in seconds. Zero when there is nothing to play.
    pub fn duration_secs(&self) -> f32 {
        self.model.duration_secs
    }

    /// Re-scans the playlist, pointed at `dirs` — the device's storage on the device, whatever the
    /// host hands over elsewhere, and a directory a test owns in a test.
    pub fn refresh_playlist_in(&mut self, dirs: &[&str]) {
        self.model.refresh_playlist_in(dirs);
    }

    /// The title band: the way back to the list, then the track's name.
    ///
    /// The original measured the title in the baked font and truncated it to an ellipsis. A view
    /// has no font metrics here — it is a function of the app's state, and the metrics live in the
    /// renderer — so the band clips instead: a title wider than the panel runs off both edges
    /// rather than carrying a made-up "...".
    ///
    /// The button is what the original had no need of: its list was a strip under the title. It sits
    /// on the left with the same width of nothing on the right, because a centred title that is
    /// centred on what is left of the band beside a button is a title a button's width off the
    /// middle of the page.
    fn title_band(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();

        container(
            Row::with_children(vec![
                back_button(theme_mode),
                container(
                    text(self.title())
                        .size(style::TITLE_FONT)
                        .color(style::title_for(theme_mode)),
                )
                .width(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .clip(true)
                .into(),
                Space::new()
                    .width(Length::Fixed(style::BACK_BUTTON))
                    .into(),
            ])
            .width(Length::Fill)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill)
        .into()
    }

    /// The disc band: the disc, and a tap on it toggles playback, as the original's
    /// `GestureDetector` did.
    ///
    /// With nothing to play it is the empty screen instead: what the box is waiting for, and a tap
    /// to look again. The tap is on the whole band and not on a button, because a person who has
    /// just pushed a card in should not have to aim.
    fn disc_band(&self) -> Element<'_, Message> {
        if self.model.playlist.is_empty() {
            return self.empty_band();
        }

        let theme_mode = self.theme_mode();
        container(
            button(self.disc())
                .padding(0)
                .style(move |_theme, _status| button::Style {
                    // The disc is its own picture; the button around it is only a target.
                    background: None,
                    text_color: style::button_icon_for(theme_mode),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: false,
                })
                .on_press(Message::PlayPause),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// The disc band with nothing to play: the state, where the music goes, and a tap to look again.
    ///
    /// A card can arrive while the box is running and the app is not restarted when it does, so a
    /// screen that only reported what it found on opening would stay wrong until the next launch.
    /// The tap is the way out of that — deliberately the whole band, so a finger does not have to
    /// find it.
    fn empty_band(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();

        let lines: Vec<Element<'_, Message>> = vec![
            text(self.empty_notice())
                .size(style::TITLE_FONT)
                .color(style::label_paused())
                .into(),
            Space::new().height(Length::Fixed(style::BAR_GAP)).into(),
            text(self.empty_hint())
                .size(style::TIME_FONT)
                .color(style::text_gray_for(theme_mode))
                .into(),
        ];

        button(
            container(
                Column::with_children(lines)
                    .align_x(Alignment::Center)
                    .spacing(0),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(0)
        .style(|_theme, _status| button::Style {
            // The band is a target, not a picture: the words in it are the picture.
            background: None,
            text_color: style::label_paused(),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        })
        .on_press(Message::Rescan)
        .into()
    }

    /// The disc: a dark circle with a label at its centre and one groove marker on it.
    ///
    /// Three things differ from the original on purpose. It blitted a baked 128x128 album-art
    /// bitmap, and iced's `image` feature would drag the `image` crate into the firmware for one
    /// picture, so the disc is built from containers instead — the renderer clamps a border radius
    /// to half the box, which turns a square into a circle. The label is purple while a track plays
    /// and grey otherwise, which is the theme's own rule. And the marker exists at all because the
    /// model has always tracked `rotation_angle` and a radially symmetric disc would hide it: the
    /// rotation is the animation, and it has to be visible to be one.
    fn disc(&self) -> Element<'_, Message> {
        let label_color = match self.model.status {
            PlaybackStatus::Playing => style::label_playing(),
            _ => style::label_paused(),
        };

        let spindle = circle(Space::new(), style::SPINDLE_SIZE, style::spindle());

        let label = container(spindle)
            .width(Length::Fixed(style::LABEL_SIZE))
            .height(Length::Fixed(style::LABEL_SIZE))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_theme| disc_style(label_color));

        let face = container(label)
            .width(Length::Fixed(style::DISC_SIZE))
            .height(Length::Fixed(style::DISC_SIZE))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_theme| disc_style(style::vinyl_outer()));

        // Where the marker sits on the disc. Down the screen is `+y`, so an increasing angle
        // turns the marker the way a record turns.
        let angle = self.model.rotation_angle.to_radians();
        let centre = style::DISC_SIZE / 2.0;
        let half = style::MARKER_SIZE / 2.0;

        let marker = container(circle(
            Space::new(),
            style::MARKER_SIZE,
            style::vinyl_groove(),
        ))
        .padding(Padding {
            top: centre + style::MARKER_ORBIT * angle.sin() - half,
            left: centre + style::MARKER_ORBIT * angle.cos() - half,
            ..Padding::ZERO
        });

        stack![face, marker].into()
    }

    /// The progress band: the bar a finger can drag, and the two timestamps below it.
    ///
    /// The bar is a slider and no longer two containers of hand-picked widths, because it is the one
    /// thing on this screen a hand wants to be on: the rail says where playback is, and the handle is
    /// what the hand takes hold of. The `BAR_MIN_FILL` sliver went with the containers — a handle
    /// standing at the left end of the rail says where a track begins better than 4 px of purple did.
    ///
    /// The timestamps are the model's numbers and not the slider's, which is what makes a drag read
    /// out loud: [`Player::update`] follows the finger through [`Message::Scrub`], so the left-hand
    /// stamp shows the second being aimed at before anything is played from there.
    fn progress_band(&self) -> Element<'_, Message> {
        let duration = self.model.duration_secs.max(0.0);
        let position = self.model.position_secs.clamp(0.0, duration);
        let theme_mode = self.theme_mode();

        let bar = Slider::new(0.0..=duration, position, Message::Scrub)
            .step(style::SEEK_STEP)
            .width(Length::Fixed(style::BAR_WIDTH))
            .height(style::BAR_TOUCH)
            .on_release(Message::Seek)
            .style(move |_theme, _status| style::slider_style(theme_mode));

        let text_color = style::text_gray_for(theme_mode);
        let stamps: Vec<Element<'_, Message>> = vec![
            text(format_time(self.model.position_secs))
                .size(style::TIME_FONT)
                .color(text_color)
                .into(),
            Space::new().width(Length::Fill).into(),
            text(format_time(self.model.duration_secs))
                .size(style::TIME_FONT)
                .color(text_color)
                .into(),
        ];

        let stamps = Row::with_children(stamps).width(Length::Fixed(style::BAR_WIDTH));

        container(
            Column::with_children(vec![
                bar.into(),
                Space::new().height(Length::Fixed(style::BAR_GAP)).into(),
                stamps.into(),
            ])
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// The controls band: backward / play-pause / forward buttons using Material Symbols.
    ///
    /// Evenly distributed across the row, centered vertically. The volume has a band of its own below
    /// it: the original kept the pair inside this band, and at these shares that leaves 69 px for a
    /// 60 px play button and a 36 px row of its own.
    fn controls_band(&self) -> Element<'_, Message> {
        let playing = self.model.status == PlaybackStatus::Playing;

        let controls: Vec<Element<'_, Message>> = vec![
            Space::new().width(Length::Fill).into(),
            prev_button(self.theme_mode()),
            Space::new().width(Length::Fill).into(),
            play_pause_button(playing),
            Space::new().width(Length::Fill).into(),
            next_button(self.theme_mode()),
            Space::new().width(Length::Fill).into(),
        ];

        container(
            Row::with_children(controls)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// The volume band: quieter, the level, and louder.
    ///
    /// The volume is the board's and not this page's — the backend writes it to its own partition on
    /// the way past, and reads it back when the board boots — so this band is a view of one number
    /// with three ways of moving it, and nothing of its own to keep. See
    /// `pomelo_hal::music_settings`.
    ///
    /// The level is a slider and the pair beside it are buttons, which is not a redundancy: a finger
    /// that knows where it wants to land drags, and a thumb holding the box steps. The pair still moves
    /// in tens, because a button is a step and a step is a decision. The drag is heard as it goes and
    /// written down only when the finger stops — see [`Message::VolumePreview`] — so that a flash
    /// partition is erased once per gesture rather than once per frame.
    ///
    /// It is the progress bar's instrument: same width, same rail, same handle.
    fn volume_band(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let volume = self.model.volume;

        let level = Slider::new(0..=100u8, volume, Message::VolumePreview)
            .step(style::VOLUME_STEP)
            .width(Length::Fixed(style::VOLUME_BAR_WIDTH))
            .height(style::BAR_TOUCH)
            .on_release(Message::VolumeCommit)
            .style(move |_theme, _status| style::slider_style(theme_mode));

        let readout = container(
            text(format!("{volume}%"))
                .size(style::VOLUME_FONT)
                .color(style::button_icon_for(theme_mode)),
        )
        .width(Length::Fixed(style::VOLUME_READOUT))
        .height(Length::Fixed(style::VOLUME_BUTTON))
        .center_x(Length::Fill)
        .center_y(Length::Fill);

        let controls: Vec<Element<'_, Message>> = vec![
            volume_button(Icon::VOLUME_DOWN, theme_mode, Message::VolumeDown),
            Space::new().width(Length::Fixed(style::VOLUME_GAP)).into(),
            level.into(),
            Space::new().width(Length::Fixed(style::VOLUME_GAP)).into(),
            readout.into(),
            Space::new().width(Length::Fixed(style::VOLUME_GAP)).into(),
            volume_button(Icon::VOLUME_UP, theme_mode, Message::VolumeUp),
        ];

        container(
            Row::with_children(controls)
                .width(Length::Fixed(style::BAR_WIDTH))
                .align_y(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}

impl Player {
    /// Describes the interface for the current state.
    ///
    /// Two screens, and the app opens on the list: opening the player is not asking it to play
    /// anything, and a board in a room that starts a track because an app was opened is a board with
    /// a mind of its own.
    pub fn view(&self) -> Element<'_, Message> {
        match self.model.page {
            Page::Library => self.library_view(),
            Page::NowPlaying => self.now_playing_view(),
        }
    }

    /// The playing screen: the original's bands, with the volume row under them.
    fn now_playing_view(&self) -> Element<'_, Message> {
        let bands: Vec<Element<'_, Message>> = vec![
            band(self.title_band(), style::TITLE_FLEX),
            band(self.disc_band(), style::DISC_FLEX),
            band(self.progress_band(), style::PROGRESS_FLEX),
            band(self.controls_band(), style::CONTROLS_FLEX),
            band(self.volume_band(), style::VOLUME_FLEX),
        ];

        self.page_shell(
            Column::with_children(bands)
                .width(Length::Fill)
                .height(Length::Fill),
        )
    }

    /// The list screen: what was found, in the order it was found.
    ///
    /// A rescan replaces the list rather than adding to it, so a row's number is the folder's order
    /// and not the order of the taps. A tap on one is [`Message::Open`], which is the only way onto
    /// the playing screen.
    fn library_view(&self) -> Element<'_, Message> {
        let head = container(self.library_header())
            .width(Length::Fill)
            .height(Length::Fixed(style::HEADER_HEIGHT));

        /* With nothing to list, the body is the empty screen — which is also the way out of a card
         * that turned up after the app was opened. The listening screen puts that same band in place
         * of its disc; here there is no disc to tap, and the list page is the one a person is looking
         * at when they push the card in. */
        let body: Element<'_, Message> = if self.model.playlist.is_empty() {
            self.empty_band()
        } else {
            self.track_list()
        };

        self.page_shell(
            Column::with_children(vec![head.into(), body])
                .width(Length::Fill)
                .height(Length::Fill),
        )
    }

    /// The list's head: what the page is, and how much of it there is.
    fn library_header(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();

        container(
            Row::with_children(vec![
                text(self.library_title())
                    .size(style::HEADER_FONT)
                    .color(style::title_for(theme_mode))
                    .into(),
                Space::new().width(Length::Fill).into(),
                text(self.track_count())
                    .size(style::TIME_FONT)
                    .color(style::text_gray_for(theme_mode))
                    .into(),
            ])
            .width(Length::Fill)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill)
        .into()
    }

    /// The rows, scrolling when there are more of them than fit.
    fn track_list(&self) -> Element<'_, Message> {
        let rows: Vec<Element<'_, Message>> = (0..self.model.playlist.len())
            .map(|index| self.track_row(index))
            .collect();

        // No scrollbar: the panel has a finger and not a pointer, and a bar a hair wide is something
        // to look at rather than something to grab.
        Scrollable::new(
            Column::with_children(rows)
                .width(Length::Fill)
                .spacing(style::ROW_GAP),
        )
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// One row: where the track falls in the list, what it is called, and how long it runs.
    ///
    /// The row that is playing is marked twice — its colour, and the mark that takes its number's
    /// place — because the list is also how a person sees what is going on while they look for the
    /// next thing, and a colour alone is a mark some people cannot read.
    fn track_row(&self, index: usize) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let track = &self.model.playlist[index];
        let playing =
            index == self.model.current_index && self.model.status != PlaybackStatus::Stopped;

        let background = if playing {
            style::row_playing_for(theme_mode)
        } else {
            style::button_bg_for(theme_mode)
        };
        let title_color = if playing {
            style::primary()
        } else {
            style::title_for(theme_mode)
        };
        let lead_color = if playing {
            style::primary()
        } else {
            style::text_gray_for(theme_mode)
        };

        let lead: Element<'_, Message> = if playing {
            text(Icon::GRAPHIC_EQ.glyph())
                .font(pomelo_material_symbols::font())
                .size(style::ROW_MARK)
                .color(lead_color)
                .into()
        } else {
            text(format!("{}", index + 1))
                .size(style::TIME_FONT)
                .color(lead_color)
                .into()
        };

        let row: Element<'_, Message> = Row::with_children(vec![
            container(lead)
                .width(Length::Fixed(style::ROW_INDEX_WIDTH))
                .center_y(Length::Fill)
                .into(),
            container(
                text(track.title.clone())
                    .size(style::ROW_FONT)
                    .color(title_color),
            )
            .width(Length::Fill)
            .center_y(Length::Fill)
            .clip(true)
            .into(),
            text(track_length(track))
                .size(style::TIME_FONT)
                .color(lead_color)
                .into(),
        ])
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .into();

        row_button(row, background, theme_mode, Message::Open(index))
    }

    /// A page: the theme's background, and the page's padding around whatever is in it.
    ///
    /// Both screens are built inside one of these, so that the two cannot drift apart in their
    /// margins: the padding is the original's, and it belongs to the page rather than to either
    /// screen. It is not called `page`: that name is [`Player::page`]'s, which answers a different
    /// question — which screen is up — and two methods of one name are one name too many.
    fn page_shell<'a>(&self, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        let bg = style::background_for(self.theme_mode());

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            })
            .padding(Padding {
                top: style::PAGE_TOP,
                bottom: style::PAGE_BOTTOM,
                ..Padding::ZERO
            })
            .into()
    }
}

/// One page band: as wide as the page, and as tall as its share of what is left.
fn band<'a>(child: Element<'a, Message>, flex: u16) -> Element<'a, Message> {
    container(child)
        .width(Length::Fill)
        .height(Length::FillPortion(flex))
        .into()
}

/// The previous / backward button.
fn prev_button(theme_mode: ThemeMode) -> Element<'static, Message> {
    icon_button(
        Icon::SKIP_PREVIOUS,
        style::ICON_SMALL,
        style::button_icon_for(theme_mode),
        style::BUTTON_SMALL,
        style::button_bg_for(theme_mode),
        style::button_pressed_for(theme_mode),
        Message::Previous,
    )
}

/// The play/pause button: the primary action, in the theme's primary colour.
fn play_pause_button(playing: bool) -> Element<'static, Message> {
    let icon = if playing {
        Icon::PAUSE
    } else {
        Icon::PLAY_ARROW
    };

    icon_button(
        icon,
        style::ICON_PLAY,
        Color::WHITE,
        style::BUTTON_PLAY,
        style::primary(),
        style::primary_pressed(),
        Message::PlayPause,
    )
}

/// The next / forward button.
fn next_button(theme_mode: ThemeMode) -> Element<'static, Message> {
    icon_button(
        Icon::SKIP_NEXT,
        style::ICON_SMALL,
        style::button_icon_for(theme_mode),
        style::BUTTON_SMALL,
        style::button_bg_for(theme_mode),
        style::button_pressed_for(theme_mode),
        Message::Next,
    )
}

/// The back button: the way from the playing screen to the list.
///
/// A small transport button, because that is what it is — the same grey and the same round shape as
/// its neighbours, one size down so that the title beside it stays the widest thing in the band.
fn back_button(theme_mode: ThemeMode) -> Element<'static, Message> {
    icon_button(
        Icon::ARROW_BACK,
        style::ICON_BACK,
        style::button_icon_for(theme_mode),
        style::BACK_BUTTON,
        style::button_bg_for(theme_mode),
        style::button_pressed_for(theme_mode),
        Message::Back,
    )
}

/// One of the volume pair: quieter on the left, louder on the right.
///
/// Its own grey, and not the transport's: "the grey discs" is what the previous / play / next trio
/// means, and a fourth and fifth one under them would make it mean nothing.
fn volume_button(icon: Icon, theme_mode: ThemeMode, message: Message) -> Element<'static, Message> {
    icon_button(
        icon,
        style::ICON_VOLUME,
        style::button_icon_for(theme_mode),
        style::VOLUME_BUTTON,
        style::volume_bg(),
        style::volume_pressed(),
        message,
    )
}

/// A track's row, wrapped in the button that opens it.
///
/// `background` is the caller's because the row that is playing is the one that is coloured, and that
/// decision belongs to whoever knows which row that is. The padding is horizontal only: the row's
/// height is the finger's, and the content inside it is centred vertically by the row itself.
fn row_button<'a>(
    content: Element<'a, Message>,
    background: Color,
    theme_mode: ThemeMode,
    message: Message,
) -> Element<'a, Message> {
    button(content)
        .width(Length::Fill)
        .height(Length::Fixed(style::ROW_HEIGHT))
        .padding(Padding {
            left: style::ROW_PADDING,
            right: style::ROW_PADDING,
            ..Padding::ZERO
        })
        .style(move |_theme, status| button::Style {
            background: Some(
                match status {
                    button::Status::Pressed => style::button_pressed_for(theme_mode),
                    _ => background,
                }
                .into(),
            ),
            text_color: style::title_for(theme_mode),
            border: Border {
                radius: style::ROW_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        })
        .on_press(message)
        .into()
}

/// How long a track runs, as the progress band's timestamps spell it.
///
/// The length comes from the metadata the scan read, and a track whose metadata did not survive the
/// probe shows the dashes rather than a zero: `00:00` is a claim about the file, and this is the
/// absence of one.
fn track_length(track: &MusicTrack) -> String {
    match track.metadata {
        Some(meta) => format_time(meta.duration_secs),
        None => "--:--".to_string(),
    }
}

/// A round button displaying a Material Symbols icon glyph.
fn icon_button(
    icon: Icon,
    icon_size: f32,
    icon_color: Color,
    button_size: f32,
    fill: Color,
    pressed: Color,
    message: Message,
) -> Element<'static, Message> {
    button(
        container(
            text(icon.glyph())
                .font(pomelo_material_symbols::font())
                .size(icon_size)
                .color(icon_color),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fixed(button_size))
    .height(Length::Fixed(button_size))
    .padding(0)
    .style(move |_theme, status| button::Style {
        background: Some(
            match status {
                button::Status::Pressed => pressed,
                _ => fill,
            }
            .into(),
        ),
        text_color: icon_color,
        border: Border {
            radius: style::ROUND.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    })
    .on_press(message)
    .into()
}

/// A square container of `size`, painted `color` and rounded as far as the renderer allows.
fn circle<'a>(content: Space, size: f32, color: Color) -> container::Container<'a, Message> {
    container(content)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_theme| disc_style(color))
}

/// A container with a background and a radius, and nothing else: no border, no shadow.
fn disc_style(color: Color) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: Border {
            radius: style::ROUND.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

