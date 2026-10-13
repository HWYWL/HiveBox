//! The one thing on this page that is not arithmetic a person could do in their head: where the next
//! tile goes.
//!
//! # Why this is here and not in the HAL
//!
//! The board has no entropy source, and this app is the only thing in the project that wants one: a
//! page that draws a reading, a folder or a scan does not need a number nobody can predict, and an
//! `EntropyBackend` that one puzzle uses would be a trait, a simulator and a device implementation
//! kept right for it. So the numbers are made here, out of the only "different every time" value the
//! HAL does have — how long the board has been up (see `Games::seed`).
//!
//! # What this is not
//!
//! It is not a security-grade generator and does not pretend to be: the state is recoverable from a
//! few outputs, and the seed is a clock. Both are fine for a puzzle and would not be fine for a key,
//! which is a sentence worth writing down precisely because the next person to want a random number
//! in this project should not reach for this type.

/// A sequence of numbers that is not the same one every time.
///
/// `xorshift64*`: a shift register with a multiply at the end, chosen for being four lines that
/// anybody can check by hand rather than for being the best of anything.
#[derive(Debug, Clone)]
pub struct Random {
    state: u64,
}

impl Random {
    /// The sequence `seed` starts. The same seed is the same game, which is what makes the tests
    /// below possible at all.
    pub fn seeded(seed: u64) -> Self {
        // The seed is stirred before it is used, because the seed is a clock reading: a board's uptime
        // in nanoseconds is a number with almost all of its bits at the top and a zero in most of the
        // low ones for the first hour of its life — and a shift register started on a number like that
        // spends its first draws moving the interesting bits around instead of mixing them. One
        // multiply-and-xor puts them everywhere at once.
        let stirred = seed
            .wrapping_add(0x9E37_79B9_7F4A_7C15)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9)
            ^ 0x94D0_49BB_1331_11EB;

        Self {
            // Zero is the one state a shift register cannot leave: it would draw zero forever, and the
            // board would put every tile in the same cell. The stir above makes it vanishingly unlikely
            // rather than impossible, so it is ruled out here.
            state: if stirred == 0 { 0x5EED_2048 } else { stirred },
        }
    }

    /// The next number.
    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;

        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..limit`.
    ///
    /// Modulo, and so very slightly biased towards the low end when `limit` does not divide the range
    /// of the generator. The bias is one part in `2^64 / limit` — the caller picks a cell out of at
    /// most sixteen and a value out of ten, and neither of those is a decision that a tile a
    /// quadrillionth of a move too early would change.
    pub fn below(&mut self, limit: usize) -> usize {
        if limit == 0 {
            // A limit of nothing has no numbers under it, and the caller asking for one is asking for
            // a cell on an empty board: the answer it can use is none.
            return 0;
        }

        (self.next() % limit as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same seed is the same game, which is the whole of what the tests of the rules rely on.
    #[test]
    fn the_same_seed_is_the_same_sequence() {
        let mut once = Random::seeded(42);
        let mut twice = Random::seeded(42);

        for _ in 0..64 {
            assert_eq!(once.next(), twice.next());
        }
    }

    /// Different seeds are different games, and a seed of zero is not a generator stuck at zero.
    #[test]
    fn different_seeds_diverge() {
        let mut one = Random::seeded(0);
        let mut other = Random::seeded(1);

        assert_ne!(one.next(), other.next());
        assert_ne!(Random::seeded(0).next(), 0, "zero is ruled out");
    }

    /// Every number is under the limit it was asked for, and the whole range is reachable.
    ///
    /// The second half is the one worth having: a generator that only ever answered `0` or `1` would
    /// pass a bounds check and put every new tile in the top-left corner.
    #[test]
    fn below_stays_under_its_limit_and_reaches_all_of_it() {
        let mut random = Random::seeded(2026);

        let mut seen = [false; 16];
        for _ in 0..4_000 {
            let value = random.below(16);
            assert!(value < 16);
            seen[value] = true;
        }

        assert!(seen.iter().all(|hit| *hit), "every cell of the board");

        // And a limit of one is always zero, an empty board has no cells to pick.
        assert_eq!(random.below(1), 0);
        assert_eq!(random.below(0), 0);
    }
}
