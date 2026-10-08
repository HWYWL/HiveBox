//! When the animation draws, and when it stops asking to be redrawn.
//!
//! The three numbers are the original's, so that the two ports drew the same animation over
//! the same two and a bit seconds. They are here rather than in `lib.rs` because the clock is the
//! one part of an animation that can be tested without one.

/// How long the blank canvas is held before the stroke starts, in seconds.
pub const DELAY: f32 = 0.6;

/// How long the stroke takes to draw itself, in seconds.
///
/// The original's 3.2 s run 50% faster.
pub const CYCLE: f32 = 2.13;

/// How long the finished picture is held before the app stops asking for frames, in seconds.
///
/// Nothing draws during it. It exists so that the last frame is on the panel and stays there: an
/// app that stopped asking the instant it finished would leave the frame before the end.
pub const SETTLE: f32 = 0.4;

/// How far along the stroke is, `elapsed` seconds after the animation started, in `0.0..=1.0`.
pub fn progress_after(elapsed: f32) -> f32 {
    if elapsed < DELAY {
        0.0
    } else {
        ((elapsed - DELAY) / CYCLE).clamp(0.0, 1.0)
    }
}

/// Whether the animation still wants a frame a tick, `elapsed` seconds after it started.
pub fn running_after(elapsed: f32) -> bool {
    elapsed < DELAY + CYCLE + SETTLE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clock arithmetic is in `f32` and the sums are rounded, so a comparison that has to hold
    /// within a millisecond of the cycle is not the same as an exact one.
    fn close(progress: f32, expected: f32) {
        assert!(
            (progress - expected).abs() < 0.001,
            "{progress} is not {expected}"
        );
    }

    #[test]
    fn nothing_is_drawn_while_the_canvas_is_blank() {
        assert_eq!(progress_after(0.0), 0.0);
        assert_eq!(progress_after(DELAY - 0.01), 0.0);
    }

    #[test]
    fn the_stroke_takes_the_cycle_to_draw_itself() {
        assert_eq!(progress_after(DELAY), 0.0);
        close(progress_after(DELAY + CYCLE / 2.0), 0.5);
        close(progress_after(DELAY + CYCLE), 1.0);
    }

    #[test]
    fn a_finished_stroke_stays_finished() {
        close(progress_after(DELAY + CYCLE + 10.0), 1.0);
        close(progress_after(60.0), 1.0);
    }

    #[test]
    fn the_animation_ends_after_the_last_frame_is_held() {
        assert!(running_after(0.0));
        assert!(
            running_after(DELAY + CYCLE),
            "the finished picture is still being drawn"
        );
        assert!(running_after(DELAY + CYCLE + SETTLE - 0.01));
        assert!(!running_after(DELAY + CYCLE + SETTLE));
        assert!(!running_after(60.0));
    }
}
