use crate::geometry::Rect;

/// A contiguous 16-bit RGB565 pixel buffer allocated on the heap.
#[derive(Debug, Clone, PartialEq)]
pub struct Pixmap565 {
    width: u32,
    height: u32,
    data: Vec<u16>,
}

pub type Pixmap = Pixmap565;
pub type PixmapMut<'a> = Pixmap565Mut<'a>;

impl Pixmap565 {
    /// Create a new Pixmap565 initialized to 0 (black).
    pub fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let total = (width as usize).checked_mul(height as usize)?;
        let mut data = Vec::new();
        data.try_reserve_exact(total).ok()?;
        data.resize(total, 0u16);
        Some(Self {
            width,
            height,
            data,
        })
    }

    /// Create from existing Vec of RGB565 words.
    pub fn from_vec(width: u32, height: u32, data: Vec<u16>) -> Option<Self> {
        if width == 0 || height == 0 || data.len() != (width as usize) * (height as usize) {
            None
        } else {
            Some(Self {
                width,
                height,
                data,
            })
        }
    }

    #[inline(always)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline(always)]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline(always)]
    pub fn data(&self) -> &[u16] {
        &self.data
    }

    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut [u16] {
        &mut self.data
    }

    #[inline(always)]
    pub fn fill(&mut self, color: u16) {
        crate::raster::fill_u16_slice(&mut self.data, color);
    }

    #[inline(always)]
    pub fn as_mut(&mut self) -> Pixmap565Mut<'_> {
        Pixmap565Mut {
            width: self.width,
            height: self.height,
            stride: self.width,
            data: &mut self.data,
        }
    }

    /// Extract a sub-rectangle of RGB565 pixels into `dst` slice.
    /// Fast slice-copy per row without any pixel format conversion.
    pub fn extract_rect(&self, rect: Rect, dst: &mut [u16]) -> (i32, i32, i32, i32, usize) {
        let pix_w = self.width as i32;
        let pix_h = self.height as i32;

        let x1 = (rect.x.floor() as i32).clamp(0, pix_w);
        let y1 = (rect.y.floor() as i32).clamp(0, pix_h);
        let x2 = (rect.right().ceil() as i32).clamp(x1, pix_w);
        let y2 = (rect.bottom().ceil() as i32).clamp(y1, pix_h);

        // Hardware constraint for display controllers (e.g. CO5300 QSPI AMOLED):
        // Round x1, y1 down to even numbers; round x2, y2 up to even numbers.
        let x1 = (x1 >> 1) << 1;
        let y1 = (y1 >> 1) << 1;
        let x2 = (((x2 + 1) >> 1) << 1).min(pix_w);
        let y2 = (((y2 + 1) >> 1) << 1).min(pix_h);

        let rect_w = (x2 - x1) as usize;
        let rect_h = (y2 - y1) as usize;
        let total_pixels = rect_w * rect_h;

        if total_pixels == 0 || dst.len() < total_pixels {
            return (x1, y1, x2, y2, 0);
        }

        let stride = self.width as usize;
        let mut out_idx = 0;

        for y in (y1 as usize)..(y2 as usize) {
            let row_start = y * stride + (x1 as usize);
            let row_end = row_start + rect_w;
            dst[out_idx..out_idx + rect_w].copy_from_slice(&self.data[row_start..row_end]);
            out_idx += rect_w;
        }

        (x1, y1, x2, y2, total_pixels)
    }
}

/// A borrowed mutable 16-bit RGB565 raster view.
pub struct Pixmap565Mut<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub data: &'a mut [u16],
}

impl<'a> Pixmap565Mut<'a> {
    #[inline(always)]
    pub fn new(data: &'a mut [u16], width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            stride: width,
            data,
        }
    }

    #[inline(always)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline(always)]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline(always)]
    pub fn data(&self) -> &[u16] {
        self.data
    }

    #[inline(always)]
    pub fn data_mut(&mut self) -> &mut [u16] {
        self.data
    }

    #[inline(always)]
    pub fn fill(&mut self, color: u16) {
        crate::raster::fill_u16_slice(self.data, color);
    }

    #[inline(always)]
    pub fn pixel_offset(&self, x: u32, y: u32) -> usize {
        (y as usize) * (self.stride as usize) + (x as usize)
    }

    #[inline(always)]
    pub fn row_mut(&mut self, y: u32) -> &mut [u16] {
        let start = (y as usize) * (self.stride as usize);
        let end = start + (self.width as usize);
        &mut self.data[start..end]
    }
}
