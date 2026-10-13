//! Snake's rules: sixteen by sixteen cells, a heading, and an apple that makes it longer.
//!
//! Everything here is arithmetic on cells and nothing here knows what a pixel is or that a page
//! exists — the same split as [`crate::grid`], and for the same reason: "moving into the cell the
//! tail is leaving is legal" is a property of this file, and the way to check it is to step a snake
//! somebody wrote down rather than to press an arrow key and watch a window.
//!
//! # The rule that is easy to get wrong
//!
//! One turn is kept and not a queue of them. Two turns in the same step — `Up` and then `Left`
//! while the snake is heading `Right` — are two legal requests whose *composition* is a reversal,
//! and a snake that applied both would move a full step backwards into its own neck. [`Snake::turn`]
//! refuses the reversal and remembers one direction; [`Snake::advance`] applies it once.

use std::collections::VecDeque;

use crate::grid::Direction;
use crate::random::Random;

/// The side of the board, in cells.
///
/// The game's own sixteen, and the number [`crate::style::SNAKE_CELL`] is the board divided by: the
/// two games play on the same 304 px square.
pub const SIDE: usize = 16;

/// Every cell on the board.
pub const CELLS: usize = SIDE * SIDE;

/// The length a snake starts at, in cells.
const START: u64 = 3;

/// How long a step takes at the start, and how quick it gets.
const SLOWEST_MS: u64 = 220;
const FASTEST_MS: u64 = 90;

/// What each apple is worth in time: the snake crosses a cell this much sooner per apple eaten.
const QUICKER_MS: u64 = 7;

/// How long one step takes, for a snake this long.
///
/// The pace is a feel rather than arithmetic, and these are the three numbers it is: a snake that
/// never sped up would be a game that only got longer, and one that sped up without a floor would
/// reach a step per frame and be over before anybody could steer it. Around twenty apples in, this
/// is as quick as it gets.
pub fn pace(length: usize) -> u64 {
    SLOWEST_MS
        .saturating_sub((length as u64).saturating_sub(START) * QUICKER_MS)
        .max(FASTEST_MS)
}

/// One cell of the board: a row and a column, both inside the side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

impl Cell {
    /// The cell one step this way, or `None` when that step is off the board.
    ///
    /// The board's edge is this function and nothing else: the wall is not a rule about walls, it is
    /// a step that lands nowhere.
    fn stepped(self, direction: Direction) -> Option<Self> {
        match direction {
            Direction::Up => self.row.checked_sub(1).map(|row| Self { row, ..self }),
            Direction::Down => (self.row + 1 < SIDE).then_some(Self { row: self.row + 1, ..self }),
            Direction::Left => self.col.checked_sub(1).map(|col| Self { col, ..self }),
            Direction::Right => (self.col + 1 < SIDE).then_some(Self { col: self.col + 1, ..self }),
        }
    }
}

/// A game of snake, whole.
#[derive(Debug, Clone)]
pub struct Snake {
    /// The cells it fills, head first.
    body: VecDeque<Cell>,
    /// The way it is moving now.
    heading: Direction,
    /// The way it has been asked to move at the next step, when that is not a reversal.
    asked: Option<Direction>,
    /// The apple, always on a cell the snake is not on.
    food: Cell,
    /// How many apples it has eaten.
    score: u32,
    /// Where the next apple goes.
    random: Random,
    /// Whether it is over: it hit a wall, or itself, or ate the whole board.
    over: bool,
}

impl Snake {
    /// A new snake: three cells in the middle of the board, heading right, one apple somewhere else.
    ///
    /// `seed` is the app's clock reading; see [`crate::random`] for why there is one.
    pub fn seeded(seed: u64) -> Self {
        let middle = SIDE / 2;
        let body: VecDeque<Cell> = (0..START as usize)
            .map(|behind| Cell {
                row: middle,
                col: middle - behind,
            })
            .collect();

        let mut random = Random::seeded(seed);
        let food = place(&body, &mut random)
            .expect("three cells of a 256-cell board leave somewhere to put an apple");

        Self {
            body,
            heading: Direction::Right,
            asked: None,
            food,
            score: 0,
            random,
            over: false,
        }
    }

    /// Ask the snake to head this way at its next step.
    ///
    /// A reversal is refused, and refused here rather than where the step happens: the heading
    /// cannot change in between, so the request that was legal when it was made is not dropped by
    /// one that arrives after it and is not. Asking for the way it is already going is not a
    /// reversal, and asking twice in one step is the second request — see this module's note on why
    /// the turns are not queued.
    pub fn turn(&mut self, direction: Direction) {
        if direction != self.heading.opposite() {
            self.asked = Some(direction);
        }
    }

    /// Move one step.
    ///
    /// Returns whether anything happened: `true` for the step that ends the game, and `false` only
    /// when it is already over. Nothing moves into a wall — the head stays where it was and the game
    /// ends, so the board a person is looking at is the last one they steered.
    pub fn advance(&mut self) -> bool {
        if self.over {
            return false;
        }

        if let Some(asked) = self.asked.take() {
            self.heading = asked;
        }

        let Some(ahead) = self.head().stepped(self.heading) else {
            self.over = true;
            return true;
        };

        let eating = ahead == self.food;
        // The cell the tail is leaving is not the snake's for the step that is starting, so moving
        // into it is legal. This is the difference between a game that can be steered tightly and
        // one that kills you for following your own tail around a corner.
        let vacated = self.body.back().copied() == Some(ahead);

        if self.body.contains(&ahead) && !vacated {
            self.over = true;
            return true;
        }

        self.body.push_front(ahead);

        if eating {
            self.score += 1;

            match place(&self.body, &mut self.random) {
                Some(food) => self.food = food,
                // Every cell is the snake's: it has eaten the board.
                None => self.over = true,
            }
        } else {
            self.body.pop_back();
        }

        true
    }

    /// The cells it fills, head first.
    pub fn body(&self) -> impl Iterator<Item = Cell> + '_ {
        self.body.iter().copied()
    }

    /// The cell its head is in: the first of [`Snake::body`], named because it is what the next
    /// step is measured from.
    pub fn head(&self) -> Cell {
        // A snake is never shorter than its head: it is built three cells long and every step after
        // that is a push before a pop.
        *self.body.front().expect("a snake always has a head")
    }

    /// The apple.
    pub fn food(&self) -> Cell {
        self.food
    }

    /// The way it is moving.
    pub fn heading(&self) -> Direction {
        self.heading
    }

    /// How many apples it has eaten.
    pub fn score(&self) -> u32 {
        self.score
    }

    /// How many cells long it is.
    pub fn length(&self) -> usize {
        self.body.len()
    }

    /// Whether it has hit a wall or itself, or eaten the board.
    pub fn is_over(&self) -> bool {
        self.over
    }
}

/// The cell the next apple goes in: the `n`th cell the snake is not on.
///
/// Counted rather than rejection-sampled. A loop that draws a cell and draws again when the snake is
/// on it has to either bound its retries or put the apple under the snake, and neither of those is a
/// board. `None` is a snake on all 256 cells, which is the one ending this game has that is not a
/// mistake.
fn place(body: &VecDeque<Cell>, random: &mut Random) -> Option<Cell> {
    let free = CELLS - body.len();

    if free == 0 {
        return None;
    }

    (0..SIDE)
        .flat_map(|row| (0..SIDE).map(move |col| Cell { row, col }))
        .filter(|cell| !body.contains(cell))
        .nth(random.below(free))
}

#[cfg(test)]
impl Snake {
    /// A snake written down by hand: these cells, head first, moving this way.
    ///
    /// The way to test a position that takes a person a minute of steering to reach — a snake
    /// wrapped around a corner, a snake with its head beside its own tail. Kept inside this crate,
    /// like [`crate::grid::Grid::from_cells`] and for the same reason.
    pub(crate) fn from_body(body: &[Cell], heading: Direction) -> Self {
        let body: VecDeque<Cell> = body.iter().copied().collect();
        let mut random = Random::seeded(1);
        let food = place(&body, &mut random).expect("a hand-written snake leaves room for an apple");

        Self {
            body,
            heading,
            asked: None,
            food,
            score: 0,
            random,
            over: false,
        }
    }

    /// Put the apple here, so that a step can be aimed at it.
    pub(crate) fn put_food(&mut self, cell: Cell) {
        self.food = cell;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// A cell, written the short way so a whole snake fits on one line.
    fn cell(row: usize, col: usize) -> Cell {
        Cell { row, col }
    }

    /// A snake three cells long in the middle of the board, heading right, with its apple put out of
    /// the way of the steps the tests below take — so that a length is a length and not an accident
    /// of where a seed put an apple.
    fn straight() -> Snake {
        let mut snake =
            Snake::from_body(&[cell(8, 8), cell(8, 7), cell(8, 6)], Direction::Right);
        snake.put_food(cell(0, SIDE - 1));

        snake
    }

    /// Every cell of the board, for the one ending that is not a mistake.
    fn everywhere() -> VecDeque<Cell> {
        (0..SIDE)
            .flat_map(|row| (0..SIDE).map(move |col| cell(row, col)))
            .collect()
    }

    /// A new snake is three cells long, on one row in the middle, heading right, and not on its own
    /// apple.
    #[test]
    fn a_new_snake_starts_in_the_middle_and_short() {
        let snake = Snake::seeded(1);

        assert_eq!(snake.length(), 3);
        assert_eq!(snake.heading(), Direction::Right, "it starts going somewhere");
        assert_eq!(snake.score(), 0, "and has eaten nothing");
        assert!(!snake.is_over());
        assert!(
            snake.body().all(|cell| cell.row == SIDE / 2),
            "a new snake is a straight line in the middle"
        );
        assert!(
            !snake.body().any(|cell| cell == snake.food()),
            "and the apple is never under it"
        );
    }

    /// A step moves the head a cell and leaves the cell behind, so the length does not change.
    #[test]
    fn a_step_moves_the_head_and_keeps_the_length() {
        let mut snake = straight();

        assert!(snake.advance(), "a step nothing stops is a step");

        assert_eq!(snake.head(), cell(8, 9), "the head moved one cell");
        assert_eq!(snake.length(), 3, "and the tail followed");
        assert!(!snake.is_over());
    }

    /// The way it is going is not a way it can be asked to go.
    #[test]
    fn a_reversal_is_refused() {
        let mut snake = straight();

        snake.turn(Direction::Left);
        snake.advance();

        assert_eq!(snake.heading(), Direction::Right, "the ask was refused");
        assert_eq!(
            snake.head(),
            cell(8, 9),
            "and it went on the way it had been going"
        );
    }

    /// Two turns in one step are one turn: the second — `Left` while heading `Right` — is the
    /// reversal the first, `Up`, would have committed if both had been kept.
    #[test]
    fn a_second_turn_in_one_step_does_not_make_a_reversal() {
        let mut snake = straight();

        snake.turn(Direction::Up);
        snake.turn(Direction::Left);
        snake.advance();

        assert_eq!(snake.heading(), Direction::Up, "the one legal turn was taken");
        assert_eq!(
            snake.head(),
            cell(7, 8),
            "and the snake went up rather than back into itself"
        );
        assert_eq!(snake.length(), 3, "and a turn is not an apple");
    }

    /// The cell the tail is leaving is not the snake's for the step that is starting, so a snake can
    /// be steered tightly enough to follow its own tail around.
    #[test]
    fn moving_into_the_cell_the_tail_is_leaving_is_legal() {
        // A ring of four cells: the head at the top left, the tail at the top right, and the head
        // heading into the tail.
        let mut snake = Snake::from_body(
            &[cell(5, 5), cell(6, 5), cell(6, 6), cell(5, 6)],
            Direction::Right,
        );
        snake.put_food(cell(15, 0));

        assert!(snake.advance());
        assert!(!snake.is_over(), "the tail's cell was free to move into");
        assert_eq!(snake.head(), cell(5, 6), "and the head went there");
        assert_eq!(snake.length(), 4, "with nothing grown and nothing dropped");
    }

    /// Running into a cell the snake is still in is the end of it.
    #[test]
    fn running_into_itself_is_the_end() {
        // The same ring, but the head is heading into its own neck rather than into the tail.
        let mut snake = Snake::from_body(
            &[cell(5, 5), cell(5, 6), cell(6, 6), cell(6, 5)],
            Direction::Right,
        );

        assert!(snake.advance());
        assert!(snake.is_over(), "the neck is not the tail");
        assert_eq!(
            snake.head(),
            cell(5, 5),
            "and it stops: the last board is the one a person steered"
        );
    }

    /// The wall is a step that lands nowhere: the game ends and the snake is still on the board.
    #[test]
    fn a_step_off_the_board_ends_it_and_moves_nothing() {
        let mut snake = Snake::from_body(
            &[cell(8, SIDE - 1), cell(8, SIDE - 2)],
            Direction::Right,
        );

        assert!(snake.advance());
        assert!(snake.is_over(), "there is no cell past the wall");
        assert_eq!(snake.head(), cell(8, SIDE - 1), "so nothing moved");
        assert_eq!(snake.length(), 2, "and nothing was dropped");
    }

    /// An apple is a point, a cell of length, and somewhere else to go next.
    #[test]
    fn an_apple_makes_it_longer_and_is_worth_a_point() {
        let mut snake = straight();
        snake.put_food(cell(8, 9));

        snake.advance();

        assert_eq!(snake.score(), 1, "an apple is a point");
        assert_eq!(snake.length(), 4, "and the tail did not follow this step");
        assert_ne!(snake.food(), cell(8, 9), "and there is a new apple");
        assert!(
            !snake.body().any(|cell| cell == snake.food()),
            "on a cell the snake is not on"
        );
    }

    /// A game that is over stays over: the steps after the last one are not steps.
    #[test]
    fn a_finished_game_does_not_move_again() {
        let mut snake = Snake::from_body(&[cell(0, 0), cell(0, 1)], Direction::Up);

        assert!(snake.advance(), "the wall is the last step");
        assert!(!snake.advance(), "and there are no steps after it");
        assert_eq!(snake.head(), cell(0, 0));
    }

    /// An apple goes on a cell the snake is not on, and there is nowhere to put one only when the
    /// snake is the whole board.
    #[test]
    fn an_apple_goes_on_a_free_cell_or_nowhere() {
        // Many seeds and many steps, because this is the one place a game draws a number: a placement
        // that could land under the snake would be a bug that showed up once in a long game.
        for seed in 0..64 {
            let mut snake = Snake::seeded(seed);
            let mut random = Random::seeded(seed);

            for _ in 0..32 {
                let food = place(&snake.body, &mut random).expect("the board is nowhere near full");

                assert!(!snake.body.contains(&food), "an apple is not under the snake");
                assert!(food.row < SIDE && food.col < SIDE, "and it is on the board");

                snake.put_food(food);
                snake.advance();
            }
        }

        assert_eq!(
            place(&everywhere(), &mut Random::seeded(1)),
            None,
            "a snake that is the whole board has eaten it"
        );
    }

    /// The pace starts at its slowest, quickens a little per apple, and stops quickening.
    #[test]
    fn the_pace_quickens_and_then_stops() {
        assert_eq!(pace(START as usize), SLOWEST_MS, "a new snake is the slowest");
        assert_eq!(
            pace(START as usize + 1),
            SLOWEST_MS - QUICKER_MS,
            "and an apple is worth a little speed"
        );
        assert_eq!(pace(1_000), FASTEST_MS, "and there is a floor under it");

        // It never gets slower as it gets longer, and it never gets quicker than the floor.
        assert!(
            (START as usize..64).all(|length| pace(length) >= pace(length + 1)),
            "the pace only ever quickens"
        );
    }
}
