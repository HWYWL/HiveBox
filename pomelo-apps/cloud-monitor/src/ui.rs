//! Everything this page draws: the header, the list, and the buttons.
//!
//! # Why the commands are in their own card at the bottom
//!
//! Three buttons on every row would be three buttons per machine on a 480 px panel, which is either a
//! row nobody can read or a button nobody can hit. So a row is *picked* — one finger, anywhere on it —
//! and the three commands live in one card under the list, aimed at whatever is picked. The list stays a
//! list, the buttons stay big enough to press, and what a command will act on is the row drawn in this
//! page's own colour.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, container, row, scrollable, text, Column, Space};
use iced::{Alignment, Border, Color, Element, Length};

use pomelo_hal::{CloudAction, CloudInstance};
use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::style::{self, Sizes};
use crate::{CloudMonitor, Message, Notice, Words};

/// The page.
pub(crate) fn page(app: &CloudMonitor) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = app.sizes();
    let words = Words::of(app.preferences.language);

    // One list, not one per product: the board merges the products it has a key for, and each row
    // says which of them it came from. There is nothing left for a person to switch between.
    let mut cards: Vec<Element<'_, Message>> = Vec::new();

    if app.configured() {
        cards.push(list(app, &words, sizes, mode));

        if let Some(commands) = commands(app, &words, sizes, mode) {
            cards.push(commands);
        }
    } else {
        // The one card this page has for a board with no credentials: where the key goes, and not a
        // failure — nothing has been asked of anybody's account yet.
        cards.push(message(
            &words,
            words.no_key,
            words.no_key_note,
            sizes,
            mode,
        ));
    }

    let page = Column::with_children(vec![
        header(app, &words, sizes, mode),
        // No scrollbar, for the reason the Wi-Fi map gives: its rail is drawn over the right edge of the
        // cards and squares off their corners, and a panel has a finger rather than a pointer.
        scrollable(
            Column::with_children(cards)
                .spacing(style::GAP)
                .width(Length::Fill),
        )
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .width(Length::Fill)
        .into(),
    ])
    .spacing(style::GAP)
    .padding(style::MARGIN)
    .width(Length::Fill)
    .height(Length::Fill);

    // The panel is 480 px square and a desktop window is not: the page keeps its width and the space
    // either side of it is the surface's own. See [`style::PANEL`].
    container(
        container(page)
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

/// The head of the page: what this is, how the request is going, and the button to ask again.
fn header<'a>(app: &CloudMonitor, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    // One line under the title, and it is whichever of the four things there is to say is the most
    // useful: what to do about a missing key, that a request is running, what the last one had to say,
    // or how many machines the list holds. See `Words::refused`.
    let (status, colour) = if !app.configured() {
        (words.no_key.to_string(), style::muted(theme))
    } else if app.busy() {
        (words.loading.to_string(), style::muted(theme))
    } else {
        match app.notice() {
            Some(Notice::Accepted { action, name }) => (
                words.accepted(name, words.action(*action)),
                style::good(),
            ),
            // A refusal is the page's one red line: it is the only thing here that is somebody else's
            // answer, and the only one a person has to do something about.
            Some(Notice::Refused {
                fault,
                code,
                message,
            }) => (words.refused(*fault, code, message), style::bad()),
            None => (words.count(app.instances().len()), style::muted(theme)),
        }
    };

    let heading = Column::with_children(vec![
        text(words.title).size(sizes.text).color(style::ink(theme)).into(),
        text(status).size(sizes.small).color(colour).into(),
    ])
    .spacing(2.0);

    let refresh = button(
        row![
            text(Icon::REFRESH.glyph())
                .font(icons::font())
                .size(sizes.small)
                .color(style::ink(theme)),
            text(words.refresh).size(sizes.small).color(style::ink(theme)),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    )
    // Nothing to press while there is nothing to ask with, and nothing to ask for while a request is
    // already in flight: see `CloudMonitor::ask`.
    .on_press_maybe(
        (app.configured() && !app.busy()).then_some(Message::Refresh),
    )
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
        text(Icon::CLOUD_SYNC.glyph())
            .font(icons::font())
            .size(sizes.icon)
            .color(style::accent()),
        heading,
        Space::new().width(Length::Fill),
        refresh,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center)
    .into()
}

/// The list of every machine in the account.
fn list<'a>(
    app: &'a CloudMonitor,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let instances = app.instances();

    let mut rows: Vec<Element<'a, Message>> = if instances.is_empty() {
        vec![text(if app.busy() {
            words.loading
        } else {
            words.empty
        })
        .size(sizes.text)
        .color(style::muted(theme))
        .into()]
    } else {
        instances
            .iter()
            .map(|instance| {
                let picked = app
                    .selected()
                    .is_some_and(|selected| selected.id == instance.id);

                instance_row(instance, picked, words, sizes, theme)
            })
            .collect()
    };

    // With nothing picked and somewhere to pick, the hint goes at the foot of the list rather than in a
    // card of its own: it is about the rows above it.
    if !instances.is_empty() && app.selected().is_none() {
        rows.push(
            text(words.select_hint)
                .size(sizes.tiny)
                .color(style::muted(theme))
                .into(),
        );
    }

    // Half a list is not a list: a product whose request failed is named under the rows that did
    // arrive, so that "my machine is gone" is never the reading of an account that merely did not
    // answer in full. See [`CloudMonitor::failed`].
    if !app.failed().is_empty() {
        rows.push(
            text(words.partial(app.failed()))
                .size(sizes.tiny)
                .color(style::bad())
                .into(),
        );
    }

    card(
        Column::with_children(vec![
            // Every product's machines in one card, so the hint says so rather than naming a region
            // that only one of the two keys would have.
            card_row(Icon::CLOUD, words.instances, words.all, sizes, theme),
            Column::with_children(rows).spacing(style::GAP / 2.0).into(),
        ])
        .spacing(style::GAP / 2.0)
        .into(),
        theme,
    )
}

/// One row of the list: what the machine is doing, what it is called, where it is, and which product
/// it belongs to.
///
/// The whole row is the button: a finger should not have to find a 12 px glyph to pick a machine, and
/// what picking it does is put the three commands at the foot of the page onto it.
fn instance_row<'a>(
    instance: &CloudInstance,
    picked: bool,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let colour = style::state_colour(instance.state, theme);

    let name = Column::with_children(vec![
        text(if instance.name.is_empty() {
            instance.id.clone()
        } else {
            instance.name.clone()
        })
        .size(sizes.name)
        .color(style::ink(theme))
        .into(),
        // The id, and only when the name above it is not already the id: two identical lines would be
        // one line drawn twice.
        text(if instance.name.is_empty() {
            instance.ip.clone()
        } else {
            format!("{}  {}", instance.id, instance.ip)
        })
        .size(sizes.small)
        .color(style::muted(theme))
        .into(),
    ])
    .spacing(1.0)
    .width(Length::Fill);

    let fill = if picked {
        // The page's colour, at the weight a selection is: enough to see which row the commands at the
        // foot of the page are aimed at, not enough to hide the state beside it.
        Color {
            a: 0.22,
            ..style::accent()
        }
    } else {
        style::track(theme)
    };

    button(
        row![
            text(style::state_icon(instance.state).glyph())
                .font(icons::font())
                .size(sizes.text)
                .color(colour),
            name,
            // Which product this row came from: the one thing a merged list would otherwise lose,
            // and the product its power commands are sent through.
            text(words.provider(instance.provider))
                .size(sizes.tiny)
                .color(style::muted(theme)),
            text(words.state(instance.state))
                .size(sizes.small)
                .color(colour),
        ]
        .spacing(style::GAP / 2.0)
        .align_y(Alignment::Center),
    )
    .on_press(Message::Select(instance.id.clone()))
    .width(Length::Fill)
    .height(Length::Fixed(style::ROW))
    .padding([6.0, 10.0])
    .style(move |_theme, status| {
        let fill = if is_pressed(status) {
            style::card(theme)
        } else {
            fill
        };

        button_style(fill, style::ink(theme), style::RADIUS / 2.0)
    })
    .into()
}

/// The three commands, aimed at whatever row is picked.
///
/// `None` when nothing is picked, which is the page with no commands on it at all: a card of three
/// greyed-out buttons is a card that says "you have not picked anything" in the least useful way.
fn commands<'a>(
    app: &CloudMonitor,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Option<Element<'a, Message>> {
    let instance = app.selected()?;

    let mut buttons: Vec<Element<'a, Message>> = Vec::new();

    for action in [CloudAction::Start, CloudAction::Stop, CloudAction::Reboot] {
        // Neither API takes a command for a machine it is already changing, and a start is not a
        // question worth asking about a machine that is up: the button is drawn and not offered.
        let offered = !instance.state.is_busy()
            && match action {
                CloudAction::Start => !instance.state.is_running(),
                CloudAction::Stop => instance.state.is_running(),
                _ => instance.state.is_running(),
            };

        buttons.push(
            command(
                action,
                offered && !app.busy(),
                app.armed(action),
                words,
                sizes,
                theme,
            )
            .into(),
        );
    }

    let heading = Column::with_children(vec![
        text(if instance.name.is_empty() {
            instance.id.clone()
        } else {
            instance.name.clone()
        })
        .size(sizes.text)
        .color(style::ink(theme))
        .into(),
        text(instance.id.clone())
            .size(sizes.tiny)
            .color(style::muted(theme))
            .into(),
    ])
    .spacing(1.0);

    Some(card(
        Column::with_children(vec![
            row![
                text(style::state_icon(instance.state).glyph())
                    .font(icons::font())
                    .size(sizes.text)
                    .color(style::state_colour(instance.state, theme)),
                heading,
                Space::new().width(Length::Fill),
            ]
            .spacing(style::GAP / 2.0)
            .align_y(Alignment::Center)
            .into(),
            row(buttons).spacing(style::GAP / 2.0).into(),
        ])
        .spacing(style::GAP / 2.0)
        .into(),
        theme,
    ))
}

/// One command button.
///
/// `armed` is the second press of a stop made visible: the button turns into a question, in the page's
/// colour, and the press after that one sends it. See [`Message::Power`].
fn command<'a>(
    action: CloudAction,
    offered: bool,
    armed: bool,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let label = if armed {
        words.confirm_stop
    } else {
        words.action(action)
    };

    // A command that would not be taken is drawn in the muted ink and has no press: not hidden, because
    // a button that disappears is a button a person looks for, and not offered, because the API would
    // refuse it.
    let ink = if offered {
        style::ink(theme)
    } else {
        style::muted(theme)
    };

    button(
        row![
            text(command_icon(action).glyph())
                .font(icons::font())
                .size(sizes.text)
                .color(if offered {
                    style::accent()
                } else {
                    style::muted(theme)
                }),
            text(label).size(sizes.text).color(ink),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(offered.then_some(Message::Power(action)))
    .width(Length::Fill)
    .padding([10.0, 8.0])
    .style(move |_theme, status| {
        let fill = if armed {
            style::accent()
        } else if is_pressed(status) {
            style::track(theme)
        } else {
            style::card(theme)
        };

        button_style(fill, style::ink(theme), style::RADIUS)
    })
    .into()
}

/// The picture on a command button: the same three the launcher's tile would use for a machine, seen
/// from the side of the person asking.
fn command_icon(action: CloudAction) -> Icon {
    match action {
        CloudAction::List => Icon::REFRESH,
        CloudAction::Start => Icon::PLAY_ARROW,
        CloudAction::Stop => Icon::STOP,
        CloudAction::Reboot => Icon::RESTART_ALT,
    }
}

/// A line of text in a card of its own: what the page says when there is nothing to draw.
fn message<'a>(
    _words: &Words,
    line: &'static str,
    note: &'static str,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    card(
        Column::with_children(vec![
            text(line).size(sizes.text).color(style::ink(theme)).into(),
            text(note).size(sizes.small).color(style::muted(theme)).into(),
        ])
        .spacing(2.0)
        .into(),
        theme,
    )
}

/// A card's heading: its icon, its title, and a hint about what is in it.
pub(crate) fn card_row<'a>(
    icon: Icon,
    title: &'static str,
    hint: &'a str,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    row![
        text(icon.glyph())
            .font(icons::font())
            .size(sizes.text)
            .color(style::muted(theme)),
        text(title).size(sizes.text).color(style::ink(theme)),
        Space::new().width(Length::Fill),
        text(hint).size(sizes.tiny).color(style::muted(theme)),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .into()
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

/// Whether a button is being held down, which is the one state that changes how it is filled.
fn is_pressed(status: button::Status) -> bool {
    matches!(status, button::Status::Pressed)
}

/// A button's capsule: its fill, and the ink its contents default to.
fn button_style(fill: Color, ink: Color, radius: f32) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}
