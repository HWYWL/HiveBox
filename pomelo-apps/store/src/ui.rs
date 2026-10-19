//! Everything this page draws: the heading, the three tabs, the rows, the line at the bottom, and
//! the prompt over all of it.
//!
//! # A row is a name, a line, and one button
//!
//! The same row in all three lists, because the person is doing the same thing in all three: reading
//! a name and a version, and deciding whether to press. Only the verb changes — 卸载, 安装, 更新 —
//! and the accent square at the left is the package's own colour when the manifest said one.
//!
//! # The button is the row's conclusion
//!
//! A row that has something to offer shows the verb; a row that is installed shows 卸载; and while a
//! download is out, every other row's button is off, because there is one request slot and a second
//! press would be refused by the backend rather than queued. That is not a limitation drawn on the
//! page for its own sake: it is the board's one radio, and the list says so.
//!
//! # Two kinds of news
//!
//! The check is page-wide and its state is the quiet line at the bottom. A *press* is about one row
//! and is drawn as a prompt over the page, where the person who pressed it is looking.

use iced::widget::{button, container, opaque, progress_bar, row, scrollable, stack, text, Column, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Theme};
use iced::widget::scrollable::{Direction, Scrollbar};

use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::style::{self, Sizes};
use crate::{
    installed_line, release_line, Message, Prompt, Section, Status, Store, Tone, Words,
};

/// The page.
pub(crate) fn page(app: &Store) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = Sizes::new();
    let words = Words::of(app.preferences.language);

    let body = Column::with_children(vec![
        header(app, &words, sizes, mode),
        tabs(app, &words, sizes, mode),
        scrollable(
            Column::with_children(rows(app, &words, sizes, mode))
                .spacing(style::GAP)
                .width(Length::Fill),
        )
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .height(Length::Fill)
        .into(),
        notice(app, &words, sizes, mode),
    ])
    .spacing(style::GAP)
    .padding(style::MARGIN)
    .width(Length::Fill)
    .height(Length::Fill);

    // The panel is 480 px square and a desktop window is not, so the page keeps its width and the
    // space either side of it is the surface's own — the same argument every page here makes.
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

    // The prompt goes *over* the page and never instead of it: `opaque` is what makes it modal, and
    // without it a press would fall through onto the rows underneath.
    match app.prompt(&words) {
        Some(prompt) => stack([opaque(page), prompt_overlay(&prompt, sizes, mode)]).into(),
        None => page.into(),
    }
}

/// The head of the page: what this is, and the button that asks the repository again.
fn header<'a>(app: &Store, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
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
            text(words.check).size(sizes.small).color(style::ink(theme)),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    )
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
        text(Icon::STORE.glyph())
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

/// The three tabs, as one control: the section on screen is filled with the page's colour.
fn tabs<'a>(app: &Store, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let mut children: Vec<Element<'a, Message>> = Vec::with_capacity(Section::ALL.len());

    for section in Section::ALL {
        let chosen = app.section == section;
        let label = section.label(words);

        children.push(
            button(
                container(text(label).size(sizes.small).color(if chosen {
                    style::ink(theme)
                } else {
                    style::muted(theme)
                }))
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .on_press(Message::Section(section))
            .width(Length::Fill)
            .padding([7.0, 6.0])
            .style(move |_theme: &Theme, status: button::Status| {
                let fill = if is_pressed(status) {
                    style::accent()
                } else if chosen {
                    style::tab_on(theme)
                } else {
                    style::card(theme)
                };

                button_style(fill, style::ink(theme), style::RADIUS / 2.0)
            })
            .into(),
        );
    }

    row(children).spacing(6.0).into()
}

/// The rows of the section on screen.
fn rows<'a>(app: &'a Store, words: &Words, sizes: Sizes, theme: ThemeMode) -> Vec<Element<'a, Message>> {
    match app.section {
        Section::Installed => installed_rows(app, words, sizes, theme),
        Section::Available => available_rows(app, words, sizes, theme),
        Section::Updates => update_rows(app, words, sizes, theme),
    }
}

/// What is installed, each row with the one button that takes it away.
fn installed_rows<'a>(
    app: &'a Store,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Vec<Element<'a, Message>> {
    let mut children = Vec::new();

    for installed in app.catalog.apps() {
        let name = app.installed_name(installed).to_string();
        let accent = Color::from_rgb8(
            installed.meta.accent.0,
            installed.meta.accent.1,
            installed.meta.accent.2,
        );

        let line = match app.upgrade_for(installed) {
            Some(release) => format!(
                "{} · {} {}",
                installed_line(installed, words),
                words.upgrade,
                release.version.text()
            ),
            None => installed_line(installed, words),
        };

        children.push(row_card(
            badge(accent, &name, sizes),
            name,
            line,
            action(
                words.uninstall,
                Message::Uninstall(installed.meta.id.clone()),
                false,
                !app.waiting(),
                sizes,
                theme,
            ),
            sizes,
            theme,
        ));
    }

    // Directories that looked like applications and could not be read are shown rather than hidden:
    // a half-downloaded package is something a person can act on, and a tile that simply never
    // appears is not.
    for broken in app.broken() {
        children.push(row_card(
            badge(style::bad(), &broken.id, sizes),
            broken.id.clone(),
            format!("{} · {}", words.broken, broken.reason),
            action(
                words.remove,
                Message::Uninstall(broken.id.clone()),
                false,
                !app.waiting(),
                sizes,
                theme,
            ),
            sizes,
            theme,
        ));
    }

    if children.is_empty() {
        children.push(empty(words.empty_installed, sizes, theme));
    }

    children
}

/// What the repository has and the box does not.
fn available_rows<'a>(
    app: &'a Store,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Vec<Element<'a, Message>> {
    let mut children = Vec::new();

    for release in app.available() {
        children.push(release_row(
            app,
            release,
            words.install,
            words,
            sizes,
            theme,
        ));
    }

    if children.is_empty() {
        children.push(empty(words.empty_available, sizes, theme));
    }

    children
}

/// What is installed and has something newer.
fn update_rows<'a>(
    app: &'a Store,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Vec<Element<'a, Message>> {
    let mut children = Vec::new();

    for (installed, release) in app.updates() {
        let name = app.release_name(release).to_string();

        let line = format!(
            "{} → {} · {}",
            installed.meta.version,
            release.version.text(),
            release.description
        );

        children.push(row_card(
            badge(
                style::accent_of(&release.accent, style::accent()),
                &name,
                sizes,
            ),
            name,
            line,
            action(
                words.upgrade,
                Message::Install(release.id.clone()),
                true,
                !app.waiting(),
                sizes,
                theme,
            ),
            sizes,
            theme,
        ));
    }

    if children.is_empty() {
        children.push(empty(words.empty_updates, sizes, theme));
    }

    children
}

fn release_row<'a>(
    app: &Store,
    release: &updater::AppRelease,
    verb: &'static str,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let name = app.release_name(release).to_string();
    let accent = style::accent_of(&release.accent, style::accent());

    row_card(
        badge(accent, &name, sizes),
        name,
        release_line(release, words),
        action(
            verb,
            Message::Install(release.id.clone()),
            true,
            !app.waiting(),
            sizes,
            theme,
        ),
        sizes,
        theme,
    )
}

/// One card: an accent square, a name over a line, and the row's one button.
fn row_card<'a>(
    badge: Element<'a, Message>,
    name: String,
    line: String,
    action: Element<'a, Message>,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let body = row![
        badge,
        Column::with_children(vec![
            text(name).size(sizes.text).color(style::ink(theme)).into(),
            text(line).size(sizes.small).color(style::muted(theme)).into(),
        ])
        .spacing(2.0)
        .width(Length::Fill),
        action,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    card(body.into(), theme)
}

/// The package's own colour with its first character in it.
///
/// An accent square rather than a bitmap: a package that is not installed has no icon to read (the
/// manifest does not carry one), and drawing the installed package's icon would mean decoding the
/// icon plane here as well as in the launcher. The square is the same picture the launcher uses for
/// an application it has no icon for, which is what makes the two screens agree.
fn badge<'a>(accent: Color, name: &str, sizes: Sizes) -> Element<'a, Message> {
    let initial = name.chars().next().map(String::from).unwrap_or_default();

    container(text(initial).size(sizes.badge).color(Color::WHITE))
    .width(Length::Fixed(style::BADGE))
    .height(Length::Fixed(style::BADGE))
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(accent.into()),
        border: Border {
            radius: (style::RADIUS * 0.75).into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// A row's verb: filled for the thing to take, a surface for the thing to take away.
fn action<'a>(
    label: &'static str,
    message: Message,
    filled: bool,
    enabled: bool,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let ink = if filled { Color::WHITE } else { style::ink(theme) };

    button(
        container(text(label).size(sizes.small).color(ink))
            .width(Length::Fill)
            .center_x(Length::Fill),
    )
    .on_press_maybe(enabled.then_some(message))
    .width(Length::Fixed(style::ACTION))
    .padding([7.0, 8.0])
    .style(move |_theme: &Theme, status: button::Status| {
        let fill = if is_pressed(status) {
            if filled {
                style::accent_pressed()
            } else {
                style::accent()
            }
        } else if filled {
            style::accent()
        } else {
            style::card(theme)
        };

        button_style(fill, ink, style::RADIUS / 2.0)
    })
    .into()
}

/// A card that says a list is empty.
fn empty<'a>(line: &'static str, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    card(
        text(line)
            .size(sizes.small)
            .color(style::muted(theme))
            .width(Length::Fill)
            .into(),
        theme,
    )
}

/// The line under the rows: what happened to the last *check*, in one sentence.
fn notice<'a>(app: &Store, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let (line, ink) = match &app.status {
        // Nothing has been asked for yet, and a page that says it is asking would be lying about a
        // request that does not exist.
        Status::Idle => (String::new(), style::muted(theme)),
        Status::Checking => (words.checking.to_string(), style::muted(theme)),
        Status::Answered => (String::new(), style::muted(theme)),
        Status::Failed(fault) => (words.fault(*fault).to_string(), style::bad()),
        Status::Unreadable => (words.unreadable.to_string(), style::bad()),
        // A press is drawn in the prompt. The line keeps its height so the rows do not jump.
        Status::Downloading(_) => (String::new(), style::muted(theme)),
    };

    text(line)
        .size(sizes.tiny)
        .color(ink)
        .width(Length::Fill)
        .into()
}

/// The prompt: a sentence about the last press, over a wash over the page.
fn prompt_overlay<'a>(prompt: &Prompt, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let (glyph, ink) = match prompt.tone {
        Tone::Working => (Icon::DOWNLOADING, style::accent()),
        Tone::Done => (Icon::CHECK_CIRCLE, style::good()),
        Tone::Failed => (Icon::ERROR, style::bad()),
    };

    let mut children: Vec<Element<'a, Message>> = vec![
        row![
            text(glyph.glyph())
                .font(icons::font())
                .size(sizes.glyph)
                .color(ink),
            text(prompt.title.clone())
                .size(sizes.text)
                .color(style::ink(theme)),
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

    // The second button, for the one prompt that is a question. Drawn filled where everything else
    // is a surface: it is what the page expects a person to press next.
    let answers: Element<'a, Message> = match prompt.confirm {
        None => away.into(),
        Some(confirm) => {
            let yes: Element<'a, Message> = button(
                container(text(confirm).size(sizes.small).color(Color::WHITE))
                    .width(Length::Fill)
                    .center_x(Length::Fill),
            )
            .on_press(Message::Confirm)
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

            row![away, yes].spacing(style::GAP).into()
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

/// Whether a finger is on the button. `Pressed` and only `Pressed`: a touchscreen has no hover.
fn is_pressed(status: button::Status) -> bool {
    matches!(status, button::Status::Pressed)
}
