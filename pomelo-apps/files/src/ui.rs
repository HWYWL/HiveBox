//! The page: a toolbar, a list, and a bar of four actions.
//!
//! This module is the far side of the app from [`crate`]: the root keeps the state and the twelve
//! messages, and everything that turns them into pixels is here. It draws with the widgets iced ships
//! — buttons, containers, rows and a scroller — and no canvas of its own, so the whole page is
//! testable as a widget tree.
//!
//! The three surfaces are stacked in the order a finger meets them: the toolbar says *where*, the
//! list says *what*, and the action bar says *what can be done*. A prompt, when there is one, covers
//! all three, because while a name is being typed nothing else on the page is a target.

use iced::theme::Palette;
use iced::widget::button::Status;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow, Theme};

use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::ThemeMode;

use crate::entry::{self, Entry};
use crate::style::{self, Sizes};
use crate::words::Words;
use crate::{Asking, Files, Message};

/// The theme: one of the project's two palettes, under its own name so a stack trace says which.
pub(crate) fn theme(mode: ThemeMode) -> Theme {
    let (name, base, background) = match mode {
        ThemeMode::Dark => ("PomeloDark", Palette::DARK, Color::from_rgb8(0x12, 0x14, 0x1B)),
        ThemeMode::Light => ("PomeloLight", Palette::LIGHT, Color::from_rgb8(0xF7, 0xF8, 0xFB)),
    };

    Theme::custom(String::from(name), Palette { background, ..base })
}

/// The page.
pub(crate) fn view(files: &Files) -> Element<'_, Message> {
    let words = Words::of(files.preferences().language);
    let sizes = Sizes::new();
    let mode = files.preferences().theme;

    let mut content = column![
        toolbar(files, sizes, mode, words),
        listing(files, sizes, mode, words),
        actions(files, sizes, mode, words),
    ]
    .spacing(style::GAP)
    .width(Length::Fill)
    .height(Length::Fill);

    if let Some(fault) = files.fault() {
        content = content.push(fault_line(fault, sizes, mode));
    }

    let page: Element<_> = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(style::MARGIN)
        .style(move |_theme| container::Style {
            background: Some(style::surface(mode).into()),
            ..container::Style::default()
        })
        .into();

    if files.prompting() {
        iced::widget::stack![page, prompt_layer(files, sizes, mode, words)].into()
    } else {
        page
    }
}

/// The top of the page: which volume, where in it, and the two ways out.
fn toolbar<'a>(
    files: &'a Files,
    sizes: Sizes,
    mode: ThemeMode,
    words: Words,
) -> Element<'a, Message> {
    let chips: Vec<Element<'a, Message>> = files
        .volumes()
        .iter()
        .enumerate()
        .map(|(index, _volume)| {
            volume_chip(index, files.current() == index, sizes, mode, words.volume(index))
        })
        .collect();

    let heading = if chips.is_empty() {
        text(words.no_card).size(sizes.small).color(style::danger())
    } else {
        text(words.refresh).size(sizes.small).color(style::muted(mode))
    };

    let refresh = icon_button(Icon::REFRESH, Message::Refresh, true, sizes, mode, false);
    let back = icon_button(Icon::ARROW_UPWARD, Message::Up, files.can_go_up(), sizes, mode, false);

    let path = text(files.cwd().to_string())
        .size(sizes.small)
        .color(style::muted(mode))
        .width(Length::Fill);

    column![
        row![Row::with_children(chips).spacing(style::GAP), Space::new().width(Length::Fill), refresh]
            .align_y(Alignment::Center),
        row![back, path, heading].spacing(style::GAP).align_y(Alignment::Center),
    ]
    .spacing(style::GAP)
    .into()
}

/// One of the volume tabs.
fn volume_chip<'a>(
    index: usize,
    chosen: bool,
    sizes: Sizes,
    mode: ThemeMode,
    label: &'static str,
) -> Element<'a, Message> {
    let colour = if chosen { Color::WHITE } else { style::muted(mode) };

    button(text(label).size(sizes.small).color(colour))
        .padding([style::GAP / 2.0, style::GAP])
        .style(move |_theme, status| chip_style(status, mode, chosen))
        .on_press(Message::Volume(index))
        .into()
}

/// The list, or a line where it would be.
fn listing<'a>(files: &'a Files, sizes: Sizes, mode: ThemeMode, words: Words) -> Element<'a, Message> {
    if files.entries().is_empty() {
        return container(text(words.empty).size(sizes.small).color(style::muted(mode)))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let rows: Vec<Element<'a, Message>> = files
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| row_of(entry, index, files.selected == Some(index), sizes, mode))
        .collect();

    scrollable(Column::with_children(rows).spacing(style::GAP).width(Length::Fill))
        .height(Length::Fill)
        .into()
}

/// One name.
fn row_of<'a>(
    entry: &'a Entry,
    index: usize,
    chosen: bool,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    let (glyph, tint) = if entry.is_dir() {
        (Icon::FOLDER, style::folder())
    } else {
        (Icon::DESCRIPTION, style::accent())
    };

    let icon = text(glyph.glyph())
        .font(pomelo_material_symbols::font())
        .size(sizes.icon)
        .color(tint);

    let name = text(entry.name.clone()).size(sizes.text).color(style::ink(mode));

    let detail = if entry.is_dir() {
        String::new()
    } else {
        entry::bytes(entry.size)
    };

    let size = text(detail).size(sizes.small).color(style::muted(mode));

    let inside = row![icon, name, Space::new().width(Length::Fill), size]
        .spacing(style::GAP)
        .align_y(Alignment::Center)
        .padding([0.0, style::GAP]);

    button(inside)
        .width(Length::Fill)
        .height(Length::Fixed(sizes.row))
        .style(move |_theme, status| row_style(status, mode, chosen))
        .on_press(Message::Open(index))
        .into()
}

/// The bar of five actions along the bottom.
fn actions<'a>(files: &'a Files, sizes: Sizes, mode: ThemeMode, words: Words) -> Element<'a, Message> {
    let selected = files.selected().is_some();
    let held = files.clipboard().is_some();

    Row::with_children(vec![
        labelled(Icon::CREATE_NEW_FOLDER, words.new_folder, Message::NewFolder, true, sizes, mode, false),
        labelled(Icon::DRIVE_FILE_RENAME, words.rename, Message::Rename, selected, sizes, mode, false),
        labelled(Icon::CONTENT_COPY, words.copy, Message::Copy, selected, sizes, mode, false),
        labelled(Icon::CONTENT_PASTE, words.paste, Message::Paste, held, sizes, mode, false),
        labelled(Icon::DELETE, words.delete, Message::Delete, selected, sizes, mode, true),
    ])
    .spacing(style::GAP)
    .width(Length::Fill)
    .into()
}

/// One action: a glyph over its name, the two of them one target.
fn labelled<'a>(
    icon: Icon,
    label: &'static str,
    message: Message,
    enabled: bool,
    sizes: Sizes,
    mode: ThemeMode,
    danger: bool,
) -> Element<'a, Message> {
    let colour = label_colour(enabled, danger, mode);

    let body = column![
        text(icon.glyph())
            .font(pomelo_material_symbols::font())
            .size(sizes.icon)
            .color(colour),
        text(label).size(sizes.small).color(colour),
    ]
    .spacing(style::GAP / 2.0)
    .align_x(Alignment::Center);

    let mut control = button(container(body).center_x(Length::Fill).padding(style::GAP / 2.0))
        .width(Length::Fill)
        .style(move |_theme, status| action_style(status, mode, enabled, danger));

    if enabled {
        control = control.on_press(message);
    }

    control.into()
}

/// One glyph, on its own.
fn icon_button<'a>(
    icon: Icon,
    message: Message,
    enabled: bool,
    sizes: Sizes,
    mode: ThemeMode,
    danger: bool,
) -> Element<'a, Message> {
    let glyph = text(icon.glyph())
        .font(pomelo_material_symbols::font())
        .size(sizes.icon)
        .color(label_colour(enabled, danger, mode));

    let mut control = button(
        container(glyph)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(style::GAP / 2.0),
    )
    .width(Length::Fixed(sizes.row))
    .height(Length::Fixed(sizes.row))
    .style(move |_theme, status| action_style(status, mode, enabled, danger));

    if enabled {
        control = control.on_press(message);
    }

    control.into()
}

/// The line a failure is written on.
fn fault_line<'a>(fault: &'a str, sizes: Sizes, mode: ThemeMode) -> Element<'a, Message> {
    container(text(fault.to_string()).size(sizes.small).color(style::danger()))
        .width(Length::Fill)
        .padding(style::GAP)
        .style(move |_theme| container::Style {
            background: Some(style::row(mode).into()),
            border: Border {
                radius: style::RADIUS.into(),
                width: 1.0,
                color: style::danger(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The colour a control's glyph and label are drawn in, which depends on whether it can be used.
fn label_colour(enabled: bool, danger: bool, mode: ThemeMode) -> Color {
    if !enabled {
        style::muted(mode)
    } else if danger {
        style::danger()
    } else {
        style::accent()
    }
}

/// A chip: the chosen one is filled with the accent, the others are plain.
fn chip_style(status: Status, mode: ThemeMode, chosen: bool) -> button::Style {
    let pressed = matches!(status, Status::Pressed);

    let background = if chosen {
        style::accent()
    } else if pressed {
        style::row(mode).scale_alpha(1.4)
    } else {
        style::row(mode)
    };

    button::Style {
        background: Some(background.into()),
        text_color: if chosen { Color::WHITE } else { style::ink(mode) },
        border: Border {
            radius: style::RADIUS.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A row: the chosen one carries the pick-out, the rest are a slab.
fn row_style(status: Status, mode: ThemeMode, chosen: bool) -> button::Style {
    let pressed = matches!(status, Status::Pressed);

    let background = if chosen {
        style::selected(mode)
    } else if pressed {
        style::row(mode).scale_alpha(1.4)
    } else {
        style::row(mode)
    };

    button::Style {
        background: Some(background.into()),
        text_color: style::ink(mode),
        border: Border {
            radius: style::RADIUS.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A toolbar or action-bar button: transparent, and only tinted while it is being pressed.
fn action_style(status: Status, mode: ThemeMode, enabled: bool, danger: bool) -> button::Style {
    let pressed = matches!(status, Status::Pressed);

    let background = if pressed && enabled {
        Some(if danger { style::danger() } else { style::accent() }.scale_alpha(0.35).into())
    } else {
        None
    };

    button::Style {
        background,
        text_color: label_colour(enabled, danger, mode),
        border: Border {
            radius: style::RADIUS.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// The box over the list: a title, a name when there is one to type, and two buttons.
///
/// A question about a removal has no box to type in, and that is the whole of the difference between
/// the three prompts: they are the same dialog, and only one of them is asking for a name.
fn prompt_layer<'a>(files: &Files, sizes: Sizes, mode: ThemeMode, words: Words) -> Element<'a, Message> {
    let Some(prompt) = files.prompt.as_ref() else {
        return Space::new().height(0).into();
    };

    let typing = matches!(prompt.asking, Asking::NewFolder | Asking::Rename(_));

    let title = match &prompt.asking {
        Asking::NewFolder => words.ask_new.to_string(),
        Asking::Rename(_) => words.ask_rename.to_string(),
        Asking::Delete(whom, _) => words.ask_delete(crate::base(whom)),
    };

    let mut card = Column::new()
        .spacing(style::GAP)
        .push(text(title).size(sizes.heading).color(style::ink(mode)));

    if typing {
        card = card.push(
            text_input(words.hint, &prompt.text)
                .on_input(Message::Name)
                .on_submit(Message::Confirm)
                .size(sizes.text)
                .padding(style::GAP),
        );
    }

    card = card.push(
        row![
            prompt_button(words.okay, Message::Confirm, sizes, mode, true),
            prompt_button(words.cancel, Message::Cancel, sizes, mode, false),
        ]
        .spacing(style::GAP),
    );

    let boxed = container(card)
        .width(Length::Fixed(style::PANEL * 0.8))
        .padding(style::MARGIN)
        .style(move |_theme| container::Style {
            background: Some(style::row(mode).into()),
            border: Border {
                radius: (style::RADIUS * 1.5).into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            shadow: Shadow::default(),
            ..container::Style::default()
        });

    container(boxed)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba8(0, 0, 0, 0.45).into()),
            ..container::Style::default()
        })
        .into()
}

/// One of the prompt's two buttons: the affirmative one carries the accent, the other is plain.
fn prompt_button<'a>(
    label: &'static str,
    message: Message,
    sizes: Sizes,
    mode: ThemeMode,
    affirmative: bool,
) -> Element<'a, Message> {
    let (background, ink) = if affirmative {
        (style::accent(), Color::WHITE)
    } else {
        (style::surface(mode), style::ink(mode))
    };

    button(text(label).size(sizes.text).color(ink))
        .width(Length::Fill)
        .padding(style::GAP)
        .style(move |_theme, status| prompt_style(status, background, ink))
        .on_press(message)
        .into()
}

/// A prompt button: its colour at rest, a little darker while held.
fn prompt_style(status: Status, background: Color, ink: Color) -> button::Style {
    let pressed = matches!(status, Status::Pressed);
    let fill = if pressed { background.scale_alpha(0.8) } else { background };

    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            radius: style::RADIUS.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: false,
    }
}
