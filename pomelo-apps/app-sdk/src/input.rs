//! Touch, as an application sees it.
//!
//! The host sends a press, any number of moves and one release. Most applications care about the
//! *tap* — a press and a release close together — and about whether a finger is currently down so
//! that a button can be drawn pressed. Both are answered here, and a drag is the difference between
//! the two positions, for the pages that scroll.
//!
//! A tap is decided on release and by distance rather than by time: the panel is held in a hand and
//! a deliberate press can last a while, while a finger that moved 30 px was scrolling, not pressing.

use app_abi::{event, Event};

use crate::ui::Rect;

/// How far a finger may move and still have been tapping, in logical pixels.
pub const TAP_SLOP: f32 = 24.0;

/// The last known state of the one finger this panel has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// Where the finger is, or was.
    pub x: f32,
    pub y: f32,
    /// Whether a finger is down right now.
    pub down: bool,
    /// Whether it went down in this frame's events.
    pub just_down: bool,
    /// Whether it came up in this frame's events.
    pub just_up: bool,
    /// Where the tap that ended in this frame was, if it was a tap.
    pub tap: Option<(f32, f32)>,
    down_at: (f32, f32),
}

impl Input {
    /// Folds one event into the state.
    pub fn apply(&mut self, event: Event) {
        match event.kind {
            event::TOUCH_DOWN => {
                self.x = event.x as f32;
                self.y = event.y as f32;
                self.down = true;
                self.just_down = true;
                self.down_at = (self.x, self.y);
            }
            event::TOUCH_MOVE => {
                self.x = event.x as f32;
                self.y = event.y as f32;
            }
            event::TOUCH_UP => {
                self.x = event.x as f32;
                self.y = event.y as f32;
                self.down = false;
                self.just_up = true;

                let moved = self.distance(self.down_at.0, self.down_at.1, self.x, self.y);

                if moved <= TAP_SLOP * TAP_SLOP {
                    self.tap = Some(self.down_at);
                }
            }
            _ => {}
        }
    }

    /// Whether the tap that ended in this frame landed inside `rect`.
    pub fn tapped(&self, rect: Rect) -> bool {
        self.tap
            .map(|(x, y)| rect.contains(x, y))
            .unwrap_or(false)
    }

    /// Whether a finger is on `rect` right now — what a pressed button is drawn from.
    pub fn pressing(&self, rect: Rect) -> bool {
        self.down && rect.contains(self.x, self.y)
    }

    /// Forgets the one-frame flags. Called by the runtime after the frame that could see them.
    pub fn end_frame(&mut self) {
        self.just_down = false;
        self.just_up = false;
        self.tap = None;
    }

    fn distance(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
        let dx = x1 - x0;
        let dy = y1 - y0;

        // The square rather than the root: `f32::sqrt` is not in `core`, and only the comparison
        // against a constant slop matters.
        let squared = dx * dx + dy * dy;

        // A monotone stand-in for the distance: the caller compares it with `TAP_SLOP * TAP_SLOP`
        // through the helper below, so the value never has to be a real length.
        squared
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(x: i32, y: i32) -> Event {
        Event::new(event::TOUCH_DOWN, x, y, 0, 0)
    }

    fn up(x: i32, y: i32) -> Event {
        Event::new(event::TOUCH_UP, x, y, 0, 0)
    }

    #[test]
    fn a_press_and_a_release_in_the_same_place_is_a_tap() {
        let mut input = Input::default();
        input.apply(down(100, 100));
        input.apply(up(104, 98));

        assert!(input.tapped(Rect::new(80.0, 80.0, 60.0, 60.0)));
        assert!(!input.tapped(Rect::new(0.0, 0.0, 40.0, 40.0)));
        assert!(!input.down);
    }

    /// A finger that travelled was scrolling, and the button underneath it was never pressed.
    #[test]
    fn a_drag_is_not_a_tap() {
        let mut input = Input::default();
        input.apply(down(100, 300));
        input.apply(Event::new(event::TOUCH_MOVE, 100, 180, 0, 0));
        input.apply(up(100, 120));

        assert!(!input.tapped(Rect::new(0.0, 0.0, 480.0, 480.0)));
    }

    /// The one-frame flags do not survive the frame that saw them: a tap is one press, not a state
    /// that stays true until the next press.
    #[test]
    fn the_flags_are_cleared_at_the_end_of_a_frame() {
        let mut input = Input::default();
        input.apply(down(10, 10));
        assert!(input.just_down);

        input.end_frame();
        assert!(!input.just_down);
        assert_eq!(input.tap, None);

        input.apply(up(10, 10));
        assert!(input.tapped(Rect::new(0.0, 0.0, 20.0, 20.0)));
    }
}
