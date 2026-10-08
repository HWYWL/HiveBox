//! The confirmation dialog: a question, and the two answers to it.
//!
//! The app's second modal. The first is the Wi-Fi page's password sheet, and it lives with that page
//! because only that page can open it; this one is the app's, because the row that asks for it is on
//! the main list and the question it asks is about the machine rather than about the list.
//!
//! It is a layer *over* whatever page is up — see [`crate::Settings::view`], which stacks it — with
//! the same dimming the sheet uses, because a modal the page behind it can still be pressed through
//! is not a modal.

use iced::widget::{container, opaque, text, Column, Row, Space};
use iced::Length;

use pomelo_material_symbols::Icon;
use pomelo_widgets::SystemPreferences;

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::common::{capsule_button, card_surface, confirm_style, dismiss_style, UI};
use crate::style;
use crate::Message;

/// The restart question: what is about to happen, and the two ways to answer it.
///
/// A cross and a tick, the same pair the password sheet asks with. The shape is this app's for "no"
/// and "yes" wherever a question is put, and neither glyph needs translating — which matters more
/// here than there: an answer that says "restart" in a language the reader has just switched away
/// from is a worse answer than a tick.
pub(crate) fn restart_dialog<'a>(preferences: SystemPreferences) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;

    let answers = Row::with_children(vec![
        capsule_button(
            Icon::CLOSE,
            Some(Message::RestartCancel),
            dismiss_style,
            theme,
        ),
        Space::new().width(Length::Fill).into(),
        capsule_button(
            Icon::CHECK,
            Some(Message::RestartConfirm),
            confirm_style,
            theme,
        ),
    ])
    .width(Length::Fill);

    let card = card_surface(
        container(
            Column::with_children(vec![
                answers.into(),
                Space::new().height(Length::Fixed(style::DIALOG_GAP)).into(),
                text(language.text(Key::Restart))
                    .size(style::NAV_FONT)
                    .color(style::label_for(theme))
                    .into(),
                Space::new().height(Length::Fixed(style::DIALOG_GAP)).into(),
                text(language.text(Key::RestartQuestion))
                    .size(style::DETAIL_FONT)
                    .color(style::muted_for(theme))
                    .into(),
            ])
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .padding(style::DIALOG_PADDING),
        theme,
    );

    // Centred, where the password sheet stands on the floor of the screen: a keyboard is a thing at
    // the foot of a panel, and a card is a thing in the middle of one.
    //
    // `opaque` is what takes the finger, and it is the same answer the sheet gives: it captures any
    // press inside its bounds, and its bounds are the whole panel, so the list behind — a screen of
    // buttons — never sees one.
    opaque(
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(style::DIALOG_MARGIN)
            .style(|_theme| container::Style {
                background: Some(style::backdrop().into()),
                ..container::Style::default()
            }),
    )
}
