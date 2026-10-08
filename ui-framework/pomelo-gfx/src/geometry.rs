#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline(always)]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub fn from_xy(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub fn distance(&self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    #[inline(always)]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    #[inline(always)]
    pub const fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Option<Self> {
        if width < 0.0 || height < 0.0 {
            None
        } else {
            Some(Self {
                x,
                y,
                width,
                height,
            })
        }
    }

    #[inline(always)]
    pub const fn from_ltwh(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            x: left,
            y: top,
            width: if width < 0.0 { 0.0 } else { width },
            height: if height < 0.0 { 0.0 } else { height },
        }
    }

    #[inline(always)]
    pub fn from_ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        let w = (right - left).max(0.0);
        let h = (bottom - top).max(0.0);
        Self {
            x: left,
            y: top,
            width: w,
            height: h,
        }
    }

    #[inline(always)]
    pub fn left(&self) -> f32 {
        self.x
    }

    #[inline(always)]
    pub fn top(&self) -> f32 {
        self.y
    }

    #[inline(always)]
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    #[inline(always)]
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    #[inline(always)]
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    #[inline(always)]
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        if right > left && bottom > top {
            Some(Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Radius {
    pub x: f32,
    pub y: f32,
}

impl Radius {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline(always)]
    pub const fn circular(radius: f32) -> Self {
        Self {
            x: radius,
            y: radius,
        }
    }

    #[inline(always)]
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RRect {
    pub rect: Rect,
    pub radius: Radius,
}

impl RRect {
    #[inline(always)]
    pub const fn from_rect_radius(rect: Rect, radius: Radius) -> Self {
        Self { rect, radius }
    }

    #[inline(always)]
    pub const fn from_rect_xy(rect: Rect, rx: f32, ry: f32) -> Self {
        Self {
            rect,
            radius: Radius { x: rx, y: ry },
        }
    }
}

/// 2D Affine Transform matrix:
/// [ sx  kx  tx ]
/// [ ky  sy  ty ]
/// [  0   0   1 ]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub sx: f32,
    pub ky: f32,
    pub kx: f32,
    pub sy: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform {
    #[inline(always)]
    pub const fn identity() -> Self {
        Self {
            sx: 1.0,
            ky: 0.0,
            kx: 0.0,
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    #[inline(always)]
    pub const fn from_row(sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        Self {
            sx,
            ky,
            kx,
            sy,
            tx,
            ty,
        }
    }

    #[inline(always)]
    pub const fn from_translate(tx: f32, ty: f32) -> Self {
        Self {
            sx: 1.0,
            ky: 0.0,
            kx: 0.0,
            sy: 1.0,
            tx,
            ty,
        }
    }

    #[inline(always)]
    pub const fn from_scale(sx: f32, sy: f32) -> Self {
        Self {
            sx,
            ky: 0.0,
            kx: 0.0,
            sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    #[inline(always)]
    pub fn post_translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            sx: self.sx,
            ky: self.ky,
            kx: self.kx,
            sy: self.sy,
            tx: self.tx + dx,
            ty: self.ty + dy,
        }
    }

    #[inline(always)]
    pub fn post_scale(&self, sx: f32, sy: f32) -> Self {
        Self {
            sx: self.sx * sx,
            ky: self.ky * sy,
            kx: self.kx * sx,
            sy: self.sy * sy,
            tx: self.tx * sx,
            ty: self.ty * sy,
        }
    }

    #[inline(always)]
    pub fn map_point(&self, p: Point) -> Point {
        Point {
            x: self.sx * p.x + self.kx * p.y + self.tx,
            y: self.ky * p.x + self.sy * p.y + self.ty,
        }
    }

    /// Map an axis-aligned rectangle.
    ///
    /// Skew is ignored, which is the model both rasterizers draw under (they
    /// only ever apply scale and translate).
    ///
    /// Correctly handles negative scale factors (reflections) by ensuring
    /// width and height remain non-negative and (x, y) remains the minimum corner.
    #[inline(always)]
    pub fn map_rect(&self, rect: Rect) -> Rect {
        let x0 = self.sx * rect.x + self.tx;
        let x1 = self.sx * (rect.x + rect.width) + self.tx;
        let y0 = self.sy * rect.y + self.ty;
        let y1 = self.sy * (rect.y + rect.height) + self.ty;

        let min_x = x0.min(x1);
        let min_y = y0.min(y1);
        let width = (x1 - x0).abs();
        let height = (y1 - y0).abs();

        Rect {
            x: min_x,
            y: min_y,
            width,
            height,
        }
    }

    /// Map a rounded rectangle: the box through [`Self::map_rect`], the corner
    /// radii by the absolute scale factors.
    #[inline(always)]
    pub fn map_rrect(&self, rrect: RRect) -> RRect {
        RRect {
            rect: self.map_rect(rrect.rect),
            radius: Radius {
                x: (self.sx * rrect.radius.x).abs(),
                y: (self.sy * rrect.radius.y).abs(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// kurbo interoperability
// ---------------------------------------------------------------------------

impl From<Point> for kurbo::Point {
    #[inline(always)]
    fn from(p: Point) -> Self {
        kurbo::Point::new(p.x as f64, p.y as f64)
    }
}

impl From<kurbo::Point> for Point {
    #[inline(always)]
    fn from(p: kurbo::Point) -> Self {
        Point::new(p.x as f32, p.y as f32)
    }
}

impl From<Point> for kurbo::Vec2 {
    #[inline(always)]
    fn from(p: Point) -> Self {
        kurbo::Vec2::new(p.x as f64, p.y as f64)
    }
}

impl From<kurbo::Vec2> for Point {
    #[inline(always)]
    fn from(v: kurbo::Vec2) -> Self {
        Point::new(v.x as f32, v.y as f32)
    }
}

impl From<Rect> for kurbo::Rect {
    #[inline(always)]
    fn from(r: Rect) -> Self {
        kurbo::Rect::new(
            r.x as f64,
            r.y as f64,
            (r.x + r.width) as f64,
            (r.y + r.height) as f64,
        )
    }
}

impl From<kurbo::Rect> for Rect {
    #[inline(always)]
    fn from(r: kurbo::Rect) -> Self {
        Rect::from_ltrb(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32)
    }
}

impl From<RRect> for kurbo::RoundedRect {
    #[inline(always)]
    fn from(r: RRect) -> Self {
        let rect = kurbo::Rect::from(r.rect);
        kurbo::RoundedRect::from_rect(rect, r.radius.x as f64)
    }
}

impl From<kurbo::RoundedRect> for RRect {
    #[inline(always)]
    fn from(r: kurbo::RoundedRect) -> Self {
        let rect = Rect::from(r.rect());
        let radii = r.radii();
        RRect::from_rect_xy(rect, radii.top_left as f32, radii.top_left as f32)
    }
}

impl From<Transform> for kurbo::Affine {
    #[inline(always)]
    fn from(t: Transform) -> Self {
        kurbo::Affine::new([
            t.sx as f64,
            t.ky as f64,
            t.kx as f64,
            t.sy as f64,
            t.tx as f64,
            t.ty as f64,
        ])
    }
}

impl From<kurbo::Affine> for Transform {
    #[inline(always)]
    fn from(a: kurbo::Affine) -> Self {
        let c = a.as_coeffs();
        Transform {
            sx: c[0] as f32,
            ky: c[1] as f32,
            kx: c[2] as f32,
            sy: c[3] as f32,
            tx: c[4] as f32,
            ty: c[5] as f32,
        }
    }
}
