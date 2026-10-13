//! A game of 2048 in progress: the board, the score, and the generator that fills it.
//!
//! The rules are in [`crate::grid`] and this is the state around them — the three things that are
//! about *this* game rather than about the game, and the two operations a person can perform on it.
//! It is here rather than in the page because it is the same game on either platform: a window and a
//! panel run these lines, and only the drawing differs.

use crate::grid::{Direction, Grid, Move, WINNING};
use crate::random::Random;

/// The tiles a game starts with. Two, which is the game's own opening and the reason the first move is
/// a real decision rather than a formality.
const OPENING_TILES: usize = 2;

/// A game of 2048.
#[derive(Debug, Clone)]
pub struct TwentyFortyEight {
    grid: Grid,
    score: u32,
    random: Random,
    /// Whether this board ever held [`WINNING`], which is a fact about the *past*: a game that reached
    /// 2048 and went on playing has a board that no longer has that tile on it, and the line saying so
    /// should not vanish when the tile merges away.
    won: bool,
}

impl TwentyFortyEight {
    /// A new game, with `seed` deciding how it goes.
    ///
    /// Seeded rather than random-from-nowhere so that a test can play a fixed game and so that two
    /// boards in the same process are independent: a generator shared between games would make the
    /// second game depend on how the first one ended.
    pub fn seeded(seed: u64) -> Self {
        let mut game = Self {
            grid: Grid::new(),
            score: 0,
            random: Random::seeded(seed),
            won: false,
        };

        for _ in 0..OPENING_TILES {
            // A board always has room for two tiles, so the answer is not worth a branch: this is the
            // one `spawn` in the app whose `false` cannot happen.
            let _ = game.grid.spawn(&mut game.random);
        }

        game
    }

    /// A game with this board on it, for the tests of what a page draws when a board is finished.
    ///
    /// Not a way to start a game — [`TwentyFortyEight::seeded`] is — but the only way to write a
    /// *position* down, and it is how a page that draws a board nobody could reach by playing is
    /// checked. The seed is [`TwentyFortyEight::seeded`]'s, so a tile that arrives on the first slide
    /// arrives where a real game would put it.
    #[cfg(test)]
    pub(crate) fn from_cells(cells: [u32; crate::grid::SIDE * crate::grid::SIDE]) -> Self {
        // Nothing is dealt and no tile arrives: the position *is* the board, and a tile on top of it
        // would be a board the test did not write down. `won` starts false for the same reason — a
        // board handed a 2048 has not been *played*, and the test that wants a win makes one.
        Self {
            grid: Grid::from_cells(cells),
            score: 0,
            random: Random::seeded(0),
            won: false,
        }
    }

    /// The board.
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// What the merges have been worth so far.
    pub fn score(&self) -> u32 {
        self.score
    }

    /// Whether a 2048 tile has been made at any point in this game.
    pub fn has_won(&self) -> bool {
        self.won
    }

    /// Whether there is nothing left to do. See [`Grid::is_over`].
    pub fn is_over(&self) -> bool {
        self.grid.is_over()
    }

    /// Slides the board, and lets a new tile in if anything moved.
    ///
    /// The order matters and is the rule: the tile arrives *after* the slide, so it cannot be pushed
    /// by the move that made room for it. A slide on a finished board is a no-op by construction —
    /// nothing can move on a board that has no moves — and so is one that would change nothing, which
    /// is what keeps a person from farming tiles by pressing a wall.
    pub fn slide(&mut self, direction: Direction) {
        // Two statements rather than one: `take` wants the whole game, and the board's own borrow of it
        // is not over until the slide has answered.
        let moved = self.grid.slide(direction);
        self.take(moved);
    }

    /// What a slide did, applied to this game: the score, the new tile, and the win.
    fn take(&mut self, moved: Move) {
        if !moved.changed {
            return;
        }

        self.score += moved.scored;
        // Checked after the slide and not only when a tile is made: the tile that reaches 2048 can be
        // one that a merge made, or one that arrived on this move, and both of those are the same
        // event to the person watching.
        self.won = self.won || self.grid.largest() >= WINNING;
        let _ = self.grid.spawn(&mut self.random);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new game has two tiles on it, and both of them are twos or fours.
    #[test]
    fn a_new_game_has_two_tiles() {
        let game = TwentyFortyEight::seeded(11);
        let cells = game.grid().cells();

        let filled: Vec<u32> = cells.iter().copied().filter(|cell| *cell != 0).collect();
        assert_eq!(filled.len(), OPENING_TILES);
        assert!(filled.iter().all(|tile| *tile == 2 || *tile == 4));
        assert_eq!(game.score(), 0);
        assert!(!game.has_won());
        assert!(!game.is_over());
    }

    /// The same seed plays the same game, however many moves are made.
    #[test]
    fn the_same_seed_plays_the_same_game() {
        let mut one = TwentyFortyEight::seeded(3);
        let mut other = TwentyFortyEight::seeded(3);

        let moves = [
            Direction::Left,
            Direction::Down,
            Direction::Right,
            Direction::Up,
            Direction::Left,
            Direction::Down,
        ];

        for direction in moves {
            one.slide(direction);
            other.slide(direction);
        }

        assert_eq!(one.grid().cells(), other.grid().cells());
        assert_eq!(one.score(), other.score());
    }

    /// A slide that changes nothing does not let a tile in.
    ///
    /// The rule a person notices: pressing a wall the board is already against cannot be used to fill
    /// the board up with free tiles, and the score cannot be grown by pressing a button. Written
    /// against a position rather than played into: *one* left slide does not put a board against the
    /// left wall — the tile that arrives afterwards can land anywhere, including to the left of a gap —
    /// so a test that pressed twice and expected the same board would be testing the generator.
    #[test]
    fn a_slide_that_moves_nothing_adds_no_tile() {
        // Already against the left wall, and nothing in any row can meet.
        let mut game = TwentyFortyEight::from_cells([
            2, 4, 8, 16, //
            32, 0, 0, 0, //
            0, 0, 0, 0, //
            0, 0, 0, 0,
        ]);

        let before = game.grid().cells();
        assert_eq!(game.score(), 0);

        game.slide(Direction::Left);

        assert_eq!(game.grid().cells(), before, "the press changed nothing");
        assert_eq!(game.score(), 0);
    }

    /// A slide that moves something does let a tile in, and takes the merges into the score.
    #[test]
    fn a_slide_that_moves_the_board_leaves_a_tile_and_scores() {
        // Two twos with a gap in front of them: the pack brings them together and they meet.
        let mut game = TwentyFortyEight::from_cells([
            0, 0, 2, 2, //
            0, 0, 0, 0, //
            0, 0, 0, 0, //
            0, 0, 0, 0,
        ]);

        game.slide(Direction::Left);

        assert_eq!(game.grid().get(0, 0), 4, "the two of them became one");
        assert_eq!(game.score(), 4, "and the merge is what it was worth");
        assert_eq!(
            game.grid().cells().iter().filter(|cell| **cell != 0).count(),
            2,
            "and a new tile arrived behind it"
        );
    }

    /// A game that reaches 2048 remembers it, even after the tile is merged away.
    ///
    /// Written against a board rather than played out: reaching 2048 by playing is a few hundred moves
    /// of a game this test would have to be able to lose, and the thing being checked is one line of
    /// `take`, not the game's whole shape.
    #[test]
    fn a_game_that_reached_2048_remembers_it() {
        let mut game = TwentyFortyEight::seeded(2);
        let two_of_them = [2048, 2048, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        game.grid = Grid::from_cells(two_of_them);

        assert!(
            !game.has_won(),
            "a board handed two of them has not been played, and a win is something that happens"
        );

        // The two meet, which is the move that makes the win.
        game.slide(Direction::Left);

        assert!(game.has_won());
        assert_eq!(game.grid().get(0, 0), 4096);
    }
}
