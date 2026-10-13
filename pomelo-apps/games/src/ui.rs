//! Everything this app draws: the list of games, and whichever game is being played.
//!
//! # Two screens, drawn by two functions
//!
//! The list is rows — a glyph, a name, a line about it, and a chevron — and a game is a board with a
//! pad under it. There is nothing between them to share but a header, and the two headers are not the
//! same header: the list's says what the app is, and a game's says which game, how it is going and how
//! to get back.
//!
//! # Two games, one page
//!
//! Both games are the same page — [`header`], a board, and [`foot`] — and the board is the only thing
//! that differs: sixteen tiles with gaps between them, or sixteen cells by sixteen with none. The
//! caption is the other difference, and it is the game's own line, because the two are played
//! differently. Everything shared is shared on purpose: a game added here is a board and a line.
//!
//! # The board is a fixed square
//!
//! [`style::TILE`] and [`style::TILE_GAP`] are constants and not the base font size's business,
//! because a 2048 board is four tiles by four: on the 480 px panel it is 328 px, and every other
//! measurement on this page was chosen to fit *around* it. The chrome still scales with the base
//! size, which is why the arithmetic is checked — see the test in [`crate::style`]. The snake's board
//! is the same square — see [`style::SNAKE_CELL`].

use iced::widget::{button, container, row, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow};

use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::game::TwentyFortyEight;
use crate::grid::{Direction, Grid, SIDE};
use crate::snake::{self, Snake};
use crate::style::{self, Part, Sizes};
use crate::{Game, Games, Message, Page, Played, Words};

/// The page.
pub(crate) fn page(app: &Games) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = Sizes::new();
    let words = Words::of(app.preferences.language);

    // The page's own edge. The panel is 480 px square and a desktop window is not: the page keeps its
    // width and the space either side of it is the surface's own — see [`style::PANEL`].
    let body: Element<'_, Message> = match &app.page {
        Page::List => list(&words, sizes, mode),
        Page::Playing(played) => playing(played, &words, sizes, mode),
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
///
/// The two games draw the same page with different middles — the same header, the same line and the
/// same pad, and a board that is sixteen cells of something either way — so the difference is here and
/// in the two functions below it, and nowhere else.
fn playing<'a>(
    played: &Played,
    words: &Words,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    match played {
        Played::Tiles(game) => tiles_page(played.game(), game, words, sizes, mode),
        Played::Snake(snake) => snake_page(played.game(), snake, words, sizes, mode),
    }
}

/// 2048 being played.
fn tiles_page<'a>(
    game: Game,
    played: &TwentyFortyEight,
    words: &Words,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    // The board's own judgement, read once: three of the four things on this page need it.
    let over = played.is_over();
    let scored = format!("{} {}", words.score, played.score());

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

    Column::with_children(vec![
        header(game, &scored, words, sizes, mode),
        tiles(played.grid(), mode),
        foot(line, colour, !over, sizes, mode),
    ])
    .spacing(style::GAP)
    .width(Length::Fill)
    .padding(style::MARGIN)
    .into()
}

/// Snake being played.
///
/// The line under the board is the one difference: a snake is steered by tipping the box as well as
/// by the arrows, so its caption says both — a page that only named the pad would be leaving out the
/// way most people will play it.
fn snake_page<'a>(
    game: Game,
    snake: &Snake,
    words: &Words,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    let over = snake.is_over();
    let scored = format!("{} {}", words.score, snake.score());

    let (line, colour) = if over {
        (words.hit, style::muted(mode))
    } else {
        (words.steer, style::muted(mode))
    };

    Column::with_children(vec![
        header(game, &scored, words, sizes, mode),
        snake_board(snake, mode),
        foot(line, colour, !over, sizes, mode),
    ])
    .spacing(style::GAP)
    .width(Length::Fill)
    .padding(style::MARGIN)
    .into()
}

/// The header every game's page has: the way back, which game is up and how it is going, and a new
/// game.
///
/// Shared rather than written twice, because it *is* the same header: the way back is a page's, the
/// restart is a game's, and only the glyph and the name change — which is what [`Game`] is for.
fn header<'a>(
    game: Game,
    under: &str,
    words: &Words,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    row![
        // The way back is the header's first control, on the left, where a page's way back is.
        icon_button(Icon::ARROW_BACK, None, sizes, mode, Message::List),
        heading(game.icon(), game.name(), under, sizes, mode),
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
    .align_y(Alignment::Center)
    .into()
}

/// The line under the board and the pad under it: the same place, and the same height, whichever
/// sentence is drawn — which is what keeps the board from moving when the line changes.
fn foot<'a>(
    line: &'a str,
    colour: Color,
    live: bool,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    Column::with_children(vec![
        text(line).size(sizes.small).color(colour).into(),
        pad(live, sizes, mode),
    ])
    .spacing(style::GAP / 2.0)
    .width(Length::Fill)
    .align_x(Alignment::Center)
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

/// The snake's board: sixteen cells by sixteen, with no gaps, in the same frame as the other game's.
///
/// Every cell is drawn, including the empty ones, because a snake can be in any of them: the board is
/// not sixteen tiles that moved, it is a grid with a body on it. Which means the board is built here
/// and not by the rules — [`Snake`] answers where its body, its head and its apple are, and this is
/// the one place that turns those three answers into 256 cells.
///
/// The frame is the other board's, letter for letter: the same `TILE_GAP` of padding and the same
/// corner, so the two games present the same square whatever is played on it. The cells touch because
/// a snake's segments have to — a snake drawn with the tiles' gaps would be a dotted line.
fn snake_board<'a>(snake: &Snake, mode: ThemeMode) -> Element<'a, Message> {
    // What each of the 256 cells is, in one pass: the body first and then the apple over it, because
    // the apple is never on the snake and writing it last says so.
    let mut parts = vec![Part::Empty; snake::CELLS];

    for cell in snake.body() {
        let part = if cell == snake.head() {
            Part::Head
        } else {
            Part::Body
        };

        parts[cell.row * snake::SIDE + cell.col] = part;
    }

    let food = snake.food();
    parts[food.row * snake::SIDE + food.col] = Part::Apple;

    let rows = (0..snake::SIDE).map(|at| {
        Row::with_children(
            (0..snake::SIDE)
                .map(|along| snake_cell(parts[at * snake::SIDE + along], mode))
                .collect::<Vec<_>>(),
        )
    });

    let board = container(Column::with_children(rows.map(Element::from).collect::<Vec<_>>()))
        .padding(style::TILE_GAP)
        .style(move |_theme| container::Style {
            background: Some(style::board(mode).into()),
            border: Border {
                radius: style::RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    // A fixed square that takes no part in the column's width, for the reason the tiles are: it stays
    // a square in a window wider than the panel.
    row![Space::new().width(Length::Fill), board, Space::new().width(Length::Fill)]
        .width(Length::Fill)
        .into()
}

/// One cell of the snake's board.
///
/// No corner and no gap: a cell of a body has to be able to touch the cell next to it, which is the
/// whole difference between this and [`tile`]. An empty one is the frame's own colour, so the board
/// reads as one square with something on it rather than as a tray of dark cells.
fn snake_cell<'a>(part: Part, mode: ThemeMode) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fixed(style::SNAKE_CELL))
        .height(Length::Fixed(style::SNAKE_CELL))
        .style(move |_theme| container::Style {
            background: Some(style::snake(part, mode).into()),
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
        let press = live.then_some(Message::Steer(direction));
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
