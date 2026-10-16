//! Everything this page draws: the heading, the two rows, the line that says what happened to the
//! check, and the prompt that says what happened to a press.
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
//!
//! # Two kinds of news, drawn two ways
//!
//! The check is page-wide — one manifest answers for both rows — and its state is the quiet line at the
//! bottom, where it is always there and never in the way. A *press* is about one row and is drawn as a
//! prompt over the page, because it is the one thing here that interrupts: a person pressed a button and
//! is owed an answer, and the answer is a sentence they should have to close rather than a line that
//! arrived under two rows they were not looking at.
//!
//! The prompt is [`crate::Prompt`]'s description and not this file's invention — which sentence, whether
//! there is a bar, and what each of its buttons says are decided where a test can hold them up. This
//! file decides where the ink comes from.
//!
//! # What is not in the row
//!
//! The offered file's *name* is not drawn in the row, though it was once. It is long — longer than the
//! version it sits under, and long enough to push the row's button into a column of its own glyphs — and
//! it answers a question the row has already answered twice, since the version is on the line above it.
//! It lives in the prompt instead, where it is the thing about to be fetched rather than a footnote to
//! something else.

use iced::widget::{button, container, opaque, progress_bar, row, stack, text, Column, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Theme};

use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::style::{self, Sizes};
use crate::{Kind, Message, Prompt, Status, Updater, Words};

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
    let page = container(
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
    });

    // The prompt goes *over* the page and never instead of it: the rows are the sentence's context, and
    // the row it is about is still the thing being answered. `opaque` is what makes it modal — without
    // it a press would fall through onto the rows underneath, and a person could ask for a second
    // download while the first one's bar was still moving.
    let overlay: Element<'_, Message> = match app.prompt(&words) {
        Some(prompt) => stack([opaque(page), prompt_overlay(&app.status, &prompt, sizes, mode)]).into(),
        None => page.into(),
    };

    overlay
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

    // Spelled out because the two arms are two different widgets and `.into()` has nothing to infer
    // from until they are both `Element`s — one arm's `Button` and the other's would otherwise be the
    // type the match had to guess.
    //
    // Both arms are [`style::ACTION`] wide, and that is the fix for the button that used to wrap: the
    // width is a *reservation*, so the row's longest neighbour decides how much room is left for the
    // version and never how much is left for the button. The content is centred in it by hand, because
    // a padded row lays its children out from the left and a button wider than its label would
    // otherwise read as a label that had drifted.
    let action: Element<'a, Message> = match &offer {
        Some(_) => button(
            container(
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
            .width(Length::Fill)
            .center_x(Length::Fill),
        )
        .on_press(Message::Install(kind))
        .width(Length::Fixed(style::ACTION))
        .padding([6.0, 8.0])
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
            container(
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
            .width(Length::Fill)
            .center_x(Length::Fill),
        )
        .on_press_maybe(app.can_check().then_some(Message::Check))
        .width(Length::Fixed(style::ACTION))
        .padding([6.0, 8.0])
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
        // One line at [`Sizes::small`] and not two at [`Sizes::text`]: the offered file's name used to
        // be here under this, and between the two of them the row ran out of width for its own button.
        // The offer keeps the row's widest column honest at the smaller size, and the name it used to be
        // under is in the prompt — see this file's header.
        text(standing).size(sizes.small).color(standing_ink),
        action,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    card(body.into(), theme)
}

/// The line under the rows: what happened to the last *check*, in one sentence.
///
/// One line and not a card, because it is not one of the page's two subjects: it is the *state of the
/// question* that was asked about them, and a card would give it the same weight as a version number.
///
/// Nothing about a press is here. A press's outcome is [`crate::Prompt`], over the page, and the arms
/// below say nothing for those states — a sentence in two places at once would be the page saying it
/// twice, and the place a person is looking after pressing a button is the one with the bar in it.
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
        // A press, and a press is drawn in the prompt. The line is still *laid out* — it keeps its
        // height, so the rows above it do not jump when a prompt is opened and closed over it.
        Status::Unsupported(_)
        | Status::Downloading(_)
        | Status::Installed(..)
        | Status::DownloadFailed(_, _) => (String::new(), style::muted(theme)),
    };

    text(line)
        .size(sizes.small)
        .color(ink)
        .width(Length::Fill)
        .into()
}

/// The prompt: the sentence about the last press, over a wash over the page.
///
/// Centred in the whole surface rather than in the panel, so that it is over the middle of whatever the
/// box is actually showing — a 480 px panel in a wider window is still where the eye is, and the wash
/// covers everything either way.
///
/// The bar is drawn only when there is something to fill it *with*: a download whose size nothing has
/// said gets the byte count and no bar, because a bar with no end is a bar that would have to invent
/// one, and a prompt is the one place on this page that must not guess. The count is there in both
/// cases, and it is the thing that shows a slow card is still moving.
fn prompt_overlay<'a>(
    status: &Status,
    prompt: &Prompt,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let (glyph, ink) = match status {
        Status::Installed(..) => (Icon::CHECK_CIRCLE, style::good()),
        Status::DownloadFailed(_, _) => (Icon::ERROR, style::bad()),
        Status::Unsupported(_) => (Icon::WARNING, style::bad()),
        _ => (Icon::DOWNLOADING, style::accent()),
    };

    let mut children: Vec<Element<'a, Message>> = vec![
        row![
            text(glyph.glyph())
                .font(icons::font())
                .size(sizes.glyph)
                .color(ink),
            text(prompt.title).size(sizes.text).color(style::ink(theme)),
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center)
        .into(),
        text(prompt.line.clone())
            .size(sizes.small)
            .color(style::ink(theme))
            .width(Length::Fill)
            .into(),
    ];

    if !prompt.detail.is_empty() {
        children.push(
            text(prompt.detail.clone())
                .size(sizes.tiny)
                .color(style::muted(theme))
                .width(Length::Fill)
                .into(),
        );
    }

    if let Some(fraction) = prompt.progress {
        children.push(
            progress_bar(0.0..=1.0, fraction)
                .length(Length::Fill)
                .girth(8.0)
                .style(move |_theme| progress_bar::Style {
                    background: style::track().into(),
                    bar: style::accent().into(),
                    border: Border {
                        radius: (4.0).into(),
                        ..Border::default()
                    },
                })
                .into(),
        );
    }

    if !prompt.bytes.is_empty() {
        children.push(
            text(prompt.bytes.clone())
                .size(sizes.tiny)
                .color(style::muted(theme))
                .width(Length::Fill)
                .into(),
        );
    }

    // The first button, and what it says is which of the things pressing it does: stop the download,
    // put a report away, or answer "not now" to the question an install leaves. It is never disabled,
    // because there is nothing here that cannot be left.
    let away = button(
        container(text(prompt.button).size(sizes.small).color(style::ink(theme)))
            .width(Length::Fill)
            .center_x(Length::Fill),
    )
    .on_press(Message::Dismiss)
    .width(Length::Fill)
    .padding([8.0, 10.0])
    .style(move |_theme: &Theme, status: button::Status| {
        let fill = if is_pressed(status) {
            style::accent()
        } else {
            style::surface(theme)
        };

        button_style(fill, style::ink(theme), style::RADIUS / 2.0)
    });

    // The second button, for the one sentence on this page that is a question, and drawn *filled* where
    // everything else here is a surface: it is what the page expects a person to press next, and the
    // accent is the colour this page already uses for the thing to be taken. White ink on it rather than
    // the theme's, because the fill does not follow the theme — see [`style::accent`].
    // Spelled out for the reason the row's own action is: the two arms are two different widgets, and
    // `into` has nothing to infer from until they are both `Element`s.
    let answers: Element<'a, Message> = match prompt.confirm {
        None => away.into(),
        Some(confirm) => {
            let reset: Element<'a, Message> = button(
                container(text(confirm).size(sizes.small).color(Color::WHITE))
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            )
            .on_press(Message::Restart)
            .width(Length::Fill)
            .padding([8.0, 10.0])
            .style(move |_theme: &Theme, status: button::Status| {
                let fill = if is_pressed(status) {
                    style::accent_pressed()
                } else {
                    style::accent()
                };

                button_style(fill, Color::WHITE, style::RADIUS / 2.0)
            })
            .into();

            row![away, reset].spacing(style::GAP).into()
        }
    };

    children.push(answers);

    let panel = container(Column::with_children(children).spacing(style::GAP))
        .width(Length::Fixed(style::PROMPT))
        .padding(style::GAP)
        .style(move |_theme| container::Style {
            background: Some(style::card(theme).into()),
            border: Border {
                radius: style::RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow {
                color: Color::from_rgba8(0, 0, 0, 0.35),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..container::Style::default()
        });

    container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(style::backdrop().into()),
            ..container::Style::default()
        })
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
