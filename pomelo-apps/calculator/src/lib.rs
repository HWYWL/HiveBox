//! The calculator — **a standard iced program**.
//!
//! Not a program that owns a loop: [`Calculator`] is a state with an `update` and a `view`, and
//! `main.rs` — three lines — is what runs it. Nothing in this crate names a platform, a panel or a
//! renderer; `iced::run` asks the platform layer for all three, and *which* layer that is depends on
//! who builds the crate. On a desktop it is iced's own `iced_winit` and a window; on this board it
//! is the host in `vendor/iced-pomelo-winit` (the facade's `iced_winit` dependency is patched to it)
//! and the panel, driven by `rust_main`. The app cannot tell the difference, and that is the point.
//!
//! The arithmetic is not in the widgets. It is [`model`], which the original app used too, because
//! two implementations of a calculator that disagree with each other is a bug waiting for a user.

mod format;
mod keys;
mod model;
pub mod style;

use iced::widget::{button, column, container, text, Column, Row, Space};
use iced::{theme::Palette, Alignment, Border, Element, Length, Shadow, Theme};

use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};

pub use format::{add_commas, eval_op, format_raw_number};
pub use keys::{Entry, Key, Kind, LAYOUT};
pub use model::CalcModel;
pub use style::SCREEN;


/// What the calculator reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A key was pressed.
    Key(Key),
}

/// The calculator.
pub struct Calculator {
    model: CalcModel,
    preferences: SystemPreferences,
}

impl Calculator {
    pub fn new() -> Self {
        Self {
            model: CalcModel::new(),
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

    /// What the display shows. Read by the host and by the tests.
    pub fn display(&self) -> String {
        self.model.primary_display()
    }

    /// The line above it: the expression so far.
    pub fn expression(&self) -> String {
        self.model.secondary_display().to_string()
    }

    /// One key of the keypad.
    ///
    /// The key sits in a *cell* that carries half of the column gap on each side, so the row needs
    /// no spacing of its own — which is what keeps the `0` row lined up with the rows above it: it
    /// has one gap fewer, and with the gaps counted as spacers it would divide a different space
    /// (96 px keys under 92 px ones, 8 px out).
    fn key(&self, entry: &'static Entry) -> Element<'_, Message> {
        let kind = entry.kind;
        let theme_mode = self.theme_mode();

        let key = button(
            container(text(entry.label).size(style::KEY_FONT))
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(0)
        .style(move |_theme, status| key_style(kind, status, theme_mode))
        .on_press(Message::Key(entry.key));

        container(key)
            .width(Length::FillPortion(entry.span))
            .height(Length::Fill)
            .padding([0.0, style::COLUMN_GAP / 2.0])
            .into()
    }

    /// The display: the value, and the expression it came from.
    ///
    /// The card divides its vertical space between the secondary expression and the primary
    /// value in a 2:3 ratio, with both lines vertically centered in their respective zones.
    fn display_card(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let secondary = container(
            text(self.model.secondary_display())
                .size(style::SECONDARY_FONT)
                .color(style::text_secondary_for(theme_mode)),
        )
        .width(Length::Fill)
        .height(Length::FillPortion(2))
        .align_x(Alignment::End)
        .align_y(Alignment::Center);

        let primary = container(
            text(self.model.primary_display())
                .size(style::PRIMARY_FONT)
                .color(style::text_primary_for(theme_mode)),
        )
        .width(Length::Fill)
        .height(Length::FillPortion(3))
        .align_x(Alignment::End)
        .align_y(Alignment::Center);

        container(
            column![secondary, primary]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .padding([style::CARD_PAD_V, style::CARD_PAD_H])
        .width(Length::Fill)
        .height(Length::FillPortion(style::CARD_SHARE))
        .style(move |_theme| container::Style {
            background: Some(style::card_for(theme_mode).into()),
            border: Border {
                radius: style::CARD_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
    }

    /// The keypad.
    ///
    /// Five rows and the four gaps between them are all *shares* of the keypad's height, so the
    /// keypad fills whatever the page hands it — a taller window, taller keys — and reproduces the
    /// design exactly on the panel it was drawn for.
    fn keypad(&self) -> Element<'_, Message> {
        let mut children: Vec<Element<'_, Message>> = Vec::with_capacity(2 * LAYOUT.len() - 1);

        for (index, row) in LAYOUT.iter().enumerate() {
            if index > 0 {
                children.push(gap(style::KEY_GAP_SHARE));
            }

            children.push(
                Row::with_children(row.iter().map(|entry| self.key(entry)))
                    .width(Length::Fill)
                    .height(Length::FillPortion(style::KEY_SHARE))
                    .into(),
            );
        }

        Column::with_children(children)
            .width(Length::Fill)
            .height(Length::FillPortion(style::KEYPAD_SHARE))
            .into()
    }
}

impl Default for Calculator {
    fn default() -> Self {
        Self::new()
    }
}

impl Calculator {
    /// The calculator's theme.
    ///
    /// A solid background, not a wallpaper primitive -- see the launcher's theme for the
    /// measurement that made this the rule: the compositor paints the background over the damage
    /// rectangle only, while a full-screen primitive costs the whole screen every frame.
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

    /// Reacts to a key.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Key(key) => key.apply(&mut self.model),
        }
    }

    /// The whole screen.
    ///
    /// The page is a column of *shares* of its height — card, gap, keypad — framed by 20 px
    /// padding on all sides. Nothing here has a fixed height, so a taller window gives taller keys
    /// rather than empty page.
    pub fn view(&self) -> Element<'_, Message> {
        let theme_mode = self.theme_mode();
        let bg = style::background_for(theme_mode);

        container(column![
            self.display_card(),
            gap(style::CARD_GAP_SHARE),
            self.keypad(),
        ])
        .padding(style::PAGE_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(bg.into()),
            ..container::Style::default()
        })
        .into()
    }
}

/// An empty box that takes a *share* of the space it is given: the gaps of this page are shares of
/// its height, not fixed distances, so they grow with the window the way the boxes around them do.
fn gap(share: u16) -> Element<'static, Message> {
    Space::new().height(Length::FillPortion(share)).into()
}

/// A key: its role's fill, the lighter one while the finger is on it.
fn key_style(kind: Kind, status: button::Status, theme_mode: ThemeMode) -> button::Style {
    let palette = style::palette_for(kind, theme_mode);

    button::Style {
        background: Some(
            match status {
                button::Status::Pressed => palette.pressed,
                _ => palette.fill,
            }
            .into(),
        ),
        text_color: palette.text,
        border: Border {
            radius: style::KEY_RADIUS.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}
