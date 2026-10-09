//! A ring gauge: one arc, drawn with iced's canvas.
//!
//! # Why this is a canvas and not a widget tree
//!
//! Everything else in this app is built out of the widgets iced ships, and a ring is the one shape
//! those widgets cannot make: `usage_bar` on the memory page is two `container`s sharing a track by
//! flex, which works because a bar is a rectangle and a rectangle is a box. A ring is not — it is a
//! curve with a thickness, drawn over another curve — and the honest way to draw a curve is the
//! geometry layer.
//!
//! That layer is available on the board: `iced-pomelo-gfx` implements iced's full
//! `geometry::Renderer`, so a `Path` a canvas builds arrives at `pomelo-gfx` as a real path and is
//! stroked by its own rasteriser. `pomelo-apps/hello` draws its signature the same way, which is
//! what makes this a port of a shape rather than a new capability.
//!
//! # The number is not drawn here
//!
//! A canvas in this stack can fill and stroke, and it cannot draw text — `fill_text` is a `todo!()`
//! in the geometry backend, because doing it properly means handing the rasteriser a font. So the
//! percentage and its label are ordinary `text` elements stacked over the canvas by
//! [`crate::pages::summary`], which is also what keeps them in the app's own font and its tier
//! preferences rather than in a canvas's.

use iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use iced::mouse;
use iced::widget::canvas::{
    self, path::Arc, Canvas, Frame, Geometry, LineCap, LineJoin, Path, Stroke as Line,
};
use iced::{Color, Element, Length, Point, Radians, Rectangle, Theme};

use crate::style;
use crate::Message;

/// How much of the ring is filled, and in what colour.
///
/// A fraction rather than a percentage, and clamped rather than trusted: a backend that reported
/// 120 % of its heap would otherwise have its arc run a fifth of the way round again, over itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// How far round the ring goes, in `0.0..=1.0`.
    pub fraction: f32,
    /// The colour of the arc.
    pub color: Color,
}

impl Reading {
    /// A reading of `percent`, drawn in `color`.
    pub fn percent(percent: f32, color: Color) -> Self {
        Self {
            fraction: percent / 100.0,
            color,
        }
    }
}

/// The ring itself, as the canvas program draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ring {
    reading: Reading,
    /// The circle the arc is laid over: the whole dial, so that the empty part of it reads as room
    /// rather than as absence.
    track: Color,
}

impl<Message, Renderer> canvas::Program<Message, Theme, Renderer> for Ring
where
    Renderer: GeometryRenderer + 'static,
{
    /// Nothing. The ring has no state to keep between frames — it is a function of the reading and
    /// the theme, both of which are rebuilt by every `view` — and a cache would be a second copy of
    /// a shape that costs one arc to re-record.
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

        // Inset by half the thickness: a stroke is centred on the path it follows, so a circle drawn
        // at the full radius would put half of itself outside the box it was given and lose it.
        let radius = (size.width.min(size.height) - style::RING_THICKNESS) / 2.0;
        let line = |color: Color| Line {
            style: color.into(),
            width: style::RING_THICKNESS,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Line::default()
        };

        frame.stroke(&Path::circle(center, radius), line(self.track));

        let fraction = self.reading.fraction.clamp(0.0, 1.0);

        if fraction > 0.0 {
            // Twelve o'clock and clockwise, which is where a dial is read from and needs no legend.
            //
            // A whole ring is drawn as a circle rather than as an arc that closes on itself: an arc
            // whose two ends meet is two round caps in the same place, and the seam shows as a
            // thicker patch at the top of an otherwise even ring.
            let sweep = if fraction >= 1.0 {
                Path::circle(center, radius)
            } else {
                let start = -std::f32::consts::FRAC_PI_2;

                Path::new(|builder| {
                    builder.arc(Arc {
                        center,
                        radius,
                        start_angle: Radians(start),
                        end_angle: Radians(start + fraction * std::f32::consts::TAU),
                    })
                })
            };

            frame.stroke(&sweep, line(self.reading.color));
        }

        vec![frame.into_geometry()]
    }
}

/// A ring, as an element.
///
/// Sized here rather than by the caller: the diameter is what makes three of them fit one row, and
/// it is the same number for all three — see [`style::RING_DIAMETER`].
pub(crate) fn ring<'a>(reading: Reading, track: Color) -> Element<'a, Message> {
    Canvas::new(Ring { reading, track })
        .width(Length::Fixed(style::RING_DIAMETER))
        .height(Length::Fixed(style::RING_DIAMETER))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arc is open, and it starts on the circle.
    ///
    /// Both halves of this are things the renderer got wrong once: a subpath that iced does not
    /// close was closed behind a canvas's back, and the extra line across the dial is what a person
    /// saw on the board. The fix is in `iced-pomelo-gfx` (see its
    /// `an_open_arc_is_not_closed_behind_the_canvas_back`); this is the same claim made at the
    /// other end — that the path this program asks for is the open one it means.
    #[test]
    fn the_arc_is_an_open_path_that_starts_on_the_ring() {
        use iced::widget::canvas::path::lyon_path::PathEvent;

        let center = Point::new(54.0, 54.0);
        let radius = 49.0;
        let start = -std::f32::consts::FRAC_PI_2;
        let fraction = 0.41;

        let path = Path::new(|builder| {
            builder.arc(Arc {
                center,
                radius,
                start_angle: Radians(start),
                end_angle: Radians(start + fraction * std::f32::consts::TAU),
            })
        });

        let events: Vec<PathEvent> = path.raw().iter().collect();

        // It begins at twelve o'clock on the ring — not at the centre, which is where a path built
        // with a `move_to` in the wrong place would start.
        match events.first().expect("a Begin") {
            PathEvent::Begin { at, .. } => {
                assert!((at.x - center.x).abs() < 0.1, "{at:?} is not above the centre");
                assert!(
                    (at.y - (center.y - radius)).abs() < 0.1,
                    "{at:?} is not on the ring"
                );
            }
            other => panic!("the path begins with {other:?}"),
        }

        // And it ends open. A closed subpath is drawn with a chord from where the curve stopped back
        // to where it started, which on this shape is a line across the middle of the dial.
        match events.last().expect("an End") {
            PathEvent::End { close, .. } => assert!(!close, "the arc is not a closed subpath"),
            other => panic!("the path ends with {other:?}"),
        }
    }

    /// A percentage becomes the share of the circle it is, and the two ends of the scale are exact.
    #[test]
    fn a_percentage_is_the_share_of_the_circle_it_is() {
        assert_eq!(Reading::percent(0.0, Color::BLACK).fraction, 0.0);
        assert_eq!(Reading::percent(50.0, Color::BLACK).fraction, 0.5);
        assert_eq!(Reading::percent(100.0, Color::BLACK).fraction, 1.0);
    }

    /// Nothing a backend can report draws outside its own ring.
    ///
    /// `clamp` and not an assertion: a heap that came back over 100 % is a backend disagreeing with
    /// itself, and the reading of it that a person sees should be "full" rather than an arc that has
    /// gone round twice — which, with two round caps, is a bright patch at the top of the dial.
    #[test]
    fn a_reading_outside_the_scale_is_drawn_at_its_near_end() {
        let over = Reading::percent(140.0, Color::BLACK);
        let under = Reading::percent(-20.0, Color::BLACK);

        assert!(over.fraction.clamp(0.0, 1.0) == 1.0);
        assert!(under.fraction.clamp(0.0, 1.0) == 0.0);

        // And the clamp really is what the drawing uses: an unclamped 1.4 would be this.
        assert!(over.fraction > 1.0, "the raw share is kept, so the clamp matters");
    }
}
