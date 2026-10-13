//! The clock face: a dial with three hands, drawn with iced's canvas.
//!
//! # Why the face is a canvas
//!
//! The pomodoro ring next door makes this argument in full — a ring is a curve, iced's widgets draw
//! rectangles, and the geometry layer is the honest way to draw a curve. A dial is the same argument
//! with more of it: sixty ticks, three hands at arbitrary angles, and a centre. Laid out as widgets it
//! would be sixty rotated rectangles and a trigonometry problem in the layout code, which is the one
//! place trigonometry is least welcome.
//!
//! # Why there is no number on it
//!
//! `fill_text` is a `todo!()` in the geometry backend — a canvas here can fill and stroke and cannot
//! draw text, because doing it properly means handing the rasteriser a font. So the face carries
//! ticks and hands, and the *numbers* are ordinary `text` elements below it, which is also what keeps
//! them in the app's own font.

use iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineCap, LineJoin, Path, Stroke as Line};
use iced::{Color, Element, Length, Point, Rectangle, Theme};

use crate::time::Civil;
use crate::{style, Message};

/// The face, as the canvas program draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dial {
    /// What the hands are drawn at. `None` is a board whose clock has never been set, and the dial is
    /// then drawn empty: no hands is not midnight, and a face showing 12:00 on a board that does not
    /// know the time would be inventing a fact about it.
    reading: Option<Civil>,
    /// The dial's own ring and its ticks.
    marks: Color,
    /// The hour and minute hands.
    hour_hand: Color,
    /// The second hand, which is the accent because it is the one that moves every second.
    second_hand: Color,
}

impl Dial {
    /// The face for `reading`, in `theme`.
    pub fn new(reading: Option<Civil>, theme: pomelo_widgets::preferences::ThemeMode) -> Self {
        Self {
            reading,
            marks: style::track(theme),
            hour_hand: style::hand(theme),
            second_hand: style::work(),
        }
    }
}

/// A point `radius` away from `center`, at `turns` of the way round clockwise from twelve o'clock.
///
/// The one piece of trigonometry in this app, written as turns rather than radians because that is
/// what a dial *is*: a hand three quarters of the way round is at nine, whatever the radius. The
/// sine is the x and the cosine is subtracted from the y because a screen's y grows *downwards*, and
/// the one thing a dial that gets this wrong does is run anticlockwise.
fn point_at(center: Point, radius: f32, turns: f32) -> Point {
    let angle = turns * std::f32::consts::TAU;

    Point::new(
        center.x + radius * angle.sin(),
        center.y - radius * angle.cos(),
    )
}

/// The stroke a dial is drawn with: round caps and joins, because a hand is a line with two ends and
/// square ends on a diagonal hand read as a hand that was cut off.
fn line<'a>(color: Color, width: f32) -> Line<'a> {
    Line {
        style: color.into(),
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Line::default()
    }
}

impl<Message, Renderer> canvas::Program<Message, Theme, Renderer> for Dial
where
    Renderer: GeometryRenderer + 'static,
{
    /// Nothing: the face is a function of the reading and the theme, both of which every `view`
    /// rebuilds. A cache would be a second copy of a shape that costs sixty lines to re-record.
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

        // Square, and taken from the smaller side: the page hands this a square, and a box that came
        // out oblong — which a Fill inside a flexible column can do — would otherwise put the dial's
        // ticks outside it on the long axis.
        let side = size.width.min(size.height);
        let radius = side / 2.0;

        let mut frame = Frame::new(renderer, size);

        let center = Point::new(size.width / 2.0, size.height / 2.0);

        // The dial's outer ring, just inside the box so that the stroke's own width is not clipped.
        frame.stroke(
            &Path::circle(center, radius - 1.0),
            line(self.marks, (radius * 0.012).max(1.0)),
        );

        // The ticks: sixty of them in one path, stroked once. The hour ticks are the long ones and
        // they are the ink; the minute ticks are half as long and are the same muted colour as the
        // ring, so that a dial read from across a desk reads as twelve marks rather than sixty.
        //
        // Sixty separate subpaths in one path is the shape that once came out wrong on this board —
        // the renderer closed every subpath behind the canvas's back, and the dial grew a line joining
        // each tick to the next. That was fixed in `iced-pomelo-gfx` (see its
        // `an_open_arc_is_not_closed_behind_the_canvas_back`, and the same claim asserted from the
        // other end in `settings`' ring); this is a dial that depends on the fix, which is worth
        // knowing before anyone removes it.
        let mut minutes = Path::new(|builder| {
            for minute in 0..60 {
                if minute % 5 == 0 {
                    continue;
                }

                let turns = minute as f32 / 60.0;
                let outer = radius * 0.90;
                let inner = radius * 0.83;

                builder.move_to(point_at(center, inner, turns));
                builder.line_to(point_at(center, outer, turns));
            }
        });

        let mut hours = Path::new(|builder| {
            for hour in 0..12 {
                let turns = hour as f32 / 12.0;
                let outer = radius * 0.90;
                let inner = radius * 0.78;

                builder.move_to(point_at(center, inner, turns));
                builder.line_to(point_at(center, outer, turns));
            }
        });

        let tick_width = (radius * 0.014).max(1.0);

        frame.stroke(&mut minutes, line(self.marks, tick_width));
        frame.stroke(&mut hours, line(self.hour_hand, tick_width * 1.6));

        if let Some(reading) = self.reading {
            // A twelve-hour dial, which is what a dial is: the hour hand of a 24-hour face is a hand
            // that goes round twice and means two different times.
            let hour = (reading.hour % 12) as f32 + reading.minute as f32 / 60.0;
            let hour_turns = hour / 12.0;
            let minute_turns = reading.minute_angle() / 60.0;
            let second_turns = reading.second_angle() / 60.0;

            frame.stroke(
                &Path::line(
                    center,
                    point_at(center, radius * 0.50, hour_turns),
                ),
                line(self.hour_hand, (radius * 0.055).max(2.0)),
            );

            frame.stroke(
                &Path::line(
                    center,
                    point_at(center, radius * 0.74, minute_turns),
                ),
                line(self.hour_hand, (radius * 0.038).max(1.5)),
            );

            // The second hand runs a little past the minute hand and a little behind it as well: a
            // hand that only goes forwards is a hand whose tail is the part that says which end is
            // which on a face this small.
            frame.stroke(
                &Path::line(
                    point_at(center, radius * 0.16, second_turns + 0.5),
                    point_at(center, radius * 0.86, second_turns),
                ),
                line(self.second_hand, (radius * 0.016).max(1.0)),
            );

            // The centre: the three hands meet in a place, and a bare meeting of three strokes at
            // slightly different widths is a smudge.
            frame.fill(&Path::circle(center, (radius * 0.028).max(1.5)), self.second_hand);
        }

        vec![frame.into_geometry()]
    }
}

/// The face, as an element, filling the box it is given.
pub fn dial<'a>(reading: Option<Civil>, theme: pomelo_widgets::preferences::ThemeMode) -> Element<'a, Message> {
    Canvas::new(Dial::new(reading, theme))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four cardinal directions, which is the whole of the dial's geometry and the one place a
    /// screen's descending y can be got wrong.
    #[test]
    fn twelve_is_up_three_is_right_and_the_y_axis_is_not_a_maths_axis() {
        let center = Point::new(100.0, 100.0);

        let twelve = point_at(center, 10.0, 0.0);
        let three = point_at(center, 10.0, 0.25);
        let six = point_at(center, 10.0, 0.5);
        let nine = point_at(center, 10.0, 0.75);

        assert!((twelve.x - 100.0).abs() < 0.01 && (twelve.y - 90.0).abs() < 0.01);
        assert!((three.x - 110.0).abs() < 0.01 && (three.y - 100.0).abs() < 0.01);
        assert!((six.x - 100.0).abs() < 0.01 && (six.y - 110.0).abs() < 0.01);
        assert!((nine.x - 90.0).abs() < 0.01 && (nine.y - 100.0).abs() < 0.01);
    }

    /// And it goes clockwise, which is the property the four points above do not quite state: half way
    /// between twelve and three is *up and to the right*, not up and to the left.
    #[test]
    fn the_hands_go_clockwise() {
        let center = Point::new(0.0, 0.0);
        let half_past_one_ish = point_at(center, 1.0, 0.125);

        assert!(half_past_one_ish.x > 0.0, "to the right");
        assert!(half_past_one_ish.y < 0.0, "and above the middle");
    }

    /// A hand at a known time points where the time says it does: half past three has the hour hand
    /// between the three and the four, and the minute hand straight down.
    #[test]
    fn a_reading_puts_the_hands_where_the_time_is() {
        let reading = crate::time::Civil {
            year: 2026,
            month: 10,
            day: 12,
            hour: 15,
            minute: 30,
            second: 0,
            weekday: 0,
        };

        let hour = (reading.hour % 12) as f32 + reading.minute as f32 / 60.0;
        let hour_turns = hour / 12.0;

        assert!((hour_turns - (3.5 / 12.0)).abs() < f32::EPSILON);
        assert!((reading.minute_angle() / 60.0 - 0.5).abs() < f32::EPSILON);
    }
}
