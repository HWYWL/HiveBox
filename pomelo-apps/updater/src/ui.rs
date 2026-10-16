//! Everything this page draws: the heading, the two rows, and the line that says what happened.
//!
//! Two rows of the same shape, because the two things this page updates *are* the same question asked
//! of two files: what is here, what is there, and which of the two buttons applies. A page that drew the
//! firmware differently from the glyph table would be a page suggesting they are differently important,
//! and a person reading it would have to work out which — which is the work this page exists to save.
//!
//! # The version is the point of the row
//!
//! Each row is read left to right as a sentence: the glyph says *what*, the name says what it is called,
//! the line under it says what is installed, and the right-hand line says what the repository has
//! instead. Delete any of the four and the row stops answering the question that opened the page.

use iced::widget::{button, container, row, text, Column, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Theme};

use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::style::{self, Sizes};
use crate::{Kind, Message, Status, Updater, Words};

/// The page.
pub(crate) fn page(app: &Updater) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = Sizes::new();
    let words = Words::of(app.preferences.language);

    let rows: Vec<Element<'_, Message>> = [Kind::Firmware, Kind::Font]
        .into_iter()
        .map(|kind| row_card(app, kind, &words, sizes, mode))
        .collect();

    let body = Column::with_children(vec![
        header(app, &words, sizes, mode),
        Column::with_children(rows).spacing(style::GAP).into(),
        Space::new().height(Length::Fill).into(),
        notice(app, &words, sizes, mode),
    ])
    .spacing(style::GAP)
    .padding(style::MARGIN)
    .width(Length::Fill)
    .height(Length::Fill);

    // The panel is 480 px square and a desktop window is not, so the page keeps its width and the space
    // either side of it is the surface's own — the same argument the forecast page makes.
    container(
        container(body)
            .max_width(style::PANEL)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(style::surface(mode).into()),
        ..container::Style::default()
    })
    .into()
}

/// The head of the page: what this is, and the button that asks the repository again.
///
/// The subtitle names the two things the page is about rather than the state of the request, because
/// the state is what the rows and the line at the bottom are for: a heading that said "checking" would
/// be the second place on the page saying it.
fn header<'a>(
    app: &Updater,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let heading = Column::with_children(vec![
        text(words.title).size(sizes.title).color(style::ink(theme)).into(),
        text(words.subtitle)
            .size(sizes.small)
            .color(style::muted(theme))
            .into(),
    ])
    .spacing(2.0);

    let check = button(
        row![
            text(Icon::REFRESH.glyph())
                .font(icons::font())
                .size(sizes.small)
                .color(style::ink(theme)),
            text(words.check)
                .size(sizes.small)
                .color(style::ink(theme)),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    )
    // Nothing to press while a request is on its way: the answer that is coming is the one this button
    // would ask for, and it answers for both rows at once — there is one manifest and not two.
    .on_press_maybe(app.can_check().then_some(Message::Check))
    .padding([6.0, 10.0])
    .style(move |_theme, status| {
        let fill = if is_pressed(status) {
            style::accent()
        } else {
            style::card(theme)
        };

        button_style(fill, style::ink(theme), style::RADIUS / 2.0)
    });

    row![
        text(Icon::SYSTEM_UPDATE.glyph())
            .font(icons::font())
            .size(sizes.glyph)
            .color(style::accent()),
        heading,
        Space::new().width(Length::Fill),
        check,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center)
    .into()
}

/// One of the two rows: what is installed, what the repository has, and the one button that applies.
///
/// The button is the row's *conclusion* and not its decoration: a row that has something to offer shows
/// the download, and a row that has not — or does not yet know — shows the check. There is never a row
/// with both, because "look again" and "take this" are different answers to the same question and the
/// row has already been asked it.
fn row_card<'a>(
    app: &'a Updater,
    kind: Kind,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let installed = words.installed_line(app.installed_version(kind), kind);
    let offer = app.offer(kind);

    // What the repository has, in the row's own colour: the offer is the page's accent, "up to date" is
    // green only once a check has said so, and the two states where nothing is known say nothing at all
    // rather than claiming a version is current.
    let (standing, standing_ink) = match (&offer, &app.status) {
        (Some(release), _) => (words.offer_line(release), style::accent()),
        (None, Status::Answered) => (words.current.to_string(), style::good()),
        (None, Status::Checking) => (words.checking.to_string(), style::muted(theme)),
        (None, _) => (String::new(), style::muted(theme)),
    };

    let detail = match &offer {
        Some(release) => release.file.as_str(),
        None => "",
    };

    // Spelled out because the two arms are two different widgets and `.into()` has nothing to infer
    // from until they are both `Element`s — one arm's `Button` and the other's would otherwise be the
    // type the match had to guess.
    let action: Element<'a, Message> = match &offer {
        Some(_) => button(
            row![
                text(Icon::DOWNLOAD.glyph())
                    .font(icons::font())
                    .size(sizes.small)
                    .color(style::ink(theme)),
                text(words.install).size(sizes.small).color(style::ink(theme)),
            ]
            .spacing(4.0)
            .align_y(Alignment::Center),
        )
        .on_press(Message::Install(kind))
        .padding([6.0, 10.0])
        .style(move |_theme: &Theme, status: button::Status| {
            let fill = if is_pressed(status) {
                style::accent()
            } else {
                style::card(theme)
            };

            button_style(fill, style::ink(theme), style::RADIUS / 2.0)
        })
        .into(),
        None => button(
            row![
                text(Icon::REFRESH.glyph())
                    .font(icons::font())
                    .size(sizes.small)
                    .color(style::muted(theme)),
                text(words.check).size(sizes.small).color(style::muted(theme)),
            ]
            .spacing(4.0)
            .align_y(Alignment::Center),
        )
        .on_press_maybe(app.can_check().then_some(Message::Check))
        .padding([6.0, 10.0])
        .style(move |_theme: &Theme, status: button::Status| {
            let fill = if is_pressed(status) {
                style::accent()
            } else {
                style::card(theme)
            };

            button_style(fill, style::muted(theme), style::RADIUS / 2.0)
        })
        .into(),
    };

    let body = row![
        text(kind_icon(kind).glyph())
            .font(icons::font())
            .size(sizes.glyph)
            .color(style::accent()),
        Column::with_children(vec![
            text(words.name(kind)).size(sizes.text).color(style::ink(theme)).into(),
            text(installed).size(sizes.small).color(style::muted(theme)).into(),
        ])
        .spacing(2.0)
        .width(Length::Fixed(style::NAME)),
        Space::new().width(Length::Fill),
        Column::with_children(vec![
            text(standing).size(sizes.text).color(standing_ink).into(),
            text(detail).size(sizes.tiny).color(style::muted(theme)).into(),
        ])
        .spacing(2.0)
        .align_x(Alignment::End),
        action,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    card(body.into(), theme)
}

/// The line under the rows: what happened to the last check, in one sentence.
///
/// One line and not a card, because it is not one of the page's two subjects: it is the *state of the
/// question* that was asked about them, and a card would give it the same weight as a version number.
fn notice<'a>(
    app: &Updater,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let (line, ink) = match &app.status {
        Status::Idle => (words.idle.to_string(), style::muted(theme)),
        Status::Checking => (words.checking.to_string(), style::muted(theme)),
        Status::Answered => (String::new(), style::muted(theme)),
        Status::Failed(fault) => (words.fault_line(*fault).to_string(), style::bad()),
        Status::Unreadable => (words.unreadable.to_string(), style::bad()),
        // The two rows need different things of a person — a new image needs a reboot's worth of flash
        // and a new table needs the card — and a page that said the same sentence for both would send
        // them to the wrong place.
        Status::Unsupported(Kind::Firmware) => (words.unsupported.to_string(), style::bad()),
        Status::Unsupported(Kind::Font) => (words.no_card.to_string(), style::bad()),
    };

    text(line)
        .size(sizes.small)
        .color(ink)
        .width(Length::Fill)
        .into()
}

/// The glyph at the left of a row.
fn kind_icon(kind: Kind) -> Icon {
    match kind {
        Kind::Firmware => Icon::MEMORY,
        Kind::Font => Icon::FONT_DOWNLOAD,
    }
}

/// A card: a slab, a radius and a padding, and whatever is put in it.
fn card<'a>(body: Element<'a, Message>, theme: ThemeMode) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding(style::GAP)
        .style(move |_theme| container::Style {
            background: Some(style::card(theme).into()),
            border: Border {
                radius: style::RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A button's capsule: its fill, the ink its contents default to, and its corner.
fn button_style(fill: Color, ink: Color, radius: f32) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Whether a finger is on the button.
///
/// `Pressed` and only `Pressed`: a touchscreen has no hover, and this platform leaves the pointer where
/// the finger lifted, so a button that has been touched stays hovered until something else moves the
/// pointer — which is a button that stays lit after it is let go.
fn is_pressed(status: button::Status) -> bool {
    matches!(status, button::Status::Pressed)
}
