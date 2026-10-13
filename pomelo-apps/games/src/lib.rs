//! The games on this box: a list of them, and the one that is being played.
//!
//! # Why an app has a list in it
//!
//! The launcher's grid is this box's list of *applications*, and a game that opened straight onto its
//! own board would be an application that happened to be a game — the second game would then have to be
//! a second tile on the launcher's grid, which is a decision about the launcher's grid made by
//! accident. So there is a list, and it has two entries on it: adding a game is adding a [`Game`], and
//! the compiler then names every place that has to know about it, which is what [`Played`] is the
//! shape of.
//!
//! # Two screens, and one of them is a game
//!
//! [`Page`] is the whole of this app's state machine: the list, or a game in progress. There is no
//! third thing to be — a game is not paused, and there is nothing to fetch — so nothing here is a
//! flag beside a `Option`, and nothing needs to be kept in step with anything else.
//!
//! # What this app asks the board for
//!
//! A seed, and — while a snake is on the panel — the tilt of the thing. See [`Games::seed`]: the game
//! has to be a different game every time somebody opens it, and a board's uptime is the only value
//! this HAL has that is not the same on two runs. See [`crate::tilt`] for the steering: a snake moves
//! on its own, so the app is poked once a frame and reads the accelerometer off the board itself,
//! which is the one subscription here that is not the desktop's alone.

mod game;
mod grid;
mod random;
mod snake;
pub mod style;
#[cfg(test)]
mod tests;
mod tilt;
mod ui;

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::theme::Palette;
use iced::{Element, Subscription, Theme};

use pomelo_hal::Board;
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

pub use game::TwentyFortyEight;
pub use grid::{Direction, Grid, Move, WINNING};
pub use snake::Snake;
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
    /// Snake: a body that grows and a board it must not run into.
    Snake,
}

impl Game {
    /// The games there are, in the order the list draws them.
    pub const ALL: [Game; 2] = [Game::TwentyFortyEight, Game::Snake];

    /// What the game is called.
    ///
    /// `2048` in both languages, because a game's name is its own: the words this app translates are
    /// the ones *about* it, which is what [`Words::about`] is.
    pub fn name(self) -> &'static str {
        match self {
            Game::TwentyFortyEight => "2048",
            Game::Snake => "Snake",
        }
    }

    /// The glyph on its row, and in the header while it is being played.
    pub fn icon(self) -> Icon {
        match self {
            // A grid of squares, which is the board it draws. A joystick would say "this is a game"
            // and the row's name already says that: what the glyph has to carry is *which* one.
            Game::TwentyFortyEight => Icon::GRID_VIEW,
            // A hand tilting, which is how this one is steered as much as an arrow is.
            Game::Snake => Icon::GESTURE,
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
    /// A direction, which is the whole of either game's input: a board has no keys, and the four
    /// arrows of the pad, the four keys, and a tilt all arrive as one of the four.
    Steer(Direction),
    /// A new board for the game that is open.
    Restart,
    /// A frame, which is the clock a snake moves on and when the board is read for a tilt.
    ///
    /// The only app in this project that is poked by frames rather than by the launcher's beat, and
    /// it has to be: a snake steps ten times a second and the beat is once. What arrives is the
    /// moment, because how long ago the last step was is the whole question — see [`Games::tick`].
    Tick(Instant),
}

/// Which of the app's two screens is on the panel, and what that screen draws from.
///
/// The game and its state travel in the same value, so there is no `Option` beside a `bool` and no
/// way to be playing nothing. It is private because a page is what the app *is*, not something a
/// caller sets: the way to open a game is [`Message::Open`].
enum Page {
    /// The list of games.
    List,
    /// A game in progress, and which one it is.
    Playing(Played),
}

/// A game in progress: which one, and how it is going.
///
/// The game and its state in a single value, which is [`Page`]'s note taken one step further: two
/// fields — a [`Game`] and a state — would be a pair that has to agree, and a page of snake holding
/// 2048's board would be a page that drew, and played, the wrong game. Here it cannot be built.
enum Played {
    /// 2048, and the board it is being played on.
    Tiles(TwentyFortyEight),
    /// Snake, and the body it is being played with.
    Snake(Snake),
}

impl Played {
    /// Which game this is a game of, for the header that names it.
    fn game(&self) -> Game {
        match self {
            Played::Tiles(_) => Game::TwentyFortyEight,
            Played::Snake(_) => Game::Snake,
        }
    }
}

/// The games app.
pub struct Games {
    board: Arc<Board>,
    pub(crate) page: Page,
    pub(crate) preferences: SystemPreferences,
    /// When the snake last took a step.
    ///
    /// The frame that arrives is the clock, and *how long since* is the question — see [`Games::tick`].
    /// It is reset when a game opens or restarts, so a snake never owes a step the moment it appears.
    /// 2048 never reads it: it moves when somebody moves it, and nothing on its page has a pace.
    pub(crate) stepped: Instant,
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
            stepped: Instant::now(),
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

    /// The 2048 game being played, if that is the one that is.
    pub fn tiles(&self) -> Option<&TwentyFortyEight> {
        match &self.page {
            Page::Playing(Played::Tiles(game)) => Some(game),
            _ => None,
        }
    }

    /// The snake being played, if that is the one that is.
    pub fn snake(&self) -> Option<&Snake> {
        match &self.page {
            Page::Playing(Played::Snake(snake)) => Some(snake),
            _ => None,
        }
    }

    /// Whether a game is on the panel, rather than the list.
    pub fn playing(&self) -> bool {
        matches!(self.page, Page::Playing(_))
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Open(game) => self.open(game),
            Message::List => self.page = Page::List,
            Message::Restart => self.restart(),
            // A direction on the list is not something anybody can send — the pad is only drawn while
            // a game is open — and it is ignored rather than a panic, because a message is a fact
            // about what happened and not a promise about when.
            Message::Steer(direction) => {
                if let Page::Playing(played) = &mut self.page {
                    match played {
                        Played::Tiles(game) => game.slide(direction),
                        Played::Snake(snake) => snake.turn(direction),
                    }
                }
            }
            Message::Tick(now) => self.tick(now),
        }
    }

    /// Opens a game: a new board of it, with a seed of its own.
    fn open(&mut self, game: Game) {
        let seed = self.seed();

        // The match is what a second game changes. A `Game` that could not say what to play would be a
        // row that opened nothing.
        let played = match game {
            Game::TwentyFortyEight => Played::Tiles(TwentyFortyEight::seeded(seed)),
            Game::Snake => Played::Snake(Snake::seeded(seed)),
        };

        self.stepped = Instant::now();
        self.page = Page::Playing(played);
    }

    /// Starts the game that is open again, from nothing.
    ///
    /// A new board rather than a cleared one: the seed comes from the clock, and a restart that dealt
    /// the same two tiles would be a restart that did not happen. On the list this does nothing, for
    /// the reason a direction there does.
    fn restart(&mut self) {
        // The seed is read before the page is borrowed mutably, because it comes off the board and
        // both of those are `self`.
        let seed = self.seed();

        let Page::Playing(played) = &mut self.page else {
            return;
        };

        // The game is copied out before the state is replaced: `Played::game` borrows the value that
        // is about to be assigned, and a `Game` is `Copy` so the borrow ends with the statement.
        let game = played.game();
        *played = match game {
            Game::TwentyFortyEight => Played::Tiles(TwentyFortyEight::seeded(seed)),
            Game::Snake => Played::Snake(Snake::seeded(seed)),
        };

        self.stepped = Instant::now();
    }

    /// A frame: steer the snake by the tilt, and step it when its step has come round.
    ///
    /// Nothing here happens on the list, or while 2048 is up: this app is poked by frames only because
    /// a snake has a clock, and the frame is that clock. The reading is taken only when there is a
    /// snake to steer it, because reading a peripheral sixty times a second to steer nothing is the
    /// one thing on this page that is not free.
    fn tick(&mut self, now: Instant) {
        if !matches!(self.page, Page::Playing(Played::Snake(_))) {
            return;
        }

        // The board is read *before* the page is borrowed, because both of those are `self`.
        let tilt = self.tilt();

        let Page::Playing(Played::Snake(snake)) = &mut self.page else {
            return;
        };

        // `None` is a board lying flat, and it is not a direction: the snake carries on the way it was
        // going rather than being steered by whatever happened last.
        if let Some(direction) = tilt {
            snake.turn(direction);
        }

        if now.saturating_duration_since(self.stepped) >= Duration::from_millis(snake::pace(snake.length()))
        {
            self.stepped = now;
            snake.advance();
        }
    }

    /// Which way the board is tipped, if it is tipped far enough to mean a direction.
    ///
    /// The reading is the HAL's — see [`crate::tilt`] for what it does with the units — and a board
    /// that cannot answer, or one lying flat, gives `None`, which is the same answer as a board not
    /// tipped far enough: this app steers by the pad either way, and a tilt is a second way in.
    fn tilt(&self) -> Option<Direction> {
        tilt::direction(self.board.imu().read_accel().ok()?)
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

    /// What is listened for, and only while a game is open.
    ///
    /// The keyboard is the desktop's way in — `as_ref` rather than matching the key itself, because a
    /// `Key` is generic over the string it may hold and the four keys this app cares about are the
    /// four with nothing in them, which is the shape the launcher's own keyboard subscription uses.
    /// A board has no keyboard: its way in is the pad the page draws.
    ///
    /// A snake adds frames, and it is the only thing in this app that does. The launcher forwards
    /// this subscription to a hosted app while it is the screen — see `Launcher::subscription` — and
    /// this platform's frames are what a snake steps on and when the board is read for a tilt. 2048
    /// does not want them: it moves when somebody moves it, and a page that repainted sixty times a
    /// second to draw the same board would be sixty repaints for nothing.
    pub fn subscription(&self) -> Subscription<Message> {
        match &self.page {
            // Nothing to listen for on the list: a list does not move.
            Page::List => Subscription::none(),
            // Arrows only: the desktop's way in, and nothing on the board.
            Page::Playing(Played::Tiles(_)) => arrows(),
            // Arrows and a clock, and the clock stops the moment the snake does. A game that is over
            // draws the same board until somebody starts a new one, and a page that repainted sixty
            // times a second to draw it again would be sixty repaints for nothing — which is the same
            // reason the other game never asked for a clock at all.
            Page::Playing(Played::Snake(snake)) => {
                if snake.is_over() {
                    arrows()
                } else {
                    Subscription::batch([arrows(), iced::window::frames().map(Message::Tick)])
                }
            }
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

/// The four arrow keys, as the four directions.
///
/// The keyboard half of [`Games::subscription`], named so that the two games can share it: they are
/// steered by the same four words, and a second copy of this closure is how they would stop being.
fn arrows() -> Subscription<Message> {
    iced::keyboard::listen().filter_map(|event| {
        let iced::keyboard::Event::KeyPressed { key, .. } = event else {
            return None;
        };

        direction(key.as_ref()).map(Message::Steer)
    })
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
    /// 2048's caption: which way the board is played.
    pub(crate) slide: &'static str,
    /// Snake's caption: the two ways to steer it.
    pub(crate) steer: &'static str,
    pub(crate) restart: &'static str,
    /// 2048's line when there is nothing left to do.
    pub(crate) over: &'static str,
    /// Snake's line when it has run into something.
    pub(crate) hit: &'static str,
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
                steer: "倾斜机身或按方向键转向",
                restart: "新的一局",
                over: "无处可动，开一局新的",
                hit: "撞上了，开一局新的",
                won: "拼出了 2048，可以继续",
            },
            Language::English => Self {
                language,
                title: "Games",
                score: "Score",
                slide: "Slide with the arrows below",
                steer: "Tilt the box or use the arrows to turn",
                restart: "New game",
                over: "No moves left — start a new game",
                hit: "You ran into something — start a new game",
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
            (Language::Chinese, Game::Snake) => "吃苹果变长，别撞到墙或自己",
            (_, Game::Snake) => "Eat apples and grow — don't hit the wall or yourself",
        }
    }
}
