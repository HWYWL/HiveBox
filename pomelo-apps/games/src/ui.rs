//! Everything this app draws: the list of games, and a game being played.
//!
//! # Two screens, drawn by two functions
//!
//! The list is rows — a glyph, a name, a line about it, and a chevron — and a game is a board with a
//! pad under it. There is nothing between them to share but a header, and the two headers are not the
//! same header: the list's says what the app is, and a game's says which game, how it is going and how
//! to get back.
//!
//! # The board is a fixed square
//!
//! [`style::TILE`] and [`style::TILE_GAP`] are constants and not the font tier's business, because a
//! 2048 board is four tiles by four: on the 480 px panel it is 328 px, and every other measurement on
//! this page was chosen to fit *around* it. The chrome still scales with the tier, which is why the
//! arithmetic is checked — see the test in [`crate::style`].

use iced::widget::{button, container, row, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow};

use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::game::TwentyFortyEight;
use crate::grid::{Direction, Grid, SIDE};
use crate::style::{self, Sizes};
use crate::{Game, Games, Message, Page, Words};

/// The page.
pub(crate) fn page(app: &Games) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = Sizes::of(app.preferences.font_tier);
    let words = Words::of(app.preferences.language);

    // The page's own edge. The panel is 480 px square and a desktop window is not: the page keeps its
    // width and the space either side of it is the surface's own — see [`style::PANEL`].
    let body: Element<'_, Message> = match &app.page {
        Page::List => list(&words, sizes, mode),
        Page::Playing(which, played) => playing(*which, played, &words, sizes, mode),
    };

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

/// The list of games: one row each, and the whole row is the button.
fn list<'a>(words: &Words, sizes: Sizes, mode: ThemeMode) -> Element<'a, Message> {
    let title = heading(Icon::GAMEPAD, words.title, "", sizes, mode);

    let mut rows: Vec<Element<'a, Message>> = vec![title.into()];
    rows.extend(
        Game::ALL
            .into_iter()
            .map(|game| game_row(game, words, sizes, mode)),
    );

    Column::with_children(rows)
        .spacing(style::GAP)
        .width(Length::Fill)
        .padding(style::MARGIN)
        .into()
}

/// One game in the list: its glyph, its name, what it is, and a chevron saying the row opens.
///
/// The whole row is the button rather than just the name, because the row is what a person points at:
/// a row that was a card with a word in it you had to hit would be a row that failed at being a list.
fn game_row<'a>(game: Game, words: &Words, sizes: Sizes, mode: ThemeMode) -> Element<'a, Message> {
    let name = Column::with_children(vec![
        text(game.name()).size(sizes.text).color(style::ink(mode)).into(),
        text(words.about(game))
            .size(sizes.small)
            .color(style::muted(mode))
            .into(),
    ])
    .spacing(2.0);

    let body = row![
        text(game.icon().glyph())
            .font(icons::font())
            .size(sizes.title)
            .color(style::muted(mode)),
        name,
        Space::new().width(Length::Fill),
        text(Icon::CHEVRON_RIGHT.glyph())
            .font(icons::font())
            .size(sizes.text)
            .color(style::muted(mode)),
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    button(body)
        .width(Length::Fill)
        .padding(style::GAP)
        .on_press(Message::Open(game))
        .style(move |_theme, status| {
            let fill = if is_pressed(status) {
                style::pressed()
            } else {
                style::card(mode)
            };

            button_style(fill, style::RADIUS)
        })
        .into()
}

/// A game being played: which one, how it is going, the board, and the pad under it.
fn playing<'a>(
    game: Game,
    played: &TwentyFortyEight,
    words: &Words,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    // The board's own judgement, read once: three of the four things on this page need it.
    let over = played.is_over();
    let scored = format!("{} {}", words.score, played.score());

    let head = row![
        // The way back is the header's first control, on the left, where a page's way back is.
        icon_button(Icon::ARROW_BACK, None, sizes, mode, Message::List),
        heading(game.icon(), game.name(), &scored, sizes, mode),
        Space::new().width(Length::Fill),
        icon_button(
            Icon::RESTART_ALT,
            Some(words.restart),
            sizes,
            mode,
            Message::Restart,
        ),
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    // One line under the board, and it is the same line whichever state the game is in: the caption
    // saying which way the arrows go, or the sentence about a game that is over, or the one about a
    // game that has been won. Same place, same height, so nothing moves when it changes — which is
    // what makes the board, and not the page, the thing a person is looking at.
    let (line, colour) = if over {
        (words.over, style::muted(mode))
    } else if played.has_won() {
        (words.won, style::won())
    } else {
        (words.slide, style::muted(mode))
    };

    let foot = Column::with_children(vec![
        text(line).size(sizes.small).color(colour).into(),
        pad(!over, sizes, mode),
    ])
    .spacing(style::GAP / 2.0)
    .width(Length::Fill)
    .align_x(Alignment::Center);

    Column::with_children(vec![head.into(), tiles(played.grid(), mode), foot.into()])
        .spacing(style::GAP)
        .width(Length::Fill)
        .padding(style::MARGIN)
        .into()
}

/// The board: sixteen cells, four to a row, with the pad's gaps between them.
fn tiles<'a>(grid: &Grid, mode: ThemeMode) -> Element<'a, Message> {
    let rows = (0..SIDE).map(|at| {
        Row::with_children(
            (0..SIDE)
                .map(|along| tile(grid.get(at, along), mode))
                .collect::<Vec<_>>(),
        )
        .spacing(style::TILE_GAP)
    });

    // The tiles are in a column inside a container rather than in a styled column: it is the frame
    // that has the colour and the corner, and the tiles are set *into* it by the padding.
    let board = container(
        Column::with_children(rows.map(Element::from).collect::<Vec<_>>())
            .spacing(style::TILE_GAP),
    )
    .padding(style::TILE_GAP)
    .style(move |_theme| container::Style {
        background: Some(style::board(mode).into()),
        border: Border {
            radius: style::RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    });

    // The board is a fixed square and takes no part in the column's width: it is `Length::Shrink`
    // inside a centred row, so that it stays a square in a window that is wider than the panel.
    row![Space::new().width(Length::Fill), board, Space::new().width(Length::Fill)]
        .width(Length::Fill)
        .into()
}

/// One cell of the board.
///
/// An empty cell is drawn as the frame's colour a step towards the ink rather than as nothing: a board
/// of tiles floating on a background would be a board whose gaps a person has to count, and counting
/// the gaps is not the game.
fn tile<'a>(value: u32, mode: ThemeMode) -> Element<'a, Message> {
    let (fill, ink) = style::tile(value, mode);

    // A zero is not drawn as a number. The game's own rule is that an empty cell is empty, and a `0`
    // in it would be a tile somebody might try to merge with.
    let label = if value == 0 {
        String::new()
    } else {
        value.to_string()
    };

    container(
        text(label)
            .size(style::tile_text(value))
            .color(ink),
    )
    .width(Length::Fixed(style::TILE))
    .height(Length::Fixed(style::TILE))
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(fill.into()),
        border: Border {
            // A tile's corner is the board's, a step tighter, so that a tile reads as set *into* the
            // frame rather than as a card lying on it.
            radius: (style::RADIUS - style::TILE_GAP / 2.0).into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// The pad: the four arrows, in the order left, up, down, right.
///
/// Four buttons in a row rather than a cross with a middle in it, and the order is the one a keyboard
/// has — a cross needs a centre, and the centre of this pad would be a control that means nothing.
/// The row is exactly as wide as the board ([`style::BOARD`]) with the board's own gaps, so each arrow
/// stands under the column or the row it moves.
fn pad<'a>(live: bool, sizes: Sizes, mode: ThemeMode) -> Element<'a, Message> {
    let arrows = [
        (Icon::KEYBOARD_ARROW_LEFT, Direction::Left),
        (Icon::KEYBOARD_ARROW_UP, Direction::Up),
        (Icon::KEYBOARD_ARROW_DOWN, Direction::Down),
        (Icon::KEYBOARD_ARROW_RIGHT, Direction::Right),
    ];

    // The four buttons are the board's width, and the gaps between them are the tiles': the width each
    // one gets is what is left over once those gaps are taken out.
    let each = (style::BOARD - style::TILE_GAP * 3.0) / 4.0;

    let buttons = arrows.into_iter().map(|(icon, direction)| {
        // Nothing to press on a board with no moves left. The arrows are still drawn — taking them
        // away would move the board up the page at the exact moment somebody is looking at it — and
        // the line above them is what says why they do nothing.
        let press = live.then_some(Message::Slide(direction));
        let ink = if live {
            style::ink(mode)
        } else {
            style::muted(mode)
        };

        button(
            text(icon.glyph())
                .font(icons::font())
                .size(sizes.title)
                .color(ink),
        )
        .width(Length::Fixed(each))
        .height(Length::Fixed(style::PAD))
        .on_press_maybe(press)
        .style(move |_theme, status| {
            let fill = if is_pressed(status) {
                style::pressed()
            } else {
                style::card(mode)
            };

            button_style(fill, style::RADIUS / 2.0)
        })
    });

    Row::with_children(buttons.map(Element::from).collect::<Vec<_>>())
        .spacing(style::TILE_GAP)
        .into()
}

/// A glyph, a title and a line under it: what a page is, or which game is on it.
///
/// The line is a `String` rather than a `&'static str` because a game's second line has a number in
/// it, and the number is the point of it.
fn heading<'a>(
    icon: Icon,
    title: &'static str,
    under: &str,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    let lines: Vec<Element<'a, Message>> = vec![
        text(title).size(sizes.title).color(style::ink(mode)).into(),
        text(under.to_string())
            .size(sizes.small)
            .color(style::muted(mode))
            .into(),
    ];

    row![
        text(icon.glyph())
            .font(icons::font())
            .size(sizes.title)
            .color(style::muted(mode)),
        Column::with_children(lines).spacing(2.0),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .into()
}

/// A button that is a glyph, and the word for it beside it when there is one.
///
/// Two of the game's three controls are glyphs — the way back to the list and a new game — and the
/// second one draws its word as well, because "start again" is a decision and a circle with an arrow
/// around it is not a sentence. The word is optional so that the two are one function.
fn icon_button<'a>(
    icon: Icon,
    label: Option<&'static str>,
    sizes: Sizes,
    mode: ThemeMode,
    message: Message,
) -> Element<'a, Message> {
    let mut contents: Vec<Element<'a, Message>> = vec![text(icon.glyph())
        .font(icons::font())
        .size(sizes.text)
        .color(style::ink(mode))
        .into()];

    if let Some(label) = label {
        contents.push(
            text(label)
                .size(sizes.small)
                .color(style::ink(mode))
                .into(),
        );
    }

    button(
        Row::with_children(contents)
            .spacing(4.0)
            .align_y(Alignment::Center),
    )
    .padding([6.0, 10.0])
    .on_press(message)
    .style(move |_theme, status| {
        let fill = if is_pressed(status) {
            style::pressed()
        } else {
            style::card(mode)
        };

        button_style(fill, style::RADIUS / 2.0)
    })
    .into()
}

/// A button's capsule: its fill and its corner. The ink is set per glyph rather than here, because
/// every button on this page has two colours in it — a glyph and a word.
fn button_style(fill: Color, radius: f32) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: Color::WHITE,
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
/// `Pressed` and only `Pressed`. A touchscreen has no hover, and this platform leaves the pointer
/// where the finger lifted, so a button that has been touched stays hovered until something else moves
/// the pointer — which is a button that stays lit after it is let go.
fn is_pressed(status: button::Status) -> bool {
    matches!(status, button::Status::Pressed)
}
