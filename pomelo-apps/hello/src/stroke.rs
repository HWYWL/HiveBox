//! The stroke, in the shapes a canvas draws with.
//!
//! `artwork` holds the curves and the rainbow and knows nothing about iced. This is the one
//! place they become paths and gradients, and it happens once, when the app starts.
//!
//! That is what makes the animation cheap to draw. A frame adds the pieces that are already
//! finished -- the same paths, the same gradients, the same commands in the same order as the frame
//! before -- and one piece cut at the point the stroke has reached. On the recorded renderer the
//! finished pieces pair with themselves and damage nothing at all, so a frame repaints the growing
//! tip rather than the picture.

use iced::advanced::graphics::gradient::Linear;
use iced::widget::canvas::Path;
use iced::{Color, Point, Rectangle, Size, Vector};

use crate::artwork::{
    rainbow_color_at, CubicBezier, Point as Canonical, Rect, Rgb, CANONICAL_CENTER_X,
    CANONICAL_CENTER_Y, CANONICAL_STROKE_WIDTH, CANONICAL_VISUAL_HEIGHT, CANONICAL_VISUAL_WIDTH,
};

use crate::style;

/// One piece of the stroke: a path, and the two colours the rainbow crosses it in.
#[derive(Debug, Clone)]
pub struct Piece {
    /// The piece, in the artwork's canonical coordinates.
    pub path: Path,
    /// The same curve, which is what a gradient along it is stated with.
    pub curve: CubicBezier,
    /// How far along the whole stroke it starts, in canonical units.
    pub start: f32,
    /// And where it ends.
    pub end: f32,
    /// The rainbow where it starts.
    pub from: Color,
    /// And where it ends.
    pub to: Color,
}

impl Piece {
    /// The gradient that runs along this piece, from one end of it to the other.
    pub fn gradient(&self) -> Linear {
        Linear::new(point(self.curve.p0), point(self.curve.p3))
            .add_stop(0.0, self.from)
            .add_stop(1.0, self.to)
    }

    /// The canonical rectangle the piece's curve stays inside.
    ///
    /// The control points of a cubic contain the curve itself, so this is a bound and not an
    /// approximation of one. What it does not contain is the line's width, which is not the
    /// curve's business: `canvas::Layout::piece_bounds` is what a cache is given, and it grows
    /// this by the line and the edge an antialiased one bleeds into.
    pub fn bounds(&self) -> Rect {
        let [p0, p1, p2, p3] = [self.curve.p0, self.curve.p1, self.curve.p2, self.curve.p3];

        let left = p0.x.min(p1.x).min(p2.x).min(p3.x);
        let top = p0.y.min(p1.y).min(p2.y).min(p3.y);
        let right = p0.x.max(p1.x).max(p2.x).max(p3.x);
        let bottom = p0.y.max(p1.y).max(p2.y).max(p3.y);

        Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }
}

/// The whole stroke, ready to draw.
///
/// It is [`artwork`](crate::artwork)'s model, cut into pieces, with each piece's curve turned into a path once. The
/// rainbow is sampled along the stroke's length, so a piece spanning one colour ramp is the most a
/// single gradient can draw.
#[derive(Debug, Clone)]
pub struct Stroke {
    pieces: Vec<Piece>,
    total_length: f32,
}

impl Default for Stroke {
    fn default() -> Self {
        Self::new()
    }
}

impl Stroke {
    /// Builds the paths and the gradients for the shared model.
    pub fn new() -> Self {
        let model = crate::artwork::get_shared_hello_model();

        let pieces = model
            .segments
            .iter()
            .map(|segment| Piece {
                path: path_of(&segment.curve),
                curve: segment.curve,
                start: segment.start_dist,
                end: segment.end_dist,
                from: color(segment.start_color),
                to: color(segment.end_color),
            })
            .collect();

        Self {
            pieces,
            total_length: model.total_length,
        }
    }

    /// How long the stroke is, in canonical units.
    pub fn total_length(&self) -> f32 {
        self.total_length
    }

    /// How many pieces it is cut into.
    pub fn pieces(&self) -> usize {
        self.pieces.len()
    }

    /// The pieces that are whole at `progress`, in the order they are drawn.
    pub fn finished(&self, progress: f32) -> impl Iterator<Item = &Piece> {
        let reached = progress.clamp(0.0, 1.0) * self.total_length;

        self.pieces
            .iter()
            .take_while(move |piece| piece.end <= reached)
    }

    /// The piece the stroke is drawing at `progress`, cut where it has reached.
    ///
    /// `None` while the canvas is blank, and again at exactly the end of a piece: the stroke is on
    /// a boundary, so nothing of the next piece is drawn yet and the one before it is finished.
    pub fn tip(&self, progress: f32) -> Option<Tip<'_>> {
        let progress = progress.clamp(0.0, 1.0);
        let reached = progress * self.total_length;

        let piece = self.pieces.iter().find(|piece| piece.end > reached)?;
        let length = piece.end - piece.start;
        let into = if length > 0.0 {
            ((reached - piece.start) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if into <= 0.0 {
            return None;
        }

        Some(Tip {
            piece,
            curve: piece.curve.split_left(into),
            colour: color(rainbow_color_at(progress)),
        })
    }
}

/// The piece still being drawn, and the part of it there is.
pub struct Tip<'a> {
    /// The whole piece, which is where its start colour and its path come from.
    pub piece: &'a Piece,
    /// The part of it the stroke has reached.
    pub curve: CubicBezier,
    /// The rainbow at the growing end, which is a colour the stroke has not been yet.
    pub colour: Color,
}

impl Tip<'_> {
    /// The gradient that runs along the part being drawn: from the piece's own start colour to the
    /// colour the stroke has just reached.
    pub fn gradient(&self) -> Linear {
        Linear::new(point(self.curve.p0), point(self.curve.p3))
            .add_stop(0.0, self.piece.from)
            .add_stop(1.0, self.colour)
    }

    /// The part being drawn, as a path.
    pub fn path(&self) -> Path {
        path_of(&self.curve)
    }
}

/// Where the artwork goes in a canvas, and how wide its line is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The canonical point the canvas is scaled from: a canonical point `q` is drawn at
    /// `(q + origin) * scale`, which is what `frame.scale(scale)` and then
    /// `frame.translate(origin)` mean -- a canvas composes its transforms on the right.
    pub origin: Vector,
    /// Canonical units to pixels.
    pub scale: f32,
    /// The width of the line, in pixels.
    pub stroke: f32,
}

impl Layout {
    /// The layout that centres the artwork in a `size` canvas, with the margins `style` asks for.
    ///
    /// `stroke` is in pixels while the path it strokes is in canonical units, and that is not an
    /// oversight: a canvas transforms a path and leaves the width as written. iced's own `tiny_skia`
    /// backend records the width next to a path it has already transformed, and the recorded
    /// renderer does the same, so the only device-space number in a frame is the line.
    pub fn in_canvas(size: Size) -> Self {
        let scale = (size.width * style::WIDTH_FRACTION / CANONICAL_VISUAL_WIDTH)
            .min(size.height * style::HEIGHT_FRACTION / CANONICAL_VISUAL_HEIGHT);

        Self {
            origin: Vector::new(
                size.width * 0.5 / scale - CANONICAL_CENTER_X,
                size.height * 0.5 / scale - CANONICAL_CENTER_Y,
            ),
            scale,
            stroke: (CANONICAL_STROKE_WIDTH * scale).clamp(style::MIN_STROKE, style::MAX_STROKE),
        }
    }

    /// The rectangle a piece is drawn inside, in the canvas's own coordinates.
    ///
    /// A `canvas::Cache` is redrawn when the bounds it was given change, and the damage that cache
    /// can cause is exactly those bounds -- so this has to cover everything the piece paints and no
    /// more. The curve's control points contain the curve, and the line reaches half its width past
    /// it, which in the artwork's own units is the width the rasteriser was given over the scale.
    /// The extra unit is the pixel an antialiased edge bleeds into; it is the same margin both
    /// renderers put on a command's own bounds.
    pub fn piece_bounds(&self, piece: &Piece) -> Rectangle {
        let bounds = piece.bounds();
        let margin = self.stroke / self.scale / 2.0 + 1.0;

        Rectangle {
            x: (bounds.x - margin + self.origin.x) * self.scale,
            y: (bounds.y - margin + self.origin.y) * self.scale,
            width: (bounds.width + 2.0 * margin) * self.scale,
            height: (bounds.height + 2.0 * margin) * self.scale,
        }
    }
}

/// A curve as a canvas path, in canonical coordinates.
fn path_of(curve: &CubicBezier) -> Path {
    Path::new(|builder| {
        builder.move_to(point(curve.p0));
        builder.bezier_curve_to(point(curve.p1), point(curve.p2), point(curve.p3));
    })
}

/// A canonical point, as a canvas point.
fn point(canonical: Canonical) -> Point {
    Point::new(canonical.x, canonical.y)
}

/// A colour, in iced's terms.
fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pieces_are_in_order_and_do_not_overlap() {
        let stroke = Stroke::new();

        assert!(stroke.pieces() > 1, "the rainbow needs more than one ramp");

        let mut reached = 0.0;

        for (index, piece) in stroke.pieces.iter().enumerate() {
            assert!(
                piece.start >= reached - 0.001,
                "piece {index} starts where the one before it ended"
            );
            assert!(piece.end > piece.start, "piece {index} is not empty");

            reached = piece.end;
        }

        assert!(
            (reached - stroke.total_length).abs() < 0.001,
            "the pieces add up to the whole stroke"
        );
    }

    #[test]
    fn nothing_is_drawn_before_the_stroke_starts() {
        let stroke = Stroke::new();

        assert_eq!(stroke.finished(0.0).count(), 0);
        assert!(stroke.tip(0.0).is_none());
    }

    #[test]
    fn the_tip_is_the_piece_the_stroke_has_reached() {
        let stroke = Stroke::new();
        let tip = stroke.tip(0.5).expect("the stroke is half drawn");

        assert!(tip.piece.start < stroke.total_length() / 2.0);
        assert!(tip.piece.end > stroke.total_length() / 2.0);
        assert!(
            tip.curve.approximate_length(32) < tip.piece.curve.approximate_length(32),
            "the tip is the part of that piece the stroke has drawn, not the whole piece"
        );
    }

    #[test]
    fn the_finished_stroke_has_no_tip() {
        let stroke = Stroke::new();

        assert_eq!(stroke.finished(1.0).count(), stroke.pieces());
        assert!(stroke.tip(1.0).is_none());
    }

    #[test]
    fn the_stroke_is_centred_in_its_canvas() {
        let layout = Layout::in_canvas(Size::new(480.0, 480.0));

        let centre_x = (CANONICAL_CENTER_X + layout.origin.x) * layout.scale;
        let centre_y = (CANONICAL_CENTER_Y + layout.origin.y) * layout.scale;

        assert!((centre_x - 240.0).abs() < 0.01, "the artwork is centred");
        assert!((centre_y - 240.0).abs() < 0.01);
    }

    #[test]
    fn the_line_is_clamped_to_something_visible_and_something_sane() {
        let small = Layout::in_canvas(Size::new(40.0, 40.0));
        let large = Layout::in_canvas(Size::new(4000.0, 4000.0));

        assert_eq!(small.stroke, style::MIN_STROKE);
        assert_eq!(large.stroke, style::MAX_STROKE);
    }

    #[test]
    fn a_piece_is_inside_the_bounds_its_cache_is_given() {
        // The bounds a piece is cached with are also its clip: if they were too tight the stroke
        // would be cut off, and if they were too loose a finished piece would damage the panel it
        // sits on. This is the mapping the two have to agree about — a canonical point `q` is drawn
        // at `(q + origin) * scale` — sampled along every curve rather than at its ends.
        let stroke = Stroke::new();
        let layout = Layout::in_canvas(Size::new(480.0, 480.0));

        for piece in stroke.finished(1.0) {
            let bounds = layout.piece_bounds(piece);

            for step in 0..=64 {
                let at = piece.curve.eval(step as f32 / 64.0);

                let device = Point::new(
                    (at.x + layout.origin.x) * layout.scale,
                    (at.y + layout.origin.y) * layout.scale,
                );

                assert!(
                    bounds.contains(device),
                    "the curve at {device:?} is outside the cached bounds {bounds:?}"
                );
            }
        }
    }
}
