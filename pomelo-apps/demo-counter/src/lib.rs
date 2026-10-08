//! The counter, built from iced widgets — **a standard iced program**.
//!
//! The smallest app here, and the one that says the least about the platform: one number, one
//! button, and a card that splits its height 2:3:2 between the title, the number and the button --
//! which is what makes it a layout test rather than a logic test. The number is 96 pt and the panel
//! is 480 px, so it only stays on one line because the type does not scale with the panel.
//!
//! Nothing here names a platform, a panel or a renderer. `main.rs` runs it, `iced::application` asks
//! for everything else, and which layer answers depends on who builds this crate: iced's own
//! `iced_winit` and a window in this project, or the board's panel in `firmware/`.

pub mod style;

use iced::widget::{button, column, container, text};
use iced::{theme::Palette, Alignment, Border, Color, Element, Length, Shadow, Theme};
use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};

pub use pomelo_widgets::preferences::Language;
pub use style::SCREEN;


/// What the counter reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// The button.
    Tap,
}

/// The counter.
pub struct Counter {
    count: u32,
    preferences: SystemPreferences,
}

impl Counter {
    pub fn new() -> Self {
        Self {
            count: 0,
            preferences: SystemPreferences::default(),
        }
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
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

    /// The current language.
    pub fn language(&self) -> Language {
        self.preferences.language
    }

    /// Sets the language.
    pub fn set_language(&mut self, language: Language) {
        self.preferences.language = language;
    }

    /// What the display shows. Read by the host and by the tests.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The number, two digits wide so that 9 and 10 do not shift the layout under the finger.
    fn number(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        container(
            text(format!("{:02}", self.count))
                .size(style::NUMBER_FONT)
                .color(style::number_for(theme_mode)),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// The button, which is what a finger is aimed at.
    fn button(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let button_text = match self.language() {
            Language::Chinese => "点击 +1",
            Language::English => "TAP +1",
        };

        button(
            container(text(button_text).size(style::LABEL_FONT).color(Color::WHITE))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .padding(style::BUTTON_PADDING)
        .width(Length::Shrink)
        .height(Length::Shrink)
        .style(move |_theme, status| {
            let fill = match status {
                button::Status::Pressed => style::button_pressed_for(theme_mode),
                _ => style::button_for(theme_mode),
            };

            button::Style {
                background: Some(fill.into()),
                text_color: Color::WHITE,
                border: Border {
                    radius: style::BUTTON_RADIUS.into(),
                    ..Border::default()
                },
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .on_press(Message::Tap)
        .into()
    }

    /// The card the counter lives in.
    ///
    /// The 2:3:2 split the original spells `Expanded(flex)` is a `FillPortion` here, and
    /// the card itself is a `Fill`: it takes the height the footer leaves it, so the absolute
    /// measurements are the type sizes and the button's padding, not the card's height.
    fn card(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let title_text = match self.language() {
            Language::Chinese => "触摸计数器",
            Language::English => "TOUCH COUNTER",
        };

        let body = column![
            band(
                container(
                    text(title_text)
                        .size(style::TITLE_FONT)
                        .color(style::title_for(theme_mode))
                )
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into(),
                2
            ),
            band(self.number(), 3),
            band(
                container(self.button())
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into(),
                2
            ),
        ]
        .align_x(Alignment::Center);

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
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
}

/// One band of the card's 2:3:2 split.
///
/// A function and not a closure: a closure taking `Element<'_, ..>` and returning one ties the two
/// lifetimes together, and `Container` is invariant over its lifetime.
fn band<'a>(child: Element<'a, Message>, share: u16) -> Element<'a, Message> {
    container(child).center_y(Length::FillPortion(share)).into()
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

impl Counter {
    /// The counter's theme.
    ///
    /// A solid background, not a wallpaper primitive: the compositor paints the background over the
    /// damage rectangle only, while a full-screen primitive costs the whole screen every frame. The
    /// launcher's theme carries the measurement.
    pub fn theme(&self) -> Theme {
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

    /// Reacts to a tap.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tap => self.count += 1,
        }
    }

    /// The whole screen.
    pub fn view(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let footer_text = match self.language() {
            Language::Chinese => "由 Iced 框架驱动",
            Language::English => "Powered by Iced",
        };

        container(
            column![
                self.card(),
                container(
                    text(footer_text)
                        .size(style::FOOTER_FONT)
                        .color(style::footer_for(theme_mode))
                )
                .width(Length::Fill)
                .center_x(Length::Fill),
            ]
            .spacing(style::CARD_GAP),
        )
        .padding(style::PAGE_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
