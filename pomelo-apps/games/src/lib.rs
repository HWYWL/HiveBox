//! The games on this box: a list of them, and the one that is being played.
//!
//! # Why an app of one game has a list in it
//!
//! The launcher's grid is this box's list of *applications*, and a game that opened straight onto its
//! own board would be an application that happened to be a game — the second game would then have to
//! be a second tile on the launcher's grid, which is a decision about the launcher's grid made by
//! accident. So there is one entry in the list today and the list is a list: adding a game is adding a
//! [`Game`], and the compiler then names the three places that have to know about it.
//!
//! # Two screens, and one of them is a game
//!
//! [`Page`] is the whole of this app's state machine: the list, or a game in progress. There is no
//! third thing to be — a game is not paused, and there is nothing to fetch — so nothing here is a
//! flag beside a `Option`, and nothing needs to be kept in step with anything else.
//!
//! # The one thing this app asks the board for
//!
//! A seed. See [`Games::seed`]: the game has to be a different game every time somebody opens it, and
//! a board's uptime is the only value this HAL has that is not the same on two runs. No request, no
//! peripheral, no clock to draw — which is why this is the one page in the project whose subscription
//! is only ever the desktop's.

mod game;
mod grid;
mod random;
pub mod style;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;

use iced::theme::Palette;
use iced::{Element, Subscription, Theme};

use pomelo_hal::Board;
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

pub use game::TwentyFortyEight;
pub use grid::{Direction, Grid, Move, WINNING};
pub use style::Sizes;

/// Which game a row in the list opens.
///
/// An enum rather than an index into [`Game::ALL`], so that the two places which have to know what a
/// game *is* — the row that names it and the page that plays it — are exhaustive matches the compiler
/// checks. A second game is a second variant, and it will not compile until both of them say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    /// 2048: sixteen cells, four slides, and the number the game is named for.
    TwentyFortyEight,
}

impl Game {
    /// The games there are, in the order the list draws them.
    pub const ALL: [Game; 1] = [Game::TwentyFortyEight];

    /// What the game is called.
    ///
    /// `2048` in both languages, because a game's name is its own: the words this app translates are
    /// the ones *about* it, which is what [`Words::about`] is.
    pub fn name(self) -> &'static str {
        match self {
            Game::TwentyFortyEight => "2048",
        }
    }

    /// The glyph on its row, and in the header while it is being played.
    pub fn icon(self) -> Icon {
        match self {
            // A grid of squares, which is the board it draws. A joystick would say "this is a game"
            // and the row's name already says that: what the glyph has to carry is *which* one.
            Game::TwentyFortyEight => Icon::GRID_VIEW,
        }
    }
}

/// What the app reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A row in the list was pressed.
    Open(Game),
    /// The way back to the list.
    List,
    /// A slide, which is the whole of the game's input: a board has no keys, and the four arrows of
    /// the pad are the four directions.
    Slide(Direction),
    /// A new board for the game that is open.
    Restart,
}

/// Which of the app's two screens is on the panel, and what that screen draws from.
///
/// The game and its state travel in the same value, so there is no `Option` beside a `bool` and no
/// way to be playing nothing. It is private because a page is what the app *is*, not something a
/// caller sets: the way to open a game is [`Message::Open`].
enum Page {
    /// The list of games.
    List,
    /// A game in progress: which one, and how it is going.
    Playing(Game, TwentyFortyEight),
}

/// The games app.
pub struct Games {
    board: Arc<Board>,
    pub(crate) page: Page,
    pub(crate) preferences: SystemPreferences,
}

impl Games {
    /// The list of games, which is where the app opens.
    ///
    /// Nothing is started here, unlike the Wi-Fi and weather pages: the list is a constant and a game
    /// begins when somebody picks one, so opening the app costs nothing and draws immediately.
    pub fn new(board: Arc<Board>) -> Self {
        Self {
            board,
            page: Page::List,
            preferences: SystemPreferences::default(),
        }
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The game being played, if one is.
    pub fn game(&self) -> Option<&TwentyFortyEight> {
        match &self.page {
            Page::Playing(_, played) => Some(played),
            Page::List => None,
        }
    }

    /// Whether a game is on the panel, rather than the list.
    pub fn playing(&self) -> bool {
        self.game().is_some()
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Open(game) => self.open(game),
            Message::List => self.page = Page::List,
            Message::Restart => self.restart(),
            // A slide on the list is not something anybody can send — the pad is only drawn while a
            // game is open — and it is ignored rather than a panic, because a message is a fact about
            // what happened and not a promise about when.
            Message::Slide(direction) => {
                if let Page::Playing(_, played) = &mut self.page {
                    played.slide(direction);
                }
            }
        }
    }

    /// Opens a game: a new board of it, with a seed of its own.
    fn open(&mut self, game: Game) {
        let seed = self.seed();

        // The match is what a second game changes. A `Game` that could not say what to play would be a
        // row that opened nothing.
        let played = match game {
            Game::TwentyFortyEight => TwentyFortyEight::seeded(seed),
        };

        self.page = Page::Playing(game, played);
    }

    /// Starts the game that is open again, from nothing.
    ///
    /// A new board rather than a cleared one: the seed comes from the clock, and a restart that dealt
    /// the same two tiles would be a restart that did not happen. On the list this does nothing, for
    /// the reason a slide there does.
    fn restart(&mut self) {
        // The seed is read before the page is borrowed mutably, because it comes off the board and
        // both of those are `self`.
        let seed = self.seed();

        let Page::Playing(game, played) = &mut self.page else {
            return;
        };

        *played = match *game {
            Game::TwentyFortyEight => TwentyFortyEight::seeded(seed),
        };
    }

    /// Something different every time a game starts.
    ///
    /// How long the board has been up, in nanoseconds. It is the only value this HAL has that is not
    /// the same on two runs, and it is read *here* rather than inside [`crate::random`] because a
    /// generator that could only be seeded by asking a board would be a generator no test could pin
    /// down. A board that cannot say how long it has been up gets one fixed game rather than no game at
    /// all: a playable board is worth more than a peculiar seed.
    fn seed(&self) -> u64 {
        self.board
            .system()
            .uptime()
            .map(|up| up.as_nanos() as u64)
            .unwrap_or(0x5EED_2F_2048)
    }

    /// The keyboard, and only while a game is open.
    ///
    /// The desktop's way in. A board has no keyboard — its way in is the pad the page draws — and the
    /// launcher's own keyboard subscription is for "go back" and "close", which it does not forward to
    /// a hosted app, so a board never sees an arrow.
    pub fn subscription(&self) -> Subscription<Message> {
        match self.page {
            // Nothing to listen for on the list: a list does not move.
            Page::List => Subscription::none(),
            Page::Playing(..) => iced::keyboard::listen().filter_map(|event| {
                let iced::keyboard::Event::KeyPressed { key, .. } = event else {
                    return None;
                };

                // `as_ref` rather than matching the key itself: a `Key` is generic over the string it
                // may hold, and the four keys this app cares about are the four with nothing in them.
                // This is the shape the launcher's own keyboard subscription uses.
                direction(key.as_ref()).map(Message::Slide)
            }),
        }
    }

    /// The theme, by the system's preference.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: style::surface(ThemeMode::Dark),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::surface(ThemeMode::Light),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// The page.
    pub fn view(&self) -> Element<'_, Message> {
        ui::page(self)
    }
}

/// An arrow key as a direction, and every other key as nothing at all.
///
/// Here rather than in `main` because what an arrow *means* is this app's business: a game is the only
/// thing in this project that knows a left arrow is the key you play with. It takes the key rather
/// than the event so that the four keys can be tested without building a keyboard event, which is
/// eleven fields of things no test has an opinion about.
fn direction(key: iced::keyboard::Key<&str>) -> Option<Direction> {
    use iced::keyboard::key::Named;
    use iced::keyboard::Key;

    match key {
        Key::Named(Named::ArrowLeft) => Some(Direction::Left),
        Key::Named(Named::ArrowUp) => Some(Direction::Up),
        Key::Named(Named::ArrowDown) => Some(Direction::Down),
        Key::Named(Named::ArrowRight) => Some(Direction::Right),
        _ => None,
    }
}

/// The words on the pages, in the language the system is set to.
///
/// One struct with both languages in it — the shape the rest of this project's pages use — so that a
/// page never has a sentence with a language of its own, and adding a word is adding a field the
/// compiler will insist be filled in twice.
pub(crate) struct Words {
    pub(crate) language: Language,
    pub(crate) title: &'static str,
    pub(crate) score: &'static str,
    pub(crate) slide: &'static str,
    pub(crate) restart: &'static str,
    pub(crate) over: &'static str,
    pub(crate) won: &'static str,
}

impl Words {
    /// The app's words, for `language`.
    pub(crate) fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language,
                title: "游戏",
                score: "得分",
                slide: "用下面的方向键滑动",
                restart: "新的一局",
                over: "无处可动，开一局新的",
                won: "拼出了 2048，可以继续",
            },
            Language::English => Self {
                language,
                title: "Games",
                score: "Score",
                slide: "Slide with the arrows below",
                restart: "New game",
                over: "No moves left — start a new game",
                won: "You made 2048 — carry on",
            },
        }
    }

    /// What a game is, as the line under its name in the list.
    ///
    /// The one sentence about a game there is, and it is a sentence about the *rules*: a list of names
    /// is a list somebody has to have heard of, and this box is on a shelf in a room with people who
    /// have not played 2048 in it.
    pub(crate) fn about(&self, game: Game) -> &'static str {
        match (self.language, game) {
            (Language::Chinese, Game::TwentyFortyEight) => "滑动合并数字，拼出 2048",
            (_, Game::TwentyFortyEight) => "Slide the tiles together and make 2048",
        }
    }
}
