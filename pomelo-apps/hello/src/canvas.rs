//! The signature as a canvas program.
//!
//! A canvas hands its program a [`Frame`] and gets back geometry: a list of commands, in device
//! space, for the renderer to record or draw. Everything here is stated in the artwork's own
//! canonical coordinates -- the frame's transform is what puts them on the panel -- except the width
//! of the line, which a canvas does not scale. `stroke::Layout` says why.
//!
//! # Only the growing tip is redrawn
//!
//! The wash and every piece that has finished go through a [`Cache`]; the piece still growing is
//! recorded live, every frame, and it is the only thing either renderer has to look at:
//!
//! * on a desktop, `iced_tiny_skia` compares a cache by the identity of its buffer and by nothing
//!   else -- without one, a canvas is a single indivisible item and a growing stroke repaints the
//!   whole window every frame -- so the damage becomes the tip;
//! * on the board, `iced-pomelo-gfx` skips a cache for the same reason, and walks the live tip
//!   command by command: the same answer with less work.
//!
//! A cache is filled with [`Cache::draw_with_bounds`] and not `draw`, and the rectangle it is given
//! is load-bearing twice: it is what the cache compares to decide whether to redraw, and it is all
//! the damage that piece can cause. [`Layout::piece_bounds`] is where a piece's rectangle comes
//! from. The caches live in the [`Program::State`], which belongs to the element -- [`Signature`]
//! itself is built again by every `view`, so anything that has to survive a frame cannot live in
//! it.
//!
//! # What this port does not keep
//!
//! The original filled its own buffer with a **dithered** gradient, which is how a two-colour
//! wash survives being quantised to RGB565. A canvas hands its gradient to the rasteriser instead,
//! and the rasteriser samples it: the same two colours arrive as visible bands. Getting the dithering
//! back would mean the geometry being an image, which is a worse trade than a smooth wash.

use std::cell::{Cell, RefCell};

use iced::advanced::graphics::geometry::Renderer as CanvasRenderer;
use iced::advanced::graphics::gradient::{Gradient, Linear};
use iced::mouse;
use iced::widget::canvas::{self, Cache, Frame, Geometry, LineCap, LineJoin, Stroke as Line};
use iced::{Point, Rectangle, Size, Theme};

use pomelo_widgets::preferences::ThemeMode;

use crate::stroke::{Layout, Piece, Stroke};
use crate::style;

/// One moment of the animation: the stroke, and how far along it is.
#[derive(Debug, Clone, Copy)]
pub struct Signature<'a> {
    /// The stroke, already cut into paths.
    pub stroke: &'a Stroke,
    /// How far along the animation is, in `0.0..=1.0`.
    pub progress: f32,
}

/// What the canvas keeps between frames: the caches, and where the animation was.
///
/// It belongs to the element and not to the program value, so it survives every rebuild of
/// `view`. `draw` is handed it by shared reference, which is why the parts that change are behind a
/// [`Cell`] or a [`RefCell`].
pub struct SignatureState<Renderer>
where
    Renderer: CanvasRenderer,
{
    /// The wash, filled once. It is redrawn if the theme changes.
    wash: RefCell<Cache<Renderer>>,
    /// One cache per piece that has finished, in the stroke's order.
    ///
    /// A piece is filled the first frame it is whole and never touched again, which is what makes
    /// it cost a pointer comparison a frame instead of a repaint.
    finished: RefCell<Vec<Cache<Renderer>>>,
    /// The progress the last frame was drawn at, so that a cycle that starts over can be told.
    progress: Cell<f32>,
    /// The theme mode the last frame was drawn with.
    theme_mode: Cell<ThemeMode>,
    /// The size those caches were filled at. A resize is the one thing that changes what a piece
    /// looks like without changing the piece's own rectangle.
    size: Cell<Size>,
}

impl<Renderer> Default for SignatureState<Renderer>
where
    Renderer: CanvasRenderer,
{
    fn default() -> Self {
        Self {
            wash: RefCell::new(Cache::new()),
            finished: RefCell::new(Vec::new()),
            progress: Cell::new(0.0),
            theme_mode: Cell::new(ThemeMode::default()),
            size: Cell::new(Size::ZERO),
        }
    }
}

impl<Message, Renderer> canvas::Program<Message, Theme, Renderer> for Signature<'_>
where
    Renderer: CanvasRenderer + 'static,
{
    type State = SignatureState<Renderer>;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let size = bounds.size();
        let progress = self.progress.clamp(0.0, 1.0);
        let layout = Layout::in_canvas(size);
        let theme_mode = if theme.palette().text == iced::Color::BLACK {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };

        // A cycle that went backwards is a restart, and a new size or theme means the pieces are drawn at
        // another scale or palette: either way the caches hold the wrong picture.
        if progress < state.progress.get()
            || state.size.get() != size
            || theme_mode != state.theme_mode.get()
        {
            state.finished.borrow_mut().clear();
            state.wash.borrow_mut().clear();
        }

        state.progress.set(progress);
        state.size.set(size);
        state.theme_mode.set(theme_mode);

        let mut geometries = Vec::new();

        // The wash is cached and redrawn only when size or theme mode changes.
        geometries.push(state.wash.borrow().draw(renderer, size, |frame| {
            frame.fill_rectangle(Point::ORIGIN, size, wash(size, theme_mode));
        }));

        if progress > 0.0 {
            let line = Line {
                width: layout.stroke,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Line::default()
            };

            let finished: Vec<(usize, &Piece)> =
                self.stroke.finished(progress).enumerate().collect();
            let mut caches = state.finished.borrow_mut();

            while caches.len() < finished.len() {
                caches.push(Cache::new());
            }

            for (index, piece) in finished {
                geometries.push(caches[index].draw_with_bounds(
                    renderer,
                    layout.piece_bounds(piece),
                    |frame| {
                        frame.scale(layout.scale);
                        frame.translate(layout.origin);

                        frame.stroke(
                            &piece.path,
                            Line {
                                style: Gradient::Linear(piece.gradient()).into(),
                                ..line
                            },
                        );
                    },
                ));
            }

            // The piece that is still growing is recorded live, every frame: it is the one thing
            // that changes, and the only damage the renderer has to find.
            if let Some(tip) = self.stroke.tip(progress) {
                let mut frame = Frame::new(renderer, size);

                frame.scale(layout.scale);
                frame.translate(layout.origin);

                frame.stroke(
                    &tip.path(),
                    Line {
                        style: Gradient::Linear(tip.gradient()).into(),
                        ..line
                    },
                );

                geometries.push(frame.into_geometry());
            }
        }

        geometries
    }
}

/// The wash behind the signature, left to right.
fn wash(size: Size, theme_mode: ThemeMode) -> Linear {
    Linear::new(Point::ORIGIN, Point::new(size.width, 0.0))
        .add_stop(0.0, style::wash_start_for(theme_mode))
        .add_stop(1.0, style::wash_end_for(theme_mode))
}
