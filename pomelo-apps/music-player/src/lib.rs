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
//! the original's four bands, in the original's proportions (4 : 15 : 3 : 3) and colours.
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
use iced::widget::{button, container, stack, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Padding, Shadow, Subscription, Theme};

use pomelo_hal::wav::format_time;
use pomelo_hal::Board;
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};
pub use model::{MusicPlayerModel, MusicTrack, PlaybackStatus};
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
        }
    }

    /// What the title band shows: the current track's name, or why there is none.
    ///
    /// Read by the host and by the tests.
    pub fn title(&self) -> String {
        match self.model.current_track() {
            Some(track) => track.title.clone(),
            None if self.model.playlist.is_empty() => "No audio".to_string(),
            None => "No track selected".to_string(),
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

    /// The title band.
    ///
    /// The original measured the title in the baked font and truncated it to an ellipsis. A view
    /// has no font metrics here — it is a function of the app's state, and the metrics live in the
    /// renderer — so the band clips instead: a title wider than the panel runs off both edges
    /// rather than carrying a made-up "...".
    fn title_band(&self) -> Element<'_, Message> {
        container(
            text(self.title())
                .size(style::TITLE_FONT)
                .color(style::title_for(self.theme_mode())),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .clip(true)
        .into()
    }

    /// The disc band: the disc, and a tap on it toggles playback, as the original's
    /// `GestureDetector` did.
    fn disc_band(&self) -> Element<'_, Message> {
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

    /// The progress band: the bar, and the two timestamps below it.
    fn progress_band(&self) -> Element<'_, Message> {
        let ratio = if self.model.duration_secs > 0.0 {
            (self.model.position_secs / self.model.duration_secs).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let filled = (style::BAR_WIDTH * ratio).max(style::BAR_MIN_FILL);

        let fill = container(Space::new())
            .width(Length::Fixed(filled))
            .height(Length::Fixed(style::BAR_HEIGHT))
            .style(|_theme| bar_style(style::primary()));

        let track_color = style::track_for(self.theme_mode());
        let track = container(fill)
            .width(Length::Fixed(style::BAR_WIDTH))
            .height(Length::Fixed(style::BAR_HEIGHT))
            .style(move |_theme| bar_style(track_color));

        let text_color = style::text_gray_for(self.theme_mode());
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
                track.into(),
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
    /// Evenly distributed across the row, centered vertically. No volume controls.
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
}

impl Player {
    /// Describes the interface for the current state.
    pub fn view(&self) -> Element<'_, Message> {
        let bands: Vec<Element<'_, Message>> = vec![
            band(self.title_band(), style::TITLE_FLEX),
            band(self.disc_band(), style::DISC_FLEX),
            band(self.progress_band(), style::PROGRESS_FLEX),
            band(self.controls_band(), style::CONTROLS_FLEX),
        ];

        let bg = style::background_for(self.theme_mode());
        container(
            Column::with_children(bands)
                .width(Length::Fill)
                .height(Length::Fill),
        )
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

/// The same, for the progress bar's two halves.
fn bar_style(color: Color) -> container::Style {
    container::Style {
        background: Some(color.into()),
        border: Border {
            radius: style::BAR_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}
