//! The web manager, built from iced widgets — **a standard iced program**.
//!
//! One switch and one address. The switch turns on [`Board::web`][pomelo_hal::Board::web] — the
//! board's HTTP management server — and the address is where a browser on the same network finds it.
//!
//! # The server is the board's; this is the panel over it
//!
//! *What* is served is not this app's business and not this crate's: the page is the firmware's
//! (`board_web.c`, baked into the image as an embedded file), and it manages the files and the
//! network there. The trait this app sees has three methods — `start`, `stop`, `status` — and no
//! route, no file and no request among them, which is exactly why the app can be this small: it
//! never learns what a request is.
//!
//! What that buys is one implementation of the switch on both sides of the `target_os` line. The
//! desktop simulator reports the same [`WebStatus`] the board does, so the layout, the address rule
//! and the tests around them are the same code whether a window or a panel is drawing it.
//!
//! # There is no password, and the app says so before it starts the server
//!
//! Anyone on the same network can read and write the box's files and change its network. The server
//! is therefore **off until somebody asks for it**, and the note under the switch says why a person
//! might not want to ask. Starting it on the app's own initiative — the moment the tile is tapped —
//! would make the decision for whoever holds the box, which is the one thing this page exists to
//! avoid.
//!
//! # The address is a rule, not a preference
//!
//! Whether the URL is drawn is decided by [`WebStatus::url`], in the HAL, because it is a fact about
//! the machine and not a style: a server that is down serves nothing, and a box that is not on a
//! network has no address a browser on that network can reach. Two questions the app would
//! otherwise answer for itself, and answer differently from the board — see that method.
//!
//! # The board is injected
//!
//! The app is handed an `Arc<pomelo_hal::Board>` — the launcher passes the one it shares with every
//! app it hosts, and `main.rs` builds the desktop simulator — so the switch is a call and the
//! address is a reading, both against the same board the rest of the system uses.

pub mod style;

use std::sync::Arc;

use iced::theme::Palette;
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Theme};
use pomelo_hal::{Board, HalError, WebStatus};
use pomelo_material_symbols::Icon;

pub use pomelo_widgets::{Language, SystemPreferences, ThemeMode};
pub use style::SCREEN;

/// What the web manager reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// The switch: bring the server up if it is down, take it down if it is up.
    Toggle,
}

/// The web manager.
///
/// The status is a field and not a fresh read per frame for the same reason the trait returns a
/// snapshot: it is read to draw a switch, and the only thing that changes it is this app.
pub struct WebManager {
    board: Arc<Board>,
    status: WebStatus,
    /// Why the last start or stop failed, if it did. Cleared by the next attempt.
    error: Option<HalError>,
    preferences: SystemPreferences,
}

impl WebManager {
    /// The web manager, and the board whose server it switches.
    ///
    /// The board's own answer is read once, at construction: an app opened onto a server that is
    /// already up — one left running by a previous visit, or by another way of starting it — shows
    /// the switch in the position it is actually in.
    pub fn new(board: Arc<Board>) -> Self {
        let status = board.web().status();

        Self {
            board,
            status,
            error: None,
            preferences: SystemPreferences::default(),
        }
    }

    /// Whether the management server is up.
    pub fn is_running(&self) -> bool {
        self.status.running
    }

    /// Whether it is up, and on which port.
    pub fn status(&self) -> WebStatus {
        self.status
    }

    /// Why the last attempt failed, or `None` if it did not.
    pub fn error(&self) -> Option<&HalError> {
        self.error.as_ref()
    }

    /// The address a browser on this network should open, while there is one to give.
    ///
    /// `None` while the server is down or the box is off the network — the rule is
    /// [`WebStatus::url`]'s, and it is asked rather than re-implemented.
    pub fn url(&self) -> Option<String> {
        self.status.url(&self.board.wifi().status())
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The active interface language.
    pub fn language(&self) -> Language {
        self.preferences.language
    }

    /// Picks the string for the active language.
    ///
    /// A static table and an exhaustive `match`, like the settings app's `i18n` and for the same
    /// reason: two languages and a handful of labels do not need a translation framework.
    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        match self.language() {
            Language::Chinese => zh,
            Language::English => en,
        }
    }

    /// The server's answer in the interface's language.
    fn error_text(&self, error: &HalError) -> String {
        match error {
            HalError::InvalidArg => self.t("端口无效", "invalid port").to_string(),
            HalError::Busy => self.t("服务器忙", "the server is busy").to_string(),
            HalError::NotSupported => self.t("此设备不支持", "not supported on this device").to_string(),
            HalError::NotInitialized => self.t("网络未就绪", "the network is not ready").to_string(),
            HalError::Timeout => self.t("操作超时", "the operation timed out").to_string(),
            HalError::Io(_) => self.t("网络 I/O 错误", "a network I/O error").to_string(),
            HalError::Internal(code) => {
                format!("{} ({code})", self.t("平台错误", "platform error"))
            }
        }
    }

    /// Starts the server if it is down and stops it if it is up.
    ///
    /// The port is [`WebStatus::DEFAULT_PORT`] — 80, the one address a person can read out loud and
    /// type. A failure is kept, not swallowed: the switch has two positions and only one of them was
    /// reached, so the page would otherwise claim something the board did not do.
    fn toggle(&mut self) {
        self.error = None;

        let mut web = self.board.web();
        let was_running = web.status().running;

        let result = if was_running {
            web.stop()
        } else {
            web.start(WebStatus::DEFAULT_PORT)
        };

        if let Err(error) = result {
            self.error = Some(error);
        }

        // The board is the authority, not the call: a start that half-succeeded reports what the
        // backend actually holds.
        self.status = web.status();
    }

    /// The heading: the app's name, and what it is for.
    fn heading(&self) -> Element<'_, Message> {
        let theme_mode = self.preferences.theme;

        column![
            text(self.t("网页管理", "Web Manager"))
                .size(style::TITLE_FONT)
                .color(style::title_for(theme_mode)),
            text(self.t("局域网文件与网络管理", "Files & network over your LAN"))
                .size(style::SUBTITLE_FONT)
                .color(style::muted_for(theme_mode)),
        ]
        .spacing(6.0)
        .align_x(Alignment::Center)
        .into()
    }

    /// The state of the server, and the address to open it at.
    fn status_card(&self, url: Option<&str>) -> Element<'_, Message> {
        let theme_mode = self.preferences.theme;
        let running = self.status.running;

        let state_colour = if running {
            style::running_for(theme_mode)
        } else if self.error.is_some() {
            style::error_for(theme_mode)
        } else {
            style::muted_for(theme_mode)
        };

        let state_glyph = if running {
            Icon::CHECK_CIRCLE
        } else {
            Icon::POWER_SETTINGS_NEW
        };

        let state_text = if running {
            self.t("服务器运行中", "Server running")
        } else if self.error.is_some() {
            self.t("启动失败", "Could not start")
        } else {
            self.t("服务器已停止", "Server stopped")
        };

        // The address line, or the reason there is not one. "Running but no address" is a state of
        // its own: the server is up and nothing on the network can reach it yet.
        let detail = match url {
            Some(url) => url.to_string(),
            None if running => self
                .t("未连接 Wi-Fi：局域网内暂无地址", "Not on Wi-Fi: no LAN address yet")
                .to_string(),
            None => self
                .t("启动后即可在浏览器中打开", "Start it, then open it in a browser")
                .to_string(),
        };

        let mut body = column![
            row![
                text(state_glyph.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(style::STATUS_FONT)
                    .color(state_colour),
                text(state_text)
                    .size(style::STATUS_FONT)
                    .color(state_colour),
            ]
            .spacing(8.0)
            .align_y(Alignment::Center),
            text(detail)
                .size(style::ADDRESS_FONT)
                .color(style::body_for(theme_mode)),
        ]
        .spacing(12.0)
        .align_x(Alignment::Center);

        if let Some(error) = &self.error {
            body = body.push(
                text(self.error_text(error))
                    .size(style::FOOTNOTE_FONT)
                    .color(style::error_for(theme_mode)),
            );
        }

        container(body)
            .width(Length::Fill)
            .padding(style::CARD_PADDING)
            .style(move |_theme| container::Style {
                background: Some(style::card_for(theme_mode).into()),
                border: Border {
                    color: style::card_border_for(theme_mode),
                    width: style::CARD_BORDER_WIDTH,
                    radius: style::CARD_RADIUS.into(),
                },
                ..container::Style::default()
            })
            .into()
    }

    /// The switch, which is what a finger is aimed at.
    fn switch(&self) -> Element<'_, Message> {
        let theme_mode = self.preferences.theme;
        let running = self.status.running;

        let label = if running {
            self.t("停止服务器", "Stop server")
        } else {
            self.t("启动服务器", "Start server")
        };

        let (fill, pressed) = if running {
            (
                style::stop_button_for(theme_mode),
                style::stop_button_pressed_for(theme_mode),
            )
        } else {
            (
                style::button_for(theme_mode),
                style::button_pressed_for(theme_mode),
            )
        };

        button(
            container(text(label).size(style::BUTTON_FONT).color(Color::WHITE))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .padding(style::BUTTON_PADDING)
        .on_press(Message::Toggle)
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Pressed => pressed,
                _ => fill,
            };

            button::Style {
                background: Some(background.into()),
                text_color: Color::WHITE,
                border: Border {
                    radius: style::BUTTON_RADIUS.into(),
                    ..Border::default()
                },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .into()
    }

    /// The note: the server has no password, and that is the person's to weigh.
    fn note(&self) -> Element<'_, Message> {
        let theme_mode = self.preferences.theme;
        let colour = style::muted_for(theme_mode);

        row![
            text(Icon::INFO.glyph())
                .font(pomelo_material_symbols::font())
                .size(style::FOOTNOTE_FONT)
                .color(colour),
            text(self.t(
                "无密码保护，请仅在可信网络中开启",
                "No password: only on a trusted network"
            ))
            .size(style::FOOTNOTE_FONT)
            .color(colour),
        ]
        .spacing(6.0)
        .align_y(Alignment::Center)
        .into()
    }
}

// There is deliberately no `Default`: a web manager without a board would have no server to switch,
// and `Board::simulated` does not exist on the device — a `Default` calling it would be a host-only
// impl on a crate the firmware compiles. `new` is the only door, and iced's `application` takes it.

impl WebManager {
    /// The web manager's theme.
    ///
    /// A solid background, not a wallpaper primitive: the compositor paints the background over the
    /// damage rectangle only, while a full-screen primitive costs the whole screen every frame. The
    /// launcher's theme carries the measurement.
    pub fn theme(&self) -> Theme {
        if self.preferences.theme.is_light() {
            Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::background_for(ThemeMode::Light),
                    ..Palette::LIGHT
                },
            )
        } else {
            Theme::custom(
                "PomeloDark",
                Palette {
                    background: style::background_for(ThemeMode::Dark),
                    ..Palette::DARK
                },
            )
        }
    }

    /// Reacts to a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Toggle => self.toggle(),
        }
    }

    /// The whole screen.
    pub fn view(&self) -> Element<'_, Message> {
        let url = self.url();

        let body = column![
            self.heading(),
            self.status_card(url.as_deref()),
            self.switch(),
            self.note(),
        ]
        .spacing(style::SECTION_GAP)
        .width(Length::Fill)
        .align_x(Alignment::Center);

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(style::PAGE_PADDING)
            .center_y(Length::Fill)
            .into()
    }
}
