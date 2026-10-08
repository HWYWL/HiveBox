//! The terminal, built from iced widgets -- **a standard iced program**.
//!
//! The shell -- the model, the commands, the storage helpers -- is [`shell`], which the original
//! terminal used too, because two shells that disagree with each other is a bug waiting for a user.
//! What is here is the face: the VS Code transcript, the active input line with its cursor, and the
//! iOS on-screen keyboard — which `pomelo-widgets` draws, because the settings app draws the same
//! one for a Wi-Fi password.
//!
//! The input line is drawn and driven by this app on purpose. `iced::widget::text_input` assumes a
//! physical keyboard and a focus system; on a panel with neither, the app owns the string (in
//! [`shell`]) and the keys write to it, exactly as the original did.
//!
//! # The size comes from the platform
//!
//! The keyboard band and the wrapping width are derived from how big the screen is, so the app has
//! to be told. It is told the way a real terminal is: [`Message::Resized`], from
//! [`Terminal::subscription`], which subscribes to `iced::window::resize_events()`. On a desktop
//! that is the window being resized; on the panel the platform sends the same message with the
//! panel's size (the launcher does it when it opens the terminal). Between the two the app starts
//! at [`SCREEN`], the design size, so it draws something sensible even if nobody ever tells it.

mod commands;
mod shell;

pub mod style;

use iced::theme::Palette;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{column, container, text, Column, Row, Scrollable, Space};
use iced::{Alignment, Color, Element, Length, Padding, Size, Subscription, Theme};
use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};

use crate::shell::{HistoryEntry, TerminalModel};

pub use shell::KeyAction;
pub use style::SCREEN;


/// An element this app builds. Everything is owned, so the helpers do not borrow the app.
type El = Element<'static, Message>;

/// What the terminal reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// A key of the on-screen keyboard was pressed.
    Key(KeyAction),
    /// The screen changed size: a window on a desktop, the panel on the board.
    Resized(Size),
}

/// The terminal.
pub struct Terminal {
    model: TerminalModel,
    /// The screen the app is drawing into. The keyboard's geometry and the wrapping width are
    /// derived from it, as the original's `MediaQuery` was. It starts at the design size and is
    /// replaced by whoever tells the app the truth -- see the module documentation.
    size: Size,
    preferences: SystemPreferences,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            model: TerminalModel::new(),
            size: Size::new(SCREEN as f32, SCREEN as f32),
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

    /// The app's subscriptions: the size of the screen, and nothing else.
    ///
    /// Not an animation, not a timer -- the terminal only changes when it is typed into or when the
    /// screen changes shape, so there is nothing to wake up for.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::window::resize_events().map(|(_window, size)| Message::Resized(size))
    }

    /// The screen this app is laying out for.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The transcript as plain text lines: a prompt renders as `rust:cwd$ cmd`, an output line as
    /// its text. Read by the host and by the tests.
    pub fn history_lines(&self) -> Vec<String> {
        self.model.history_lines()
    }

    /// The line being typed. Read by the host and by the tests.
    pub fn current_input(&self) -> &str {
        self.model.input()
    }

    /// The current working directory. Read by the host and by the tests.
    pub fn cwd(&self) -> &str {
        self.model.cwd()
    }

    /// The keyboard band's height: about 48% of the screen, clamped to 180..240 px.
    ///
    /// The formula is the keyboard crate's, so the band a password prompt gets in the settings app
    /// is the same share of the same screen.
    pub fn keyboard_height(&self) -> f32 {
        style::keyboard_height(self.size.height)
    }

    /// One line of the transcript, in its fixed 24 px cell.
    fn cell(child: El) -> El {
        container(child)
            .width(Length::Fill)
            .height(Length::Fixed(style::LINE_HEIGHT))
            .align_y(Alignment::Center)
            .into()
    }

    /// The `pomelo:/internal$ ` prompt's coloured parts, without what follows.
    fn prompt_prefix(short_cwd: String, theme_mode: ThemeMode) -> Vec<El> {
        vec![
            label(shell::DEFAULT_USER, style::prompt_user_for(theme_mode)),
            label(":", style::prompt_punct_for(theme_mode)),
            label(short_cwd, style::prompt_dir_for(theme_mode)),
            label("$ ", style::prompt_sym_for(theme_mode)),
        ]
    }

    /// The visible tail of the input line and the cursor block after it.
    fn input_tail(&self) -> El {
        let theme_mode = self.theme_mode();
        let short_cwd = self.model.short_cwd();
        let prompt_w = style::measure(&format!("{}:{short_cwd}$ ", shell::DEFAULT_USER));
        let max_text_w = self.text_width();
        let avail = (max_text_w - prompt_w - style::CURSOR_WIDTH - 8.0).max(40.0);
        let visible = visible_tail(self.model.input(), avail);

        let cursor_color = style::command_text_for(theme_mode);
        let cursor_block = container(Space::new())
            .width(Length::Fixed(style::CURSOR_WIDTH))
            .height(Length::Fixed(style::CURSOR_HEIGHT))
            .style(move |_theme| container::Style {
                background: Some(cursor_color.into()),
                ..container::Style::default()
            });
        let cursor = container(cursor_block).padding(Padding {
            top: style::CURSOR_OFFSET_Y,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        });

        let mut parts = Self::prompt_prefix(short_cwd, theme_mode);
        parts.push(
            Row::with_children(vec![
                label(visible, style::command_text_for(theme_mode)),
                cursor.into(),
            ])
            .spacing(2.0)
            .align_y(Alignment::Center)
            .into(),
        );
        Row::with_children(parts).align_y(Alignment::Center).into()
    }

    /// The width the transcript wraps into: the screen minus the safe inset.
    fn text_width(&self) -> f32 {
        let pad = style::PAGE_PADDING.min(self.size.width * 0.05);
        (self.size.width - pad * 2.0).max(180.0)
    }

    /// The transcript: every history line, then the active input line.
    ///
    /// Wrapping is this UI's job — `shell::wrap_line` takes the measurement as a parameter — so
    /// one raw output entry becomes as many 24 px cells as it needs.
    fn transcript(&self) -> Vec<El> {
        let theme_mode = self.theme_mode();
        let max_text_w = self.text_width();
        let mut lines = Vec::with_capacity(self.model.history.len() + 1);

        for entry in &self.model.history {
            match entry {
                HistoryEntry::Prompt { cwd, cmd } => {
                    let mut parts = Self::prompt_prefix(cwd.clone(), theme_mode);
                    parts.push(label(cmd.clone(), style::command_text_for(theme_mode)));
                    lines.push(Self::cell(
                        Row::with_children(parts).align_y(Alignment::Center).into(),
                    ));
                }
                HistoryEntry::Output(text) => {
                    let text_color = style::text_fg_for(theme_mode);
                    for line in shell::wrap_line(text, max_text_w, &style::measure) {
                        lines.push(Self::cell(label(line, text_color)));
                    }
                }
            }
        }

        lines.push(Self::cell(self.input_tail()));
        lines
    }

    /// The transcript viewport, scrolled to the bottom.
    fn viewport(&self) -> El {
        let pad = style::PAGE_PADDING.min(self.size.width * 0.05);
        let content = Column::with_children(self.transcript()).width(Length::Fill);

        let scroller = Scrollable::new(content)
            .direction(Direction::Vertical(Scrollbar::hidden()))
            .anchor_bottom()
            .width(Length::Fill)
            .height(Length::Fill);

        container(scroller)
            .padding(Padding {
                top: 20.0,
                right: pad,
                bottom: 4.0,
                left: pad,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The on-screen keyboard, anchored to the bottom.
    ///
    /// The band is the shared keyboard's: the same four rows, keys and palette the settings app
    /// draws for a Wi-Fi password. What is the terminal's own here is the string the keys write
    /// into (`Message::Key` → the model), the mode it remembers, and the *height* — a keyboard
    /// fills the box it is given, so how tall that box is, is stated here on the layout and not
    /// passed into the widget.
    fn keyboard(&self) -> El {
        container(pomelo_widgets::touch_keyboard::band_with_theme(
            self.model.keyboard_mode,
            Message::Key,
            self.theme_mode(),
        ))
        .height(Length::Fixed(self.keyboard_height()))
        .into()
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal {
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
                    ..Palette::LIGHT
                },
            )
        } else {
            Theme::custom(
                "Pomelo",
                Palette {
                    background: style::background_for(theme_mode),
                    ..Palette::DARK
                },
            )
        }
    }

    /// Reacts to one message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Key(action) => self.model.handle_key_action(action),
            Message::Resized(size) => self.size = size,
        }
    }

    /// Describes the interface for the current state.
    ///
    /// The parameter is the elided lifetime, not [`El`]: everything below is owned, so the element
    /// could be `'static`, but iced asks for `for<'a> fn(&'a State) -> Element<'a, _>` and a
    /// `'static` return does not satisfy that bound. It coerces here instead.
    pub fn view(&self) -> Element<'_, Message> {
        let bg = style::background_for(self.theme_mode());
        container(column![self.viewport(), self.keyboard()])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            })
            .into()
    }
}

/// One coloured label of the prompt or the transcript.
fn label(content: impl Into<String>, color: Color) -> El {
    text(content.into())
        .size(style::FONT_SIZE)
        .color(color)
        .into()
}

/// The visible tail of a line: a terminal shows the end of what is being typed.
fn visible_tail(text: &str, max_width: f32) -> String {
    if style::measure(text) <= max_width {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        let candidate: String = chars[start..].iter().collect();
        if style::measure(&candidate) <= max_width {
            return candidate;
        }
    }
    String::new()
}
