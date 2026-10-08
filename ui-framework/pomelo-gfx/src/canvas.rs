use crate::color::Color;
use crate::geometry::{Point, RRect, Rect, Transform};
use crate::paint::{FillRule, Paint, Stroke};
use crate::path::Path;
use crate::pixmap::Pixmap565Mut;
use crate::raster;

pub struct Canvas<'a> {
    pixmap: Pixmap565Mut<'a>,
    current_transform: Transform,
    transform_stack: Vec<Transform>,
    clip_stack: Vec<Option<Rect>>,
    current_clip: Option<Rect>,
}

impl<'a> Canvas<'a> {
    pub fn new(pixmap: Pixmap565Mut<'a>) -> Self {
        Self {
            pixmap,
            current_transform: Transform::identity(),
            transform_stack: Vec::new(),
            clip_stack: Vec::new(),
            current_clip: None,
        }
    }

    #[inline(always)]
    pub fn width(&self) -> u32 {
        self.pixmap.width
    }

    #[inline(always)]
    pub fn height(&self) -> u32 {
        self.pixmap.height
    }

    #[inline(always)]
    pub fn data(&self) -> &[u16] {
        self.pixmap.data()
    }

    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut [u16] {
        self.pixmap.data_mut()
    }

    pub fn save(&mut self) {
        self.transform_stack.push(self.current_transform);
        self.clip_stack.push(self.current_clip);
    }

    pub fn restore(&mut self) {
        if let Some(tf) = self.transform_stack.pop() {
            self.current_transform = tf;
        }
        if let Some(clip) = self.clip_stack.pop() {
            self.current_clip = clip;
        }
    }

    #[inline(always)]
    pub fn current_clip(&self) -> Option<Rect> {
        self.current_clip
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.current_transform = self.current_transform.post_translate(dx, dy);
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.current_transform = self.current_transform.post_scale(sx, sy);
    }

    pub fn clip_rect(&mut self, rect: Rect) {
        let mapped = self.current_transform.map_rect(rect);
        self.current_clip = match self.current_clip {
            Some(prev) => prev.intersect(&mapped),
            None => Some(mapped),
        };
    }

    pub fn clear(&mut self, color: Color) {
        if self.current_clip.is_none() {
            self.pixmap.fill(color.to_rgb565());
        } else {
            let full_rect = Rect::from_ltwh(
                0.0,
                0.0,
                self.pixmap.width as f32,
                self.pixmap.height as f32,
            );
            raster::fill_rect(&mut self.pixmap, self.current_clip, full_rect, color);
        }
    }

    pub fn copy_pixels_from(&mut self, src: &[u16]) {
        let dst = self.pixmap.data_mut();
        let len = dst.len().min(src.len());
        dst[..len].copy_from_slice(&src[..len]);
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        let transformed = self.current_transform.map_rect(rect);
        raster::fill_rect(&mut self.pixmap, self.current_clip, transformed, color);
    }

    /// Fills a rectangle with a [`Paint`] rather than a colour: the shader-aware twin of
    /// [`Canvas::draw_rect`].
    ///
    /// It goes through the same quad filler a stroke's segments use, which is what lets a gradient
    /// land in a rectangle. `fill_path` cannot stand in for this: it flattens the path and
    /// *strokes* the outline with a 1 px line, so a filled shape comes out as an outline and the
    /// fill rule is never read.
    pub fn fill_rect(&mut self, rect: Rect, paint: &Paint) {
        let r = self.current_transform.map_rect(rect);

        raster::fill_convex_quad(
            &mut self.pixmap,
            self.current_clip,
            Point::from_xy(r.left(), r.top()),
            Point::from_xy(r.right(), r.top()),
            Point::from_xy(r.right(), r.bottom()),
            Point::from_xy(r.left(), r.bottom()),
            paint,
        );
    }

    pub fn draw_rrect(&mut self, rrect: RRect, color: Color) {
        let transformed = self.current_transform.map_rrect(rrect);
        raster::fill_rrect(&mut self.pixmap, self.current_clip, transformed, color);
    }

    pub fn draw_rrect_stroke(&mut self, rrect: RRect, color: Color, stroke_width: f32) {
        let transformed = self.current_transform.map_rrect(rrect);
        let sw = stroke_width * self.current_transform.sx;
        raster::stroke_rrect(&mut self.pixmap, self.current_clip, transformed, color, sw);
    }

    pub fn draw_circle(&mut self, center: Point, radius: f32, color: Color) {
        let mapped = self.current_transform.map_point(center);
        let r = radius * self.current_transform.sx;
        raster::fill_circle(&mut self.pixmap, self.current_clip, mapped, r, color);
    }

    pub fn blit_mask(&mut self, x: i32, y: i32, w: u32, h: u32, mask: &[u8], color: Color) {
        let tx = x + (self.current_transform.tx.round() as i32);
        let ty = y + (self.current_transform.ty.round() as i32);
        raster::blit_mask(
            &mut self.pixmap,
            self.current_clip,
            tx,
            ty,
            w,
            h,
            mask,
            color,
        );
    }

    pub fn stroke_path(&mut self, path: &Path, paint: &Paint, stroke: &Stroke) {
        let polylines = path.flatten(0.5);
        for poly in polylines {
            let transformed_poly: Vec<Point> = poly
                .iter()
                .map(|p| self.current_transform.map_point(*p))
                .collect();
            let mut s = stroke.clone();
            s.width *= self.current_transform.sx;
            raster::stroke_polyline(
                &mut self.pixmap,
                self.current_clip,
                &transformed_poly,
                paint,
                &s,
            );
        }
    }

    pub fn fill_path(&mut self, path: &Path, paint: &Paint, _fill_rule: FillRule) {
        let polylines = path.flatten(0.5);
        for poly in polylines {
            let transformed_poly: Vec<Point> = poly
                .iter()
                .map(|p| self.current_transform.map_point(*p))
                .collect();
            let s = Stroke {
                width: 1.0,
                ..Default::default()
            };
            raster::stroke_polyline(
                &mut self.pixmap,
                self.current_clip,
                &transformed_poly,
                paint,
                &s,
            );
        }
    }

    pub fn fill_dithered_horizontal_gradient(&mut self, c0: (u8, u8, u8), c1: (u8, u8, u8)) {
        raster::fill_dithered_horizontal_gradient(&mut self.pixmap, self.current_clip, c0, c1);
    }

    pub fn blit_image_565(&mut self, x: i32, y: i32, w: u32, h: u32, pixels: &[u16]) {
        let tx = x + (self.current_transform.tx.round() as i32);
        let ty = y + (self.current_transform.ty.round() as i32);
        raster::blit_image_565(&mut self.pixmap, self.current_clip, tx, ty, w, h, pixels);
    }

    pub fn blit_image_565_with_alpha(
        &mut self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        rgb: &[u16],
        alpha: &[u8],
    ) {
        let tx = x + (self.current_transform.tx.round() as i32);
        let ty = y + (self.current_transform.ty.round() as i32);
        raster::blit_image_565_with_alpha(
            &mut self.pixmap,
            self.current_clip,
            tx,
            ty,
            w,
            h,
            rgb,
            alpha,
        );
    }

    pub fn blit_image_565_with_alpha_scaled(
        &mut self,
        x: i32,
        y: i32,
        dst_w: u32,
        dst_h: u32,
        src_w: u32,
        src_h: u32,
        rgb: &[u16],
        alpha: &[u8],
    ) {
        let tx = x + (self.current_transform.tx.round() as i32);
        let ty = y + (self.current_transform.ty.round() as i32);
        raster::blit_image_565_with_alpha_scaled(
            &mut self.pixmap,
            self.current_clip,
            tx,
            ty,
            dst_w,
            dst_h,
            src_w,
            src_h,
            rgb,
            alpha,
        );
    }
}
