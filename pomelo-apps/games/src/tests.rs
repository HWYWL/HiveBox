//! The app's own tests: which screen is on the panel, what a press does to it, and what it draws.
//!
//! Nothing here names a platform layer or draws a pixel. The board is the HAL's simulator, the widget
//! tree is the real one through `iced_test`, and the game is the real game — a board that has been
//! played, or a position written down when the test needs one nobody could play their way to.
//!
//! The rules themselves are [`crate::grid`]'s tests and the generator is [`crate::random`]'s: what is
//! asserted here is the wiring between them and the two screens — that the list opens a board, that a
//! slide reaches it, that the way back is where a page's way back is, and that a board with nothing
//! left to do says so rather than going quiet.

use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::Key;
use iced::Size;
use iced_test::Simulator;
use pomelo_hal::Board;
use pomelo_widgets::preferences::{FontSizeTier, Language, SystemPreferences, ThemeMode};

use crate::game::TwentyFortyEight;
use crate::grid::{Direction, SIDE};
use crate::style::PANEL;
use crate::{direction, Game, Games, Message, Page};

/// A screen tall enough to show the whole page without scrolling.
///
/// The page is a header, a board and a pad, and it is designed to fit the 480 px panel — but the
/// simulator has no scroll, so anything below the fold is invisible to `find`. Generous rather than
/// exact: a number that had to be re-tuned with every gap would turn a metric change into a test
/// failure, and the panel's own arithmetic is `style`'s tests.
const TALL: f32 = 800.0;

/// A full board with no two neighbours equal: no empty cell, nothing to merge, nothing to do.
///
/// Written down as cells rather than played into being, because a position nobody can move from is not
/// a position a game reaches *deterministically* — it is where one particular seed ends up, and a test
/// that depended on one would be a test that failed the day the generator was touched.
const STUCK: [u32; SIDE * SIDE] = [
    2, 4, 2, 4, //
    4, 2, 4, 2, //
    2, 4, 2, 4, //
    4, 2, 4, 2,
];

/// The board a two-of-them board is written as, for the game that has been won.
const TWO_OF_THEM: [u32; SIDE * SIDE] = [
    2048, 2048, 0, 0, //
    0, 0, 0, 0, //
    0, 0, 0, 0, //
    0, 0, 0, 0,
];

/// A board with nothing plugged into it. The game asks it one question — see `Games::seed` — and
/// answers everything else itself.
fn board() -> Arc<Board> {
    Arc::new(Board::simulated())
}

/// The app's page on a screen `height` logical pixels tall.
///
/// The font is asked for by *generic* family, not by name: the tester's own default is "Fira Sans",
/// which only exists when iced's `fira-sans` feature is on — glyphs this firmware has no room for. A
/// generic resolves against whatever the machine has, so text measures to real glyphs.
fn screen(app: &Games, height: f32) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(PANEL, height),
        app.view(),
    )
}

/// The app in English, on the list.
///
/// The language is set because the view tests assert English labels: the default is Chinese, and a test
/// that meant to check a label would otherwise pass or fail on a default it does not control.
fn listing() -> Games {
    let mut app = Games::new(board());

    app.set_preferences(SystemPreferences {
        language: Language::English,
        theme: ThemeMode::Dark,
        font_tier: FontSizeTier::Standard,
        ..SystemPreferences::default()
    });

    app
}

/// The app with 2048 open, in English, played by nothing.
fn opened() -> Games {
    let mut app = listing();
    app.update(Message::Open(Game::TwentyFortyEight));

    app
}

/// The app with a board written down on it, for the two positions a game cannot be played into.
fn showing(cells: [u32; SIDE * SIDE]) -> Games {
    let mut app = listing();
    app.page = Page::Playing(Game::TwentyFortyEight, TwentyFortyEight::from_cells(cells));

    app
}

/// The app opens on the list, and the list has the one game on it and says what it is.
#[test]
fn the_app_opens_on_the_list() {
    let app = listing();

    assert!(!app.playing(), "nothing is being played yet");
    assert!(app.game().is_none());

    let mut ui = screen(&app, TALL);
    assert!(ui.find("Games").is_ok(), "the page says what it is");
    assert!(ui.find("2048").is_ok(), "and names the game in it");
    assert!(
        ui.find("Slide the tiles together and make 2048").is_ok(),
        "and says what the game is, because a name is not a rule"
    );
}

/// Picking the row deals a board: two tiles, no score, and a game rather than a list.
#[test]
fn picking_a_game_deals_a_board() {
    let app = opened();

    assert!(app.playing(), "a game is open");

    let game = app.game().expect("and it is a game");
    let tiles: Vec<u32> = game
        .grid()
        .cells()
        .iter()
        .copied()
        .filter(|cell| *cell != 0)
        .collect();

    assert_eq!(tiles.len(), 2, "2048 opens with two tiles");
    assert!(tiles.iter().all(|tile| *tile == 2 || *tile == 4));
    assert_eq!(game.score(), 0);
    assert!(!game.is_over());

    let mut ui = screen(&app, TALL);
    assert!(ui.find("Score 0").is_ok(), "and the page says so");
    assert!(
        ui.find("Slide with the arrows below").is_ok(),
        "and which way the board is played"
    );
}

/// The way back is the list, and the game is not kept behind it.
#[test]
fn the_way_back_is_the_list_again() {
    let mut app = opened();
    app.update(Message::List);

    assert!(!app.playing());
    assert!(app.game().is_none(), "a game left behind is not paused");

    let mut ui = screen(&app, TALL);
    assert!(ui.find("Slide the tiles together and make 2048").is_ok());
}

/// A slide reaches the board, and the page is drawn from what it did.
///
/// On a position rather than on a dealt board, and that is the point: what a slide does to a board
/// depends on where the tiles are, and the dealt board is wherever the clock put it. This is the
/// wiring — a message, the rules, and the drawing of the result — with every number in it known.
#[test]
fn a_slide_reaches_the_board() {
    let mut app = listing();
    let position = [
        0, 0, 2, 2, //
        0, 0, 0, 0, //
        0, 0, 0, 0, //
        0, 0, 0, 0,
    ];
    app.page = Page::Playing(
        Game::TwentyFortyEight,
        TwentyFortyEight::from_cells(position),
    );

    app.update(Message::Slide(Direction::Left));

    let game = app.game().expect("a game");
    assert_eq!(game.grid().get(0, 0), 4, "the two twos met at the wall");
    assert_eq!(game.score(), 4);
    assert_eq!(
        game.grid().cells().iter().filter(|cell| **cell != 0).count(),
        2,
        "and the tile that arrived behind them is the only other one"
    );

    let mut ui = screen(&app, TALL);
    assert!(
        ui.find("Score 4").is_ok(),
        "and the page is drawn from the board rather than from before it"
    );
}

/// A slide on the list is not a slide of anything, and it is not a panic either.
#[test]
fn a_slide_on_the_list_does_nothing() {
    let mut app = listing();

    app.update(Message::Slide(Direction::Left));
    app.update(Message::Restart);

    assert!(!app.playing(), "the list is still the list");

    let mut ui = screen(&app, TALL);
    assert!(ui.find("Slide the tiles together and make 2048").is_ok());
}

/// A new game is a new game: two tiles again, and the score back to nothing.
#[test]
fn a_new_game_starts_again() {
    let mut app = opened();
    app.update(Message::Restart);

    let game = app.game().expect("a game");
    let tiles = game.grid().cells().iter().filter(|cell| **cell != 0).count();

    assert_eq!(tiles, 2);
    assert_eq!(game.score(), 0);
    assert!(!game.is_over());

    let mut ui = screen(&app, TALL);
    assert!(ui.find("Score 0").is_ok(), "and the page says so");
}

/// A board with nothing left to do says so, rather than going quiet.
///
/// The line is drawn where the caption is, and it is the same height: the board must not move up the
/// page at the exact moment somebody is looking at it.
#[test]
fn a_finished_board_says_so() {
    let app = showing(STUCK);
    assert!(app.game().expect("a game").is_over());

    let mut ui = screen(&app, TALL);
    assert!(
        ui.find("No moves left — start a new game").is_ok(),
        "the page says what happened and what to do about it"
    );
    assert!(
        ui.find("Slide with the arrows below").is_err(),
        "and stops telling anybody which way to slide"
    );
}

/// A board that has been won says so and goes on: 2048 is not the end of the game, it is its name.
#[test]
fn a_won_board_says_so_and_goes_on() {
    let mut game = TwentyFortyEight::from_cells(TWO_OF_THEM);
    game.slide(Direction::Left);
    assert!(game.has_won(), "the two met");
    assert!(!game.is_over(), "and the board is not finished");

    let mut app = listing();
    app.page = Page::Playing(Game::TwentyFortyEight, game);

    let mut ui = screen(&app, TALL);
    assert!(ui.find("You made 2048 — carry on").is_ok());
    assert!(ui.find("Slide with the arrows below").is_err());
}

/// The four keys this app listens for are the four directions, and every other key is nothing.
#[test]
fn the_four_arrows_are_the_four_directions() {
    assert_eq!(direction(Key::Named(Named::ArrowLeft)), Some(Direction::Left));
    assert_eq!(direction(Key::Named(Named::ArrowUp)), Some(Direction::Up));
    assert_eq!(
        direction(Key::Named(Named::ArrowDown)),
        Some(Direction::Down)
    );
    assert_eq!(
        direction(Key::Named(Named::ArrowRight)),
        Some(Direction::Right)
    );

    // The keys that are next to the arrows and mean nothing here: on a board with a pad, `w` is a way
    // back to the launcher, and a page that swallowed it would be a page somebody could not leave.
    assert_eq!(direction(Key::Named(Named::Enter)), None);
    assert_eq!(direction(Key::Character("w")), None);
    assert_eq!(direction(Key::Character("q")), None);
}
