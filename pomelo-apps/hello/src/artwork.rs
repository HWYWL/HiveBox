//! The geometry of the "hello" stroke: the curves, and where the rainbow is along them.
//!
//! This is the half of the animation that has nothing to do with a framework. What is left for
//! [`crate::stroke`] is turning a [`CubicBezier`] into whatever path type it draws with, and a pair
//! of [`Rgb`]s into whatever a gradient is there.
//!
//! The numbers are the artwork's: the curve list and the canonical size below describe the 2021
//! "hello" screensaver's signature, and the rainbow sample points came from it.

/// A point in the artwork's canonical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Across.
    pub x: f32,
    /// Down.
    pub y: f32,
}

impl Point {
    /// A point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A rectangle, in canonical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// A colour, in the only form the geometry needs: bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// A colour.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    pub p0: Point,
    pub p1: Point,
    pub p2: Point,
    pub p3: Point,
}

impl CubicBezier {
    pub const fn new(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> Self {
        Self {
            p0: Point::new(p0.0, p0.1),
            p1: Point::new(p1.0, p1.1),
            p2: Point::new(p2.0, p2.1),
            p3: Point::new(p3.0, p3.1),
        }
    }

    /// Evaluate point at curve parameter u in [0.0, 1.0].
    pub fn eval(&self, u: f32) -> Point {
        let inv = 1.0 - u;
        let c0 = inv * inv * inv;
        let c1 = 3.0 * inv * inv * u;
        let c2 = 3.0 * inv * u * u;
        let c3 = u * u * u;
        Point::new(
            c0 * self.p0.x + c1 * self.p1.x + c2 * self.p2.x + c3 * self.p3.x,
            c0 * self.p0.y + c1 * self.p1.y + c2 * self.p2.y + c3 * self.p3.y,
        )
    }

    /// Approximate arc length of the cubic Bezier by numerical chord summation.
    pub fn approximate_length(&self, steps: usize) -> f32 {
        let mut len = 0.0;
        let mut prev = self.eval(0.0);
        for i in 1..=steps {
            let u = i as f32 / steps as f32;
            let pt = self.eval(u);
            let dx = pt.x - prev.x;
            let dy = pt.y - prev.y;
            len += (dx * dx + dy * dy).sqrt();
            prev = pt;
        }
        len
    }

    /// Subdivide the cubic Bezier curve at parameter u in [0.0, 1.0] using De Casteljau's algorithm.
    /// Returns the truncated sub-curve from 0.0 to u.
    pub fn split_left(&self, u: f32) -> CubicBezier {
        let u = u.clamp(0.0, 1.0);
        let lerp = |a: Point, b: Point| Point::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u);

        let p01 = lerp(self.p0, self.p1);
        let p12 = lerp(self.p1, self.p2);
        let p23 = lerp(self.p2, self.p3);

        let p012 = lerp(p01, p12);
        let p123 = lerp(p12, p23);

        let p0123 = lerp(p012, p123);

        CubicBezier {
            p0: self.p0,
            p1: p01,
            p2: p012,
            p3: p0123,
        }
    }

    /// Subdivide the cubic Bezier curve at parameter u in [0.0, 1.0] using De Casteljau's algorithm.
    /// Returns the truncated sub-curve from u to 1.0.
    pub fn split_right(&self, u: f32) -> CubicBezier {
        let u = u.clamp(0.0, 1.0);
        let lerp = |a: Point, b: Point| Point::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u);

        let p01 = lerp(self.p0, self.p1);
        let p12 = lerp(self.p1, self.p2);
        let p23 = lerp(self.p2, self.p3);

        let p012 = lerp(p01, p12);
        let p123 = lerp(p12, p23);

        let p0123 = lerp(p012, p123);

        CubicBezier {
            p0: p0123,
            p1: p123,
            p2: p23,
            p3: self.p3,
        }
    }

    /// Extract sub-curve from u0 to u1 in [0.0, 1.0].
    pub fn split_range(&self, u0: f32, u1: f32) -> CubicBezier {
        if u1 <= u0 {
            let p = self.eval(u0);
            return CubicBezier::new((p.x, p.y), (p.x, p.y), (p.x, p.y), (p.x, p.y));
        }
        let left = self.split_left(u1);
        let u_sub = if u1 > 0.0001 {
            (u0 / u1).clamp(0.0, 1.0)
        } else {
            0.0
        };
        left.split_right(u_sub)
    }
}

/// Official mathematical Bezier curves forming the authentic macOS "hello" continuous cursive wordmark.
/// Extracted directly from official Apple vector artwork without terminal period dot.
pub const HELLO_CURVES: &[CubicBezier] = &[
    // --- Letter 'h' ---
    // Curve 0: Entry sweep up toward ascender loop
    CubicBezier::new(
        (-109.06, 632.23),
        (1.95, 570.52),
        (103.11, 491.16),
        (217.88, 356.08),
    ),
    // Curve 1: Upper rise into loop apex
    CubicBezier::new(
        (217.88, 356.08),
        (296.0, 263.87),
        (338.0, 158.58),
        (340.0, 85.96),
    ),
    // Curve 2: Apex round loop
    CubicBezier::new(
        (340.0, 85.96),
        (341.0, 31.96),
        (314.67, -9.0),
        (266.0, -9.0),
    ),
    // Curve 3: Turn around and descend
    CubicBezier::new(
        (266.0, -9.0),
        (212.0, -9.0),
        (178.0, 31.96),
        (157.0, 125.99),
    ),
    // Curve 4: Descend down stem to baseline
    CubicBezier::new(
        (157.0, 125.99),
        (134.0, 229.34),
        (117.0, 347.92),
        (74.0, 728.16),
    ),
    // Curve 5: Retrace seamlessly up the stem to launch arch
    CubicBezier::new(
        (74.0, 728.16),
        (75.4, 715.77),
        (76.81, 703.39),
        (78.21, 691.0),
    ),
    // Curve 6: Arch peak of 'h'
    CubicBezier::new(
        (78.21, 691.0),
        (100.23, 497.47),
        (184.0, 356.16),
        (291.0, 356.16),
    ),
    // Curve 7: Downward curve of 'h' arch
    CubicBezier::new(
        (291.0, 356.16),
        (355.0, 356.16),
        (395.67, 407.16),
        (384.13, 480.16),
    ),
    // Curve 8: Right leg descending
    CubicBezier::new(
        (384.13, 480.16),
        (377.62, 523.16),
        (370.09, 567.16),
        (361.31, 618.16),
    ),
    // Curve 9: Foot of 'h' curving along baseline into 'e'
    CubicBezier::new(
        (361.31, 618.16),
        (351.07, 682.16),
        (380.33, 732.16),
        (468.96, 732.16),
    ),
    // --- Letter 'e' ---
    // Curve 10: Sweep diagonally into eye of 'e'
    CubicBezier::new(
        (468.96, 732.16),
        (598.22, 732.16),
        (739.24, 660.32),
        (811.41, 549.06),
    ),
    // Curve 11: Top loop of 'e'
    CubicBezier::new(
        (811.41, 549.06),
        (836.0, 511.16),
        (846.0, 477.16),
        (847.0, 444.16),
    ),
    // Curve 12: Turn around crest of 'e'
    CubicBezier::new(
        (847.0, 444.16),
        (848.0, 384.16),
        (814.0, 339.16),
        (754.0, 339.16),
    ),
    // Curve 13: Belly of 'e'
    CubicBezier::new(
        (754.0, 339.16),
        (678.0, 339.16),
        (620.0, 425.16),
        (620.0, 535.16),
    ),
    // Curve 14: Baseline sweep out of 'e' toward first 'l'
    CubicBezier::new(
        (620.0, 535.16),
        (620.0, 653.16),
        (684.0, 736.16),
        (819.92, 736.16),
    ),
    // --- First letter 'l' ---
    // Curve 15: Ascend diagonally into first tall loop
    CubicBezier::new(
        (819.92, 736.16),
        (1004.72, 736.16),
        (1209.42, 514.31),
        (1303.48, 266.73),
    ),
    // Curve 16: Upper rise of first 'l'
    CubicBezier::new(
        (1303.48, 266.73),
        (1330.04, 196.83),
        (1340.0, 131.92),
        (1340.0, 86.56),
    ),
    // Curve 17: Apex loop of first 'l'
    CubicBezier::new(
        (1340.0, 86.56),
        (1340.0, 32.78),
        (1323.0, -8.52),
        (1275.0, -8.52),
    ),
    // Curve 18: Turn around apex of first 'l'
    CubicBezier::new(
        (1275.0, -8.52),
        (1228.0, -8.52),
        (1197.0, 27.98),
        (1169.0, 85.6),
    ),
    // Curve 19: Descend down upper stem of first 'l'
    CubicBezier::new(
        (1169.0, 85.6),
        (1136.19, 152.43),
        (1111.93, 248.83),
        (1102.0, 357.8),
    ),
    // Curve 20: Descend down to baseline leading to second 'l'
    CubicBezier::new(
        (1102.0, 357.8),
        (1077.0, 631.22),
        (1133.0, 732.16),
        (1266.15, 732.16),
    ),
    // --- Second letter 'l' ---
    // Curve 21: Ascend diagonally into second tall loop
    CubicBezier::new(
        (1266.15, 732.16),
        (1427.61, 732.16),
        (1607.12, 507.23),
        (1698.77, 265.97),
    ),
    // Curve 22: Upper rise of second 'l'
    CubicBezier::new(
        (1698.77, 265.97),
        (1725.04, 196.83),
        (1735.0, 131.92),
        (1735.0, 86.56),
    ),
    // Curve 23: Apex loop of second 'l'
    CubicBezier::new(
        (1735.0, 86.56),
        (1735.0, 32.78),
        (1718.0, -8.52),
        (1670.0, -8.52),
    ),
    // Curve 24: Turn around apex of second 'l'
    CubicBezier::new(
        (1670.0, -8.52),
        (1623.0, -8.52),
        (1592.0, 27.98),
        (1564.0, 85.6),
    ),
    // Curve 25: Descend down upper stem of second 'l'
    CubicBezier::new(
        (1564.0, 85.6),
        (1531.19, 152.43),
        (1506.93, 248.83),
        (1497.0, 357.8),
    ),
    // Curve 26: Descend down to baseline leading into 'o'
    CubicBezier::new(
        (1497.0, 357.8),
        (1472.0, 631.22),
        (1528.0, 732.16),
        (1646.91, 732.16),
    ),
    // --- Letter 'o' and terminal wave flick ---
    // Curve 27: Ascend into entry of 'o'
    CubicBezier::new(
        (1646.91, 732.16),
        (1765.62, 732.16),
        (1830.11, 628.67),
        (1868.78, 518.78),
    ),
    // Curve 28: Crest of 'o'
    CubicBezier::new(
        (1868.78, 518.78),
        (1907.0, 410.16),
        (1954.0, 343.16),
        (2052.0, 343.16),
    ),
    // Curve 29: Right bowl of 'o'
    CubicBezier::new(
        (2052.0, 343.16),
        (2133.0, 343.16),
        (2197.0, 403.16),
        (2197.0, 516.16),
    ),
    // Curve 30: Bottom sweep of 'o'
    CubicBezier::new(
        (2197.0, 516.16),
        (2197.0, 641.16),
        (2115.9, 735.16),
        (2013.42, 736.16),
    ),
    // Curve 31: Left bowl of 'o'
    CubicBezier::new(
        (2013.42, 736.16),
        (1923.23, 737.16),
        (1864.0, 664.16),
        (1870.0, 554.16),
    ),
    // Curve 32: Inner loop bridge of 'o'
    CubicBezier::new(
        (1870.0, 554.16),
        (1877.0, 432.16),
        (1951.0, 343.16),
        (2048.0, 343.16),
    ),
    // Curve 33: Wave flick transition of 'o'
    CubicBezier::new(
        (2048.0, 343.16),
        (2104.0, 343.16),
        (2151.04, 368.05),
        (2188.0, 395.16),
    ),
    // Curve 34: Terminal wave flourish ending in cyan flick
    CubicBezier::new(
        (2188.0, 395.16),
        (2288.21, 468.26),
        (2365.43, 423.08),
        (2395.0, 350.8),
    ),
];

/// Exact canonical bounding box of the official Apple "hello" wordmark.
#[allow(dead_code)]
pub const CANONICAL_BOUNDS: Rect = Rect {
    x: -109.06,
    y: -9.0,
    width: 2504.06,
    height: 746.16,
};

/// Canonical stroke width widened to produce the authentic thick neon-tube cursive font.
pub const CANONICAL_STROKE_WIDTH: f32 = 76.0;

/// Total visual width and height including stroke radius (38px on all sides).
pub const CANONICAL_VISUAL_WIDTH: f32 = 2580.06;
pub const CANONICAL_VISUAL_HEIGHT: f32 = 822.16;

/// Exact center of the visual ink artwork.
pub const CANONICAL_CENTER_X: f32 = 1142.97;
pub const CANONICAL_CENTER_Y: f32 = 364.08;

/// One piece of the cursive stroke.
///
/// The stroke is cut into these because each carries **one** colour ramp: the rainbow is sampled
/// along the stroke's length, so a single path with a single gradient cannot draw it. An app turns
/// each segment's `curve` into a path and its two colours into a gradient from one end to the other.
#[derive(Clone, Debug, PartialEq)]
pub struct HelloStrokeSegment {
    /// The piece of the stroke this covers.
    pub curve: CubicBezier,
    /// How far along the whole stroke it starts, in canonical units.
    pub start_dist: f32,
    /// How far along the whole stroke it ends.
    pub end_dist: f32,
    /// The rainbow colour where it starts.
    pub start_color: Rgb,
    /// The rainbow colour where it ends.
    pub end_color: Rgb,
}

/// Precomputed stroke geometry containing micro-segments with continuous stroke-following rainbow gradient colors.
pub struct HelloStrokeModel {
    pub segments: Vec<HelloStrokeSegment>,
    pub total_length: f32,
}

impl Default for HelloStrokeModel {
    fn default() -> Self {
        Self::new()
    }
}

impl HelloStrokeModel {
    pub fn new() -> Self {
        let mut lengths = Vec::with_capacity(HELLO_CURVES.len());
        let mut total_length = 0.0;
        for curve in HELLO_CURVES {
            let l = curve.approximate_length(24);
            lengths.push(l);
            total_length += l;
        }

        let mut segments = Vec::new();
        let mut accumulated = 0.0;

        for (i, curve) in HELLO_CURVES.iter().enumerate() {
            let seg_len = lengths[i];
            // Divide each curve into ~48px micro-segments (~190 segments total for 60fps real-time budget)
            let k = ((seg_len / 48.0).round() as usize).max(3);
            for j in 0..k {
                let u0 = j as f32 / k as f32;
                let u1 = (j + 1) as f32 / k as f32;

                let sub_curve = curve.split_range(u0, u1);
                let d0 = accumulated + u0 * seg_len;
                let d1 = accumulated + u1 * seg_len;

                let start_progress = (d0 / total_length).clamp(0.0, 1.0);
                let end_progress = (d1 / total_length).clamp(0.0, 1.0);

                let start_color = rainbow_color_at(start_progress);
                let end_color = rainbow_color_at(end_progress);

                segments.push(HelloStrokeSegment {
                    curve: sub_curve,
                    start_dist: d0,
                    end_dist: d1,
                    start_color,
                    end_color,
                });
            }
            accumulated += seg_len;
        }

        Self {
            segments,
            total_length,
        }
    }
}

/// The 2021 M1 Mac "hello" screensaver's rainbow, sampled along the stroke's length:
/// deep teal, jade, lime, lemon, amber, tangerine, coral, crimson, rose, orchid, lavender, lilac,
/// indigo, cobalt, azure, cyan.
pub const HELLO_RAINBOW_STOPS: [(f32, (u8, u8, u8)); 17] = [
    (0.000, (11, 114, 133)),
    (0.050, (47, 179, 128)),
    (0.088, (140, 220, 60)),
    (0.140, (255, 214, 0)),
    (0.190, (255, 155, 0)),
    (0.250, (247, 107, 28)),
    (0.300, (236, 68, 54)),
    (0.350, (224, 48, 88)),
    (0.440, (240, 90, 112)),
    (0.490, (238, 101, 128)),
    (0.570, (184, 62, 146)),
    (0.660, (150, 85, 185)),
    (0.685, (186, 104, 200)),
    (0.760, (86, 77, 181)),
    (0.840, (59, 114, 222)),
    (0.920, (37, 117, 252)),
    (1.000, (34, 211, 238)),
];

/// Samples that rainbow at `progress`, which runs from the start of the stroke to its end.
pub fn rainbow_color_at(progress: f32) -> Rgb {
    let t = progress.clamp(0.0, 1.0);

    for window in HELLO_RAINBOW_STOPS.windows(2) {
        let (p0, c0) = window[0];
        let (p1, c1) = window[1];

        if t <= p1 {
            let u = if p1 > p0 { (t - p0) / (p1 - p0) } else { 0.0 };
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * u).round() as u8;

            return Rgb::new(mix(c0.0, c1.0), mix(c0.1, c1.1), mix(c0.2, c1.2));
        }
    }

    let last = HELLO_RAINBOW_STOPS[HELLO_RAINBOW_STOPS.len() - 1].1;

    Rgb::new(last.0, last.1, last.2)
}

static SHARED_MODEL: std::sync::OnceLock<std::sync::Arc<HelloStrokeModel>> =
    std::sync::OnceLock::new();

/// The stroke, built once and shared: it is a few hundred curve splits, and every frame that wants
/// it wants the same one.
pub fn get_shared_hello_model() -> std::sync::Arc<HelloStrokeModel> {
    SHARED_MODEL
        .get_or_init(|| std::sync::Arc::new(HelloStrokeModel::new()))
        .clone()
}
