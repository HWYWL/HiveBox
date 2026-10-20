//! The task switcher: what is running, and the two ways to stop it.
//!
//! # What it is a layer over
//!
//! The sheet comes down over whatever is on screen — the desktop, or an app still running behind it
//! — and that is the whole of what it means: the apps it lists are *still there*. So it is a layer
//! and not another screen, and a layer takes the press: the wash under the cards is one large
//! button, and a tap on it puts the sheet away. The back key and the swipe up from the foot of the
//! panel do the same thing, because both of them already mean "not this".
//!
//! # Two ways to stop an app, and why both
//!
//! A phone has both, and they are not the same gesture: the cross on a card says "not this one" and
//! needs a finger to name one, and the control in the head says "not any of them" and needs the
//! finger to mean it once. Both end in [`crate::Launcher::kill_app`] — the difference is how many
//! intentions a finger has to state, not what happens to the memory afterwards.
//!
//! # The three words are matched, and not tabled
//!
//! Every other word on this sheet is an app's name, and the catalogue already has those in both
//! languages ([`Entry::localized_name`](pomelo_widgets::AppMeta::localized_name)). Three strings do
//! not earn the machinery the settings app built for ninety keys — see its `i18n` for what that
//! looks like when the table is worth having — and a `match` over two languages is exhaustive
//! either way, which is the part that matters.

use iced::widget::{button, container, stack, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow};
use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::{Language, ThemeMode};

use crate::style;
use crate::Message;

/// One running app as the sheet draws it: what it is called, and the two things that identify it.
///
/// A card is built by the launcher rather than looked up here, because the grid's later tiles are
/// installed packages and this module is about the sheet, not about where a name came from.
pub struct Card {
    pub name: String,
    pub accent: (u8, u8, u8),
    pub glyph: Icon,
}

/// The sheet as an element, over `apps` — the running ones, the most recent first.
///
/// `card` answers what an index names, and `None` for an index nothing names: an application that was
/// uninstalled while it was running is a card that has gone, not one wearing another's name.
pub fn view<'a>(
    apps: &[usize],
    language: Language,
    theme_mode: ThemeMode,
    card: &dyn Fn(usize) -> Option<Card>,
) -> Element<'a, Message> {
    let ink = match theme_mode {
        ThemeMode::Dark => Color::WHITE,
        ThemeMode::Light => Color::BLACK,
    };
    let muted = match theme_mode {
        ThemeMode::Dark => Color::from_rgb8(150, 156, 168),
        ThemeMode::Light => Color::from_rgb8(110, 116, 128),
    };
    let slab = match theme_mode {
        ThemeMode::Dark => style::RECENTS_CARD_DARK,
        ThemeMode::Light => style::RECENTS_CARD_LIGHT,
    };
    let slab = Color::from_rgba8(slab.0, slab.1, slab.2, f32::from(slab.3) / 255.0);

    // The wash: a button the size of the screen, so that a tap landing anywhere the cards are not —
    // including the gaps between them — puts the sheet away. It is the layer *under* the sheet,
    // which is what leaves the cards and their crosses their own presses.
    let wash = button(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(0)
        .style(move |_theme, _status| button::Style {
            background: Some(
                Color::from_rgba8(
                    style::RECENTS_BACKDROP.0,
                    style::RECENTS_BACKDROP.1,
                    style::RECENTS_BACKDROP.2,
                    f32::from(style::RECENTS_BACKDROP.3) / 255.0,
                )
                .into(),
            ),
            text_color: Color::TRANSPARENT,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        })
        .on_press(Message::RecentsClose);

    let mut head = Row::with_children(vec![
        text(Icon::HISTORY.glyph())
            .font(icons::font())
            .size(style::RECENTS_KILL_GLYPH)
            .color(muted)
            .into(),
        Space::new().width(Length::Fixed(style::GLYPH_GAP)).into(),
        text(title(language))
            .size(style::RECENTS_TITLE_FONT)
            .color(ink)
            .into(),
        Space::new().width(Length::Fill).into(),
    ])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    // Offered only when there is something to clear: a "clear all" over an empty list is a control
    // that does nothing and looks like it should.
    if !apps.is_empty() {
        head = head.push(clear_all_control(language, ink));
    }

    let mut list = Column::new()
        .width(Length::Fill)
        .spacing(style::RECENTS_CARD_GAP);

    if apps.is_empty() {
        list = list.push(notice_card(nothing_running(language), muted, slab));
    } else {
        for &index in apps {
            list = list.push(running_card(index, ink, muted, slab, card));
        }
    }

    let sheet = container(
        Column::with_children(vec![
            head.into(),
            Space::new()
                .height(Length::Fixed(style::RECENTS_HEAD_GAP))
                .into(),
            list.into(),
        ])
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(style::RECENTS_MARGIN);

    // The sheet is centred in the screen rather than hung from the top: it is as tall as what is
    // running, and a sheet with three cards pinned to the top of a 480 px panel would read as a page
    // that had failed to fill.
    stack![
        wash,
        container(sheet).center_x(Length::Fill).center_y(Length::Fill)
    ]
    .into()
}

/// One running app: which one it is, and the cross that stops it.
fn running_card<'a>(
    index: usize,
    ink: Color,
    muted: Color,
    slab: Color,
    card: &dyn Fn(usize) -> Option<Card>,
) -> Element<'a, Message> {
    // An index nothing names: an application uninstalled while it was in the background. An empty
    // card is a better answer than one wearing another app's name — see `Launcher::view`.
    let Some(card) = card(index) else {
        return Space::new().into();
    };

    let accent = Color::from_rgb8(card.accent.0, card.accent.1, card.accent.2);
    let glyph = card.glyph;

    let icon = container(
        container(
            text(glyph.glyph())
                .font(icons::font())
                .size(style::RECENTS_ICON_GLYPH)
                .color(Color::WHITE),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .width(Length::Fixed(style::RECENTS_ICON))
    .height(Length::Fixed(style::RECENTS_ICON))
    .style(move |_theme| container::Style {
        background: Some(accent.into()),
        border: Border {
            radius: style::RECENTS_ICON_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    });

    let cross = button(
        container(
            text(Icon::CLOSE.glyph())
                .font(icons::font())
                .size(style::RECENTS_KILL_GLYPH)
                .color(muted),
        )
        .center_x(Length::Fixed(style::RECENTS_KILL))
        .center_y(Length::Fixed(style::RECENTS_KILL)),
    )
    .padding(0)
    .style(move |_theme, status| control(status, ink))
    .on_press(Message::RecentsKill(index));

    let line = Row::with_children(vec![
        icon.into(),
        Space::new().width(Length::Fixed(style::GLYPH_GAP)).into(),
        text(card.name)
            .size(style::RECENTS_NAME_FONT)
            .color(ink)
            .into(),
        Space::new().width(Length::Fill).into(),
        cross.into(),
    ])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    container(line)
        .width(Length::Fill)
        .height(Length::Fixed(style::RECENTS_CARD_H))
        .padding(style::RECENTS_PADDING)
        .style(move |_theme| slab_of(slab))
        .into()
}

/// The one control that stops every app at once.
fn clear_all_control<'a>(language: Language, ink: Color) -> Element<'a, Message> {
    let line = Row::with_children(vec![
        text(Icon::DELETE_SWEEP.glyph())
            .font(icons::font())
            .size(style::RECENTS_KILL_GLYPH)
            .color(ink)
            .into(),
        Space::new().width(Length::Fixed(style::GLYPH_GAP)).into(),
        text(clear_all(language))
            .size(style::RECENTS_NAME_FONT)
            .color(ink)
            .into(),
    ])
    .align_y(Alignment::Center);

    button(line)
        .padding([style::RECENTS_PADDING, style::RECENTS_PADDING])
        .style(move |_theme, status| control(status, ink))
        .on_press(Message::RecentsClearAll)
        .into()
}

/// What the sheet says when there is nothing in the background.
fn notice_card<'a>(words: &'static str, muted: Color, slab: Color) -> Element<'a, Message> {
    container(
        container(text(words).size(style::RECENTS_NAME_FONT).color(muted))
            .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(style::RECENTS_CARD_H))
    .padding(style::RECENTS_PADDING)
    .style(move |_theme| slab_of(slab))
    .into()
}

/// The card surface: a slab of whatever is under the sheet.
fn slab_of(slab: Color) -> container::Style {
    container::Style {
        background: Some(slab.into()),
        border: Border {
            radius: style::RECENTS_CARD_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The look of the sheet's two controls: a glyph, a word, and a wash under the finger.
///
/// One style for both, because they are one kind of thing: each says "stop" and each is answered by
/// the same press. The wash is the ink at a fifth — a film rather than a fill, so that what is under
/// it is still what is being read.
fn control(status: button::Status, ink: Color) -> button::Style {
    let background = match status {
        button::Status::Pressed => Some(Color { a: 0.18, ..ink }.into()),
        _ => None,
    };

    button::Style {
        background,
        text_color: ink,
        border: Border {
            radius: (style::RECENTS_KILL / 2.0).into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// The head of the sheet.
fn title(language: Language) -> &'static str {
    match language {
        Language::Chinese => "后台应用",
        Language::English => "Background apps",
    }
}

/// The control that stops them all.
fn clear_all(language: Language) -> &'static str {
    match language {
        Language::Chinese => "全部清除",
        Language::English => "Clear all",
    }
}

/// What an empty sheet says.
fn nothing_running(language: Language) -> &'static str {
    match language {
        Language::Chinese => "没有后台应用",
        Language::English => "Nothing in the background",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{CATALOGUE, FILES, MUSIC, TERMINAL};

    /// Every word exists in both languages, and the two are not the same word.
    ///
    /// A sheet that spoke one language in either setting would be the same defect as a missing
    /// translation, and the `match` cannot catch it: both arms compile.
    #[test]
    fn the_sheets_words_are_in_both_languages() {
        for word in [title, clear_all, nothing_running] {
            assert!(!word(Language::Chinese).is_empty());
            assert!(!word(Language::English).is_empty());
            assert_ne!(
                word(Language::Chinese),
                word(Language::English),
                "{:?} reads the same twice",
                word(Language::Chinese)
            );
        }
    }

    /// The sheet builds with nothing running, with one app, and with more than fits its width.
    ///
    /// A widget tree has no pixels, so what a test can hold is that nothing in here panics on the
    /// shapes that are easy to get wrong: an empty list (which draws the notice instead of a card),
    /// and an index the catalogue does not have (which draws nothing rather than the wrong name).
    #[test]
    fn the_sheet_builds_for_every_shape_of_list() {
        let card = |index: usize| {
            CATALOGUE.get(index).map(|entry| Card {
                name: entry.name.to_string(),
                accent: entry.accent,
                glyph: entry.icon.as_glyph().unwrap_or(Icon::APPS),
            })
        };

        for apps in [vec![], vec![MUSIC], vec![MUSIC, TERMINAL, FILES]] {
            for theme_mode in [ThemeMode::Dark, ThemeMode::Light] {
                let _ = view(&apps, Language::Chinese, theme_mode, &card);
                let _ = view(&apps, Language::English, theme_mode, &card);
            }
        }

        let _ = view(&[99], Language::English, ThemeMode::Dark, &card);
    }
}
