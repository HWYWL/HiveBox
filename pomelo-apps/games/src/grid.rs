//! 2048's rules: sixteen cells, four slides, and what happens to the numbers.
//!
//! Everything here is arithmetic on a `[u32; 16]` and nothing here knows what a pixel is or that a
//! page exists. That split is what makes the game testable at all: "a slide merges each pair once" is
//! a property of this file, and the way to check it is to slide a board somebody wrote down — not to
//! press a button in a window and look at it.
//!
//! # The one rule that is easy to get wrong
//!
//! A tile merges **once per move**, so `2 2 2 2` slid left is `4 4` and not `8`: the first pair
//! merges into a four, and that four does not then merge with the four behind it. The rule shows up
//! here as [`settle`] walking the packed line two at a time and skipping both cells of a pair it
//! merged.

use crate::random::Random;

/// Which way a slide goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The side of the board. 2048 is played on four by four and the number is nowhere in the rules
/// except in the shape of this type.
pub const SIDE: usize = 4;

/// The number of cells on the board.
///
/// `pub(crate)` because it is the length of the array a written-down position is: the tests spell one
/// out, and a board's size is not a secret from the page that draws it.
pub(crate) const CELLS: usize = SIDE * SIDE;

/// The value a tile has when a person has won — the game's name, and the only number in it that is
/// not arithmetic.
pub const WINNING: u32 = 2048;

/// What a slide did.
///
/// Two facts, and they are not the same fact: a slide can move the board without scoring (`2 4` slid
/// left stays `2 4`, slid right becomes `4 2` and scores nothing) and it cannot score without moving.
/// The caller needs the first — a board that moved is a board that owes a new tile — and wants the
/// second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    /// Whether any tile ended up somewhere else, or any pair became one.
    pub changed: bool,
    /// What the merges were worth: the sum of the tiles they made.
    pub scored: u32,
}

/// The board: sixteen cells, and a zero is an empty one.
///
/// Row-major, so cell `(row, col)` is at `row * SIDE + col`, and the only two places that arithmetic
/// appears are [`Grid::at`] and the tests. Nothing else indexes it directly, which is what keeps the
/// four directions from having four subtly different ideas of where a row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    cells: [u32; CELLS],
}

impl Grid {
    /// An empty board, which is not yet a game: [`crate::game::TwentyFortyEight`] is what puts the
    /// first two tiles down.
    pub fn new() -> Self {
        Self { cells: [0; CELLS] }
    }

    /// A board with these cells on it, row by row.
    ///
    /// Not a way to *start* a game — a game starts empty and fills up by being played, see
    /// [`crate::game::TwentyFortyEight::seeded`] — but the one way to write a position down, which is
    /// what a test of the rules is. Kept inside this crate: a board handed to the app already full
    /// would be a board nobody could have played.
    #[cfg(test)]
    pub(crate) fn from_cells(cells: [u32; CELLS]) -> Self {
        Self { cells }
    }

    /// The value in a cell, or zero if the cell is off the board.
    ///
    /// Total rather than panicking, because the drawing code walks `0..SIDE` twice nested and a
    /// mistyped bound there should draw an empty cell rather than take the panel down.
    pub fn get(&self, row: usize, col: usize) -> u32 {
        if row >= SIDE || col >= SIDE {
            return 0;
        }

        self.cells[Self::at(row, col)]
    }

    /// Every cell, row by row. What the tests read, and what a board's own drawing is driven from.
    pub fn cells(&self) -> [u32; CELLS] {
        self.cells
    }

    /// The largest tile on the board, which is zero only on an empty one.
    pub fn largest(&self) -> u32 {
        self.cells.iter().copied().max().unwrap_or(0)
    }

    /// Slides the whole board towards `direction`, and reports what that did.
    pub fn slide(&mut self, direction: Direction) -> Move {
        let mut moved = Move {
            changed: false,
            scored: 0,
        };

        for line in 0..SIDE {
            // The line is read in the order the slide pushes it: index 0 is the cell the tiles are
            // moving towards, so `line[0]` is the far wall whichever direction this is. That is what
            // makes one `settle` serve all four.
            let mut before = [0; SIDE];
            for at in 0..SIDE {
                before[at] = self.cells[Self::cell(line, at, direction)];
            }

            let after = settle(before, &mut moved.scored);

            for at in 0..SIDE {
                let cell = Self::cell(line, at, direction);
                if self.cells[cell] != after[at] {
                    moved.changed = true;
                    self.cells[cell] = after[at];
                }
            }
        }

        moved
    }

    /// Puts a new tile down in an empty cell, and says whether there was one.
    ///
    /// Nine times in ten a `2` and one time in ten a `4`: the game's own rule, and the reason a board
    /// fills up in the shape it does rather than in a straight line. `false` is a board with no room,
    /// which is not an error — it is a board that is [`Grid::is_over`] or one slide away from it.
    pub fn spawn(&mut self, random: &mut Random) -> bool {
        let empty: Vec<usize> = (0..CELLS).filter(|cell| self.cells[*cell] == 0).collect();

        if empty.is_empty() {
            return false;
        }

        let cell = empty[random.below(empty.len())];
        self.cells[cell] = if random.below(10) == 0 { 4 } else { 2 };

        true
    }

    /// Whether there is nothing left to do: no empty cell and no two neighbours that could merge.
    ///
    /// The neighbours are checked right and down only, and that is enough: every pair of touching
    /// cells is one of those two, seen from one of its two ends.
    pub fn is_over(&self) -> bool {
        if self.cells.contains(&0) {
            return false;
        }

        for row in 0..SIDE {
            for col in 0..SIDE {
                let here = self.get(row, col);

                if col + 1 < SIDE && here == self.get(row, col + 1) {
                    return false;
                }

                if row + 1 < SIDE && here == self.get(row + 1, col) {
                    return false;
                }
            }
        }

        true
    }

    /// Where a cell is, by row and column.
    fn at(row: usize, col: usize) -> usize {
        row * SIDE + col
    }

    /// Where the `at`th cell of `line` is, counting from the wall the slide pushes towards.
    ///
    /// The four arms are the whole of the geometry of this file. `line` is a row for `Left` and
    /// `Right` and a column for `Up` and `Down`; `at` counts from the far wall, so `0` is the cell the
    /// tiles pile up against.
    fn cell(line: usize, at: usize, direction: Direction) -> usize {
        match direction {
            Direction::Left => Self::at(line, at),
            Direction::Right => Self::at(line, SIDE - 1 - at),
            Direction::Up => Self::at(at, line),
            Direction::Down => Self::at(SIDE - 1 - at, line),
        }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

/// One line of the board after a slide: packed against the far wall, with each pair merged once.
///
/// `line` is in the order the slide pushes, so the first cell is the wall. What is added to `scored`
/// is what the merges were worth — the caller owns the total, because it is a total across four
/// lines.
fn settle(line: [u32; SIDE], scored: &mut u32) -> [u32; SIDE] {
    let mut packed = [0; SIDE];
    let mut count = 0;

    for value in line {
        if value != 0 {
            packed[count] = value;
            count += 1;
        }
    }

    let mut settled = [0; SIDE];
    // Two cursors: `read` walks the packed line and `write` walks the result. They differ by exactly
    // one for every pair that merged, which is the whole of the arithmetic.
    let mut read = 0;
    let mut write = 0;

    while read < count {
        // Equal neighbours become one tile of their sum, and both are spent: a tile merges once per
        // move, which is why `2 2 2 2` is `4 4` and not `8`.
        if read + 1 < count && packed[read] == packed[read + 1] {
            let merged = packed[read] * 2;
            *scored += merged;
            settled[write] = merged;
            read += 2;
        } else {
            settled[write] = packed[read];
            read += 1;
        }

        write += 1;
    }

    settled
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board written down as four rows, which is how a person reads one.
    fn grid(rows: [[u32; SIDE]; SIDE]) -> Grid {
        let mut cells = [0; CELLS];
        for (row, values) in rows.iter().enumerate() {
            for (col, value) in values.iter().enumerate() {
                cells[Grid::at(row, col)] = *value;
            }
        }

        Grid::from_cells(cells)
    }

    /// The rows of a board, for a readable assertion.
    fn rows(grid: &Grid) -> [[u32; SIDE]; SIDE] {
        let mut rows = [[0; SIDE]; SIDE];
        for row in 0..SIDE {
            for col in 0..SIDE {
                rows[row][col] = grid.get(row, col);
            }
        }

        rows
    }

    /// A slide packs a line against the wall it goes towards, and takes the gaps out.
    #[test]
    fn a_slide_packs_the_line() {
        let mut board = grid([[2, 0, 0, 4], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]]);

        let moved = board.slide(Direction::Left);
        assert_eq!(rows(&board)[0], [2, 4, 0, 0]);
        assert!(moved.changed, "the four moved");
        assert_eq!(moved.scored, 0, "and nothing merged");

        let moved = board.slide(Direction::Right);
        assert_eq!(rows(&board)[0], [0, 0, 2, 4]);
        assert!(moved.changed);
        assert_eq!(moved.scored, 0);

        // And a slide that changes nothing says so: the board was already against that wall.
        let moved = board.slide(Direction::Right);
        assert!(!moved.changed);
        assert_eq!(moved.scored, 0);
    }

    /// Equal neighbours merge, and the two of them become one tile of their sum.
    #[test]
    fn equal_neighbours_merge() {
        let mut board = grid([[2, 2, 0, 0], [4, 4, 4, 0], [0, 8, 8, 8], [0, 0, 0, 0]]);

        let moved = board.slide(Direction::Left);

        assert_eq!(
            rows(&board),
            [[4, 0, 0, 0], [8, 4, 0, 0], [16, 8, 0, 0], [0; SIDE]]
        );
        // 4 + 8 + 16, and the second 8 of the third row does not merge because the first two took
        // each other.
        assert_eq!(moved.scored, 28);
    }

    /// A tile merges once per move, which is the rule the whole game turns on.
    ///
    /// `2 2 2 2` slid left is `4 4`. It is *not* `8`, and it is not `2 4 2` either: the pair nearest
    /// the wall takes each other, and the pair behind it takes each other, and neither of the two new
    /// fours may merge again on the same move.
    #[test]
    fn a_tile_merges_once_per_move() {
        let mut board = grid([[2, 2, 2, 2], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]]);

        let moved = board.slide(Direction::Left);

        assert_eq!(rows(&board)[0], [4, 4, 0, 0]);
        assert_eq!(moved.scored, 8, "two merges of two plus two");

        // And the same line pushed the other way merges the same pair, from the other wall.
        let mut board = grid([[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [2, 2, 2, 2]]);
        board.slide(Direction::Right);
        assert_eq!(rows(&board)[3], [0, 0, 4, 4]);
    }

    /// The two vertical directions are the two horizontal ones, turned.
    ///
    /// Said as a test because it is the only thing the four arms of [`Grid::cell`] can get wrong that
    /// the tests above cannot see: they all slide a single row, and a column is a different index.
    #[test]
    fn up_and_down_are_the_horizontal_moves_turned() {
        let mut board = grid([[2, 0, 0, 0], [2, 0, 0, 0], [4, 0, 0, 0], [4, 0, 0, 0]]);

        board.slide(Direction::Up);
        assert_eq!(
            rows(&board),
            [[4, 0, 0, 0], [8, 0, 0, 0], [0; SIDE], [0; SIDE]]
        );

        // The same board pushed down piles up at the bottom, and the ends are the same two tiles.
        let mut board = grid([[2, 0, 0, 0], [2, 0, 0, 0], [4, 0, 0, 0], [4, 0, 0, 0]]);
        board.slide(Direction::Down);
        assert_eq!(
            rows(&board),
            [[0; SIDE], [0; SIDE], [4, 0, 0, 0], [8, 0, 0, 0]]
        );
    }

    /// A new tile is a two or a four, in an empty cell, and never on top of one.
    #[test]
    fn a_new_tile_lands_on_an_empty_cell() {
        let mut board = grid([[2, 4, 8, 16], [32, 64, 128, 256], [512, 1024, 2, 4], [8, 16, 0, 0]]);
        let before = board.cells();
        let mut random = Random::seeded(1);

        assert!(board.spawn(&mut random));

        let after = board.cells();

        // Exactly one cell that was empty has something in it, and it is a two or a four: what the
        // board gained, rather than what a particular corner of it now holds.
        let appeared: Vec<u32> = (0..CELLS)
            .filter(|cell| before[*cell] == 0 && after[*cell] != 0)
            .map(|cell| after[cell])
            .collect();

        assert_eq!(appeared.len(), 1, "one new tile");
        assert!(
            appeared[0] == 2 || appeared[0] == 4,
            "{} is neither a two nor a four",
            appeared[0]
        );

        // And nothing that was already there was touched.
        for cell in 0..CELLS {
            if before[cell] != 0 {
                assert_eq!(after[cell], before[cell]);
            }
        }
    }

    /// A full board has nowhere to put one, and saying so is not an error.
    #[test]
    fn a_full_board_takes_no_new_tile() {
        let mut board = grid([[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]);
        let mut random = Random::seeded(7);

        assert!(!board.spawn(&mut random), "there was no empty cell");
        assert_eq!(board.cells(), grid([[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]).cells());
    }

    /// The game is over only when nothing can move: an empty cell is always a move, and so is a pair.
    #[test]
    fn the_board_is_over_only_when_nothing_can_move() {
        // Nothing to do: no empty cell, no equal neighbours.
        let stuck = grid([[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]);
        assert!(stuck.is_over());

        // The same board with one cell free is not over, however full it looks.
        let almost = grid([[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 0]]);
        assert!(!almost.is_over());

        // And a full board with one pair in it is not over either: that pair is a move.
        let mergeable = grid([[2, 2, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]]);
        assert!(!mergeable.is_over());
    }

    /// The biggest tile is the biggest tile, and an empty board has none.
    #[test]
    fn the_largest_tile_is_the_largest_tile() {
        assert_eq!(Grid::new().largest(), 0);
        assert_eq!(grid([[0, 4, 32, 0], [2, 0, 8, 0], [0, 0, 0, 0], [0, 0, 0, 0]]).largest(), 32);
    }

    /// A cell off the board is empty, rather than a panic or a wrapped index.
    #[test]
    fn a_cell_off_the_board_is_empty() {
        let board = grid([[2, 4, 2, 4], [0; SIDE], [0; SIDE], [0; SIDE]]);

        assert_eq!(board.get(0, 0), 2);
        assert_eq!(board.get(4, 0), 0, "row four is not row zero");
        assert_eq!(board.get(0, 4), 0);
        assert_eq!(board.get(99, 99), 0);
    }
}
