//! Lightweight Flutter-style animation primitives for Iced widgets.
//!
//! Provides:
//! - [`Curve`]: Easing curves (e.g. `EaseOutCubic`, `EaseInOutQuad`)
//! - [`AnimationController`]: Frame-driven controller interpolating values over time

use std::time::{Duration, Instant};

/// Standard easing curves, modeled after Flutter's `Curves`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Curve {
    /// Linear progression at constant speed.
    Linear,
    /// Cubic deceleration curve (fast start, smooth stop).
    #[default]
    EaseOutCubic,
    /// Cubic acceleration curve (slow start, fast end).
    EaseInCubic,
    /// Symmetrical cubic ease-in and ease-out.
    EaseInOutCubic,
    /// Quadratic deceleration curve.
    EaseOutQuad,
    /// Quadratic acceleration curve.
    EaseInQuad,
    /// Symmetrical quadratic ease-in and ease-out.
    EaseInOutQuad,
}

impl Curve {
    /// Transforms normalized progression `t` in `[0.0, 1.0]` according to this curve.
    pub fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Self::EaseInCubic => t.powi(3),
            Self::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Self::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInQuad => t * t,
            Self::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
        }
    }
}

/// An animation controller driving scalar transitions over time.
#[derive(Debug, Clone)]
pub struct AnimationController {
    from: f32,
    to: f32,
    current: f32,
    start_time: Option<Instant>,
    duration: Duration,
    curve: Curve,
    is_animating: bool,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl AnimationController {
    /// Creates a new [`AnimationController`] resting at `initial`.
    pub fn new(initial: f32) -> Self {
        Self {
            from: initial,
            to: initial,
            current: initial,
            start_time: None,
            duration: Duration::from_millis(250),
            curve: Curve::EaseOutCubic,
            is_animating: false,
        }
    }

    /// Starts animating from `from` to `to` over `duration` using `curve`.
    pub fn animate_to(
        &mut self,
        from: f32,
        to: f32,
        duration: Duration,
        curve: Curve,
        now: Instant,
    ) {
        self.from = from;
        self.to = to;
        self.current = from;
        self.start_time = Some(now);
        self.duration = duration;
        self.curve = curve;
        self.is_animating = from != to && duration > Duration::ZERO;

        if !self.is_animating {
            self.current = to;
            self.start_time = None;
        }
    }

    /// Advances the animation to `now`.
    ///
    /// Returns `true` if the animation is still in progress, or `false` if it
    /// has completed (or was already idle).
    pub fn update(&mut self, now: Instant) -> bool {
        if !self.is_animating {
            return false;
        }

        let Some(start_time) = self.start_time else {
            self.is_animating = false;
            return false;
        };

        if now <= start_time {
            self.current = self.from;
            return true;
        }

        let elapsed = now.duration_since(start_time);
        if elapsed >= self.duration {
            self.current = self.to;
            self.is_animating = false;
            self.start_time = None;
            return false;
        }

        let t = (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);
        let progress = self.curve.transform(t);
        self.current = self.from + (self.to - self.from) * progress;
        true
    }

    /// The current animated scalar value.
    pub fn value(&self) -> f32 {
        self.current
    }

    /// Sets the current value directly and cancels any active animation.
    pub fn set_value(&mut self, value: f32) {
        self.current = value;
        self.from = value;
        self.to = value;
        self.is_animating = false;
        self.start_time = None;
    }

    /// Whether an animation is currently running.
    pub fn is_animating(&self) -> bool {
        self.is_animating
    }

    /// Cancels any in-flight animation immediately, keeping the current value.
    pub fn stop(&mut self) {
        self.is_animating = false;
        self.start_time = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_zero_and_end_at_one() {
        let curves = [
            Curve::Linear,
            Curve::EaseOutCubic,
            Curve::EaseInCubic,
            Curve::EaseInOutCubic,
            Curve::EaseOutQuad,
            Curve::EaseInQuad,
            Curve::EaseInOutQuad,
        ];

        for curve in curves {
            assert!((curve.transform(0.0) - 0.0).abs() < 1e-6);
            assert!((curve.transform(1.0) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn animation_controller_progression() {
        let mut controller = AnimationController::new(0.0);
        let t0 = Instant::now();

        controller.animate_to(
            0.0,
            100.0,
            Duration::from_millis(100),
            Curve::Linear,
            t0,
        );

        assert!(controller.is_animating());
        assert_eq!(controller.value(), 0.0);

        // Advance 50ms (halfway)
        let t50 = t0 + Duration::from_millis(50);
        let running = controller.update(t50);
        assert!(running);
        assert!((controller.value() - 50.0).abs() < 0.1);

        // Advance 100ms (end)
        let t100 = t0 + Duration::from_millis(100);
        let running = controller.update(t100);
        assert!(!running);
        assert_eq!(controller.value(), 100.0);
        assert!(!controller.is_animating());
    }
}
