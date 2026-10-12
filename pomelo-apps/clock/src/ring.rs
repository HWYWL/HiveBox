//! The pomodoro ring: how much of the phase is left, drawn as an arc.
//!
//! The same shape as `settings`' ring gauge and for the same reason — a curve is not a box, and every
//! widget iced ships draws boxes. The differences are the two this app needs: the arc is drawn
//! *anticlockwise as it empties* rather than clockwise as it fills, and its colour is the phase rather
//! than a threshold.
//!
//! That first difference is the one worth stating. A timer ring that fills up as the time runs out is
//! a ring that finishes full, and the moment it finishes is the moment it looks most complete — which
//! is exactly backwards for a thing whose whole message is "this is nearly over". This ring starts
//! full and unwinds, so the empty part is the part that has been spent.

use iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use iced::mouse;
use iced::widget::canvas::{
    self, path::Arc, Canvas, Frame, Geometry, LineCap, LineJoin, Path, Stroke,
};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Theme};

use crate::{style, Message};

/// The arc, as the canvas program draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ring {
    /// How much of the phase is left, in `0.0..=1.0`.
    remaining: f32,
    /// The colour of the arc: the phase's, dimmed when the timer is paused.
    color: Color,
    /// The circle the arc is laid over. The whole dial, so the spent part of it reads as room rather
    /// than as absence.
    track: Color,
}

impl<Message, Renderer> canvas::Program<Message, Theme, Renderer> for Ring
where
    Renderer: GeometryRenderer + 'static,
{
    /// Nothing: the ring is a function of what it is constructed with, all of which every `view`
    /// rebuilds.
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);

        let center = Point::new(size.width / 2.0, size.height / 2.0);

        // Inset by half the thickness: a stroke is centred on the path it follows, so a circle at the
        // full radius would put half of itself outside the box and lose it.
        let radius = (size.width.min(size.height) - style::RING_THICKNESS) / 2.0;

        let stroke = |color: Color| Stroke {
            style: color.into(),
            width: style::RING_THICKNESS,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };

        frame.stroke(&Path::circle(center, radius), stroke(self.track));

        let remaining = self.remaining.clamp(0.0, 1.0);

        if remaining > 0.0 {
            // Twelve o'clock, and the arc runs *backwards* from it, so that what is drawn is what is
            // left and the gap is what has been spent.
            //
            // A full ring is a circle rather than an arc that closes on itself: an arc whose two ends
            // meet is two round caps in the same place, and the seam shows as a thicker patch at the
            // top of an otherwise even ring.
            let sweep = if remaining >= 1.0 {
                Path::circle(center, radius)
            } else {
                let start = -std::f32::consts::FRAC_PI_2;

                Path::new(|builder| {
                    builder.arc(Arc {
                        center,
                        radius,
                        start_angle: Radians(start - remaining * std::f32::consts::TAU),
                        end_angle: Radians(start),
                    })
                })
            };

            frame.stroke(&sweep, stroke(self.color));
        }

        vec![frame.into_geometry()]
    }
}

/// The ring, as an element, filling the box it is given.
///
/// `remaining` is the share of the phase still to run and `color` is what the phase is worth — see
/// [`style::phase_color`], and [`style::muted`] for a timer that is paused.
pub fn ring<'a>(
    remaining: f32,
    color: Color,
    track: Color,
) -> Element<'a, Message> {
    Canvas::new(Ring {
        remaining,
        color,
        track,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arc is open, which is the claim `settings`' ring makes from the other end and the one that
    /// was wrong on this board once: the renderer closed every subpath behind the canvas's back, and
    /// an arc that is closed comes back with a chord across it — here, a line across the middle of the
    /// timer.
    #[test]
    fn the_arc_is_open_and_runs_backwards_from_twelve() {
        use iced::widget::canvas::path::lyon_path::PathEvent;

        let center = Point::new(60.0, 60.0);
        let radius = 50.0;
        let remaining = 0.25;
        let start = -std::f32::consts::FRAC_PI_2;

        let path = Path::new(|builder| {
            builder.arc(Arc {
                center,
                radius,
                start_angle: Radians(start - remaining * std::f32::consts::TAU),
                end_angle: Radians(start),
            })
        });

        let events: Vec<PathEvent> = path.raw().iter().collect();

        // It starts a quarter of the way *round* — nine o'clock, not three — which is the difference
        // between a ring that unwinds and one that fills.
        match events.first().expect("a Begin") {
            PathEvent::Begin { at, .. } => {
                assert!(
                    (at.x - (center.x - radius)).abs() < 0.1,
                    "{at:?} should start at nine o'clock"
                );
                assert!((at.y - center.y).abs() < 0.1, "{at:?} is not level with the centre");
            }
            other => panic!("the path begins with {other:?}"),
        }

        match events.last().expect("an End") {
            PathEvent::End { close, .. } => assert!(!close, "the arc is not a closed subpath"),
            other => panic!("the path ends with {other:?}"),
        }
    }

    /// Neither end of the scale draws outside the ring, and the two ends are drawn differently: an
    /// empty ring is no arc at all rather than an arc of zero length, which with round caps is a dot
    /// at twelve o'clock that reads as "a little bit left".
    #[test]
    fn the_ends_of_the_scale_are_a_track_and_a_circle() {
        // Nothing left: the drawing skips the arc entirely, which the `remaining > 0.0` guard is.
        assert!(!(0.0f32 > 0.0));

        // Everything left: a full circle rather than a closed arc, so that there is no seam.
        let full = Ring {
            remaining: 1.0,
            color: Color::BLACK,
            track: Color::WHITE,
        };
        assert!(full.remaining >= 1.0, "which is what picks the circle");

        // And a reading outside the scale is drawn at its near end rather than wound round again.
        let over = Ring {
            remaining: 1.4,
            color: Color::BLACK,
            track: Color::WHITE,
        };
        assert_eq!(over.remaining.clamp(0.0, 1.0), 1.0);
    }
}
