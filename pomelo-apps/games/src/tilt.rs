//! Steering a game by tipping the board.
//!
//! # What this reads
//!
//! Gravity, and nothing else. A board lying flat on a desk has all of it pointing through the screen
//! and none of it along either of the in-plane axes; tip one edge down and the part of gravity that
//! runs towards that edge is the sine of the angle. So this divides the reading by its own magnitude
//! and asks which of the two in-plane axes is carrying more of what is left — the tilt is a fraction
//! of a gravity either way, and a fraction is a number this can compare with a threshold.
//!
//! # Why nothing here knows what a g is
//!
//! [`ImuBackend::read_accel`](pomelo_hal::ImuBackend::read_accel) documents its answer as g, and the
//! simulator answers in m/s². Both are gravity, they differ by a factor of 9.81, and this file would
//! be wrong on one of them whichever it picked. It picks neither: the reading is divided by its own
//! magnitude first, so the fraction is what is left, and the threshold is a fraction too. A driver
//! that lands and reports whatever it likes is a driver this works with.
//!
//! # The part that cannot be checked without the board
//!
//! Which of the chip's axes is which of the panel's. See [`AXES`].

use pomelo_hal::Vec3;

use crate::grid::Direction;

/// How much of a gravity in one of the in-plane axes means a direction: about 20 degrees of tip.
///
/// Above the noise on a hand-held board by a wide margin — the simulator's own jitter is a few
/// thousandths — and low enough that steering does not mean holding the board up on one edge.
pub const THRESHOLD: f32 = 0.35;

/// The chip's two in-plane axes as the panel's: `+x` is to the right of the screen and `+y` towards
/// its bottom, so a positive reading on an axis means that edge is the low one.
///
/// `(1.0, 1.0)` is a part mounted square with the panel, which is what the reference layout is and
/// what this board is assumed to be. It is a constant rather than a comment because it is the one
/// thing in this file nobody can check without the hardware in their hand: a board whose part is
/// rotated a quarter turn, or mounted on the other side, hands over its tilt with two of these signs
/// the other way round, and left and right coming out swapped is a pair of signs here and not a
/// rewrite of [`direction`].
const AXES: (f32, f32) = (1.0, 1.0);

/// A magnitude this far from gravity is not a board being tipped: it is a board being shaken, or a
/// reading that never arrived. In the units of whatever the backend answers in, so it is two orders
/// of magnitude either side of both of them.
const SANE: (f32, f32) = (0.5, 50.0);

/// Which way the board is tipped, if it is tipped far enough to mean one.
///
/// `None` is a board lying flat, and it is what a person steering with the pad and the arrow keys
/// will see for as long as they are not holding the thing: a game that read nothing as "the last
/// way it was going" would be a game steered by whatever happened last.
pub fn direction(accel: Vec3) -> Option<Direction> {
    let magnitude = accel.magnitude();

    if !(SANE.0..=SANE.1).contains(&magnitude) {
        return None;
    }

    let (across, down) = (AXES.0 * accel.x / magnitude, AXES.1 * accel.y / magnitude);

    if across.abs().max(down.abs()) < THRESHOLD {
        return None;
    }

    // The stronger of the two axes, so that a board held at 45 degrees picks a way rather than
    // answering nothing.
    Some(if across.abs() >= down.abs() {
        if across > 0.0 {
            Direction::Right
        } else {
            Direction::Left
        }
    } else if down > 0.0 {
        Direction::Down
    } else {
        Direction::Up
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An edge down is a way to go, and it is the edge that went down.
    #[test]
    fn a_tipped_board_is_steered_towards_the_low_edge() {
        // Tipped a bit over a fifth of a gravity: the low edge is the way the snake is asked to go.
        assert_eq!(
            direction(Vec3::new(0.6, 0.0, 0.8)),
            Some(Direction::Right)
        );
        assert_eq!(direction(Vec3::new(-0.6, 0.0, 0.8)), Some(Direction::Left));
        assert_eq!(direction(Vec3::new(0.0, 0.6, 0.8)), Some(Direction::Down));
        assert_eq!(direction(Vec3::new(0.0, -0.6, 0.8)), Some(Direction::Up));
    }

    /// Flat, shaken, barely tipped, or not answering: none of them is a direction.
    #[test]
    fn a_board_that_is_not_tipped_steers_nothing() {
        // Flat on a desk: all of gravity through the screen, and none of it along either plane.
        assert_eq!(direction(Vec3::new(0.0, 0.0, 1.0)), None, "in g");
        assert_eq!(direction(Vec3::new(0.0, 0.0, 9.81)), None, "in m/s²");

        // A magnitude nowhere near a gravity: a board being shaken, or a reading that never arrived.
        assert_eq!(direction(Vec3::new(0.0, 0.0, 0.0)), None);
        assert_eq!(direction(Vec3::new(100.0, 0.0, 0.0)), None);

        // A tilt too shallow to mean anything: the noise of a hand, not a steering.
        assert_eq!(direction(Vec3::new(0.05, 0.0, 0.99)), None);
    }

    /// Held at forty-five degrees the board picks a way rather than answering nothing, and the same
    /// tilt in g and in m/s² is the same way — which is the whole reason no unit is named here.
    #[test]
    fn the_stronger_axis_wins_and_the_unit_does_not_matter() {
        assert_eq!(
            direction(Vec3::new(0.7, -0.7, 0.1)),
            Some(Direction::Right),
            "a tie goes to the horizontal, where a hand is steadiest"
        );

        assert_eq!(
            direction(Vec3::new(0.6, 0.0, 0.8)),
            direction(Vec3::new(0.6 * 9.81, 0.0, 0.8 * 9.81)),
            "the reading is divided by its own magnitude, so only the fraction is left"
        );
    }
}
