use crate::geometry::{Point, Rect};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathVerb {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    pub(crate) verbs: Vec<PathVerb>,
}

impl Path {
    pub fn new() -> Self {
        Self { verbs: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// Precise mathematical bounding box of this path, calculated via kurbo derivative extrema.
    pub fn bounds(&self) -> Rect {
        use kurbo::Shape;
        let bez = kurbo::BezPath::from(self);
        Rect::from(bez.bounding_box())
    }

    /// Flatten this path into one or more polylines of line segments using kurbo's
    /// curvature-adaptive non-recursive subdivision.
    pub fn flatten(&self, tolerance: f32) -> Vec<Vec<Point>> {
        let mut polylines = Vec::new();
        let mut current_poly = Vec::new();
        let bez = kurbo::BezPath::from(self);
        let tol = (tolerance.max(0.05)) as f64;

        kurbo::flatten(bez, tol, |el| {
            match el {
                kurbo::PathEl::MoveTo(p) => {
                    if !current_poly.is_empty() {
                        polylines.push(std::mem::take(&mut current_poly));
                    }
                    current_poly.push(Point::from(p));
                }
                kurbo::PathEl::LineTo(p) => {
                    current_poly.push(Point::from(p));
                }
                kurbo::PathEl::ClosePath => {
                    if let Some(&first) = current_poly.first() {
                        if current_poly.last() != Some(&first) {
                            current_poly.push(first);
                        }
                    }
                    if !current_poly.is_empty() {
                        polylines.push(std::mem::take(&mut current_poly));
                    }
                }
                _ => {}
            }
        });

        if !current_poly.is_empty() {
            polylines.push(current_poly);
        }

        polylines
    }
    /// Construct a `Path` from any `kurbo::Shape` at the specified tolerance.
    pub fn from_shape(shape: &impl kurbo::Shape, tolerance: f64) -> Self {
        let mut path = Path::new();
        for el in shape.path_elements(tolerance) {
            match el {
                kurbo::PathEl::MoveTo(p) => path.verbs.push(PathVerb::MoveTo(p.into())),
                kurbo::PathEl::LineTo(p) => path.verbs.push(PathVerb::LineTo(p.into())),
                kurbo::PathEl::QuadTo(p1, p2) => {
                    path.verbs.push(PathVerb::QuadTo(p1.into(), p2.into()))
                }
                kurbo::PathEl::CurveTo(p1, p2, p3) => {
                    path.verbs.push(PathVerb::CubicTo(p1.into(), p2.into(), p3.into()))
                }
                kurbo::PathEl::ClosePath => path.verbs.push(PathVerb::Close),
            }
        }
        path
    }
}

impl From<kurbo::Circle> for Path {
    fn from(c: kurbo::Circle) -> Self {
        Path::from_shape(&c, 0.1)
    }
}

impl From<kurbo::RoundedRect> for Path {
    fn from(r: kurbo::RoundedRect) -> Self {
        Path::from_shape(&r, 0.1)
    }
}

impl From<kurbo::Rect> for Path {
    fn from(r: kurbo::Rect) -> Self {
        Path::from_shape(&r, 0.1)
    }
}

impl From<kurbo::Line> for Path {
    fn from(l: kurbo::Line) -> Self {
        Path::from_shape(&l, 0.1)
    }
}

impl From<&Path> for kurbo::BezPath {
    fn from(path: &Path) -> Self {
        let mut bez = kurbo::BezPath::new();
        for verb in &path.verbs {
            match *verb {
                PathVerb::MoveTo(p) => bez.move_to(kurbo::Point::from(p)),
                PathVerb::LineTo(p) => bez.line_to(kurbo::Point::from(p)),
                PathVerb::QuadTo(p1, p2) => {
                    bez.quad_to(kurbo::Point::from(p1), kurbo::Point::from(p2))
                }
                PathVerb::CubicTo(p1, p2, p3) => {
                    bez.curve_to(
                        kurbo::Point::from(p1),
                        kurbo::Point::from(p2),
                        kurbo::Point::from(p3),
                    )
                }
                PathVerb::Close => bez.close_path(),
            }
        }
        bez
    }
}

impl From<kurbo::BezPath> for Path {
    fn from(bez: kurbo::BezPath) -> Self {
        let mut path = Path::new();
        for el in bez.elements() {
            match *el {
                kurbo::PathEl::MoveTo(p) => path.verbs.push(PathVerb::MoveTo(p.into())),
                kurbo::PathEl::LineTo(p) => path.verbs.push(PathVerb::LineTo(p.into())),
                kurbo::PathEl::QuadTo(p1, p2) => {
                    path.verbs.push(PathVerb::QuadTo(p1.into(), p2.into()))
                }
                kurbo::PathEl::CurveTo(p1, p2, p3) => {
                    path.verbs.push(PathVerb::CubicTo(p1.into(), p2.into(), p3.into()))
                }
                kurbo::PathEl::ClosePath => path.verbs.push(PathVerb::Close),
            }
        }
        path
    }
}

#[derive(Debug, Clone, Default)]
pub struct PathBuilder {
    verbs: Vec<PathVerb>,
}

impl PathBuilder {
    pub fn new() -> Self {
        Self { verbs: Vec::new() }
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::MoveTo(Point::new(x, y)));
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::LineTo(Point::new(x, y)));
    }

    pub fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.verbs
            .push(PathVerb::QuadTo(Point::new(x1, y1), Point::new(x, y)));
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.verbs.push(PathVerb::CubicTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
            Point::new(x, y),
        ));
    }

    pub fn close(&mut self) {
        self.verbs.push(PathVerb::Close);
    }

    pub fn push_shape(&mut self, shape: &impl kurbo::Shape, tolerance: f64) {
        for el in shape.path_elements(tolerance) {
            match el {
                kurbo::PathEl::MoveTo(p) => self.move_to(p.x as f32, p.y as f32),
                kurbo::PathEl::LineTo(p) => self.line_to(p.x as f32, p.y as f32),
                kurbo::PathEl::QuadTo(p1, p2) => {
                    self.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32)
                }
                kurbo::PathEl::CurveTo(p1, p2, p3) => self.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p3.x as f32,
                    p3.y as f32,
                ),
                kurbo::PathEl::ClosePath => self.close(),
            }
        }
    }

    pub fn push_rect(&mut self, rect: Rect) {
        let kr: kurbo::Rect = rect.into();
        self.push_shape(&kr, 0.1);
    }

    pub fn push_circle(&mut self, cx: f32, cy: f32, radius: f32) {
        let circle = kurbo::Circle::new((cx as f64, cy as f64), radius as f64);
        self.push_shape(&circle, 0.1);
    }

    pub fn push_rrect(&mut self, rrect: crate::geometry::RRect) {
        let kr: kurbo::RoundedRect = rrect.into();
        self.push_shape(&kr, 0.1);
    }

    pub fn finish(self) -> Option<Path> {
        if self.verbs.is_empty() {
            None
        } else {
            Some(Path { verbs: self.verbs })
        }
    }
}
