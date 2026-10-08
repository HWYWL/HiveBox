#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::from_rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::from_rgb(0, 0, 0);
    pub const WHITE: Self = Self::from_rgb(255, 255, 255);
    pub const RED: Self = Self::from_rgb(255, 0, 0);
    pub const GREEN: Self = Self::from_rgb(0, 255, 0);
    pub const BLUE: Self = Self::from_rgb(0, 0, 255);

    #[inline(always)]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[inline(always)]
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline(always)]
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline(always)]
    pub fn red(&self) -> f32 {
        self.r as f32 / 255.0
    }

    #[inline(always)]
    pub fn green(&self) -> f32 {
        self.g as f32 / 255.0
    }

    #[inline(always)]
    pub fn blue(&self) -> f32 {
        self.b as f32 / 255.0
    }

    #[inline(always)]
    pub fn alpha(&self) -> f32 {
        self.a as f32 / 255.0
    }

    #[inline(always)]
    pub fn to_color_u8(&self) -> ColorU8 {
        ColorU8 {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }

    #[inline(always)]
    pub const fn from_argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a }
    }

    #[inline(always)]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    #[inline(always)]
    pub fn to_rgb565(&self) -> u16 {
        rgb888_to_rgb565(self.r, self.g, self.b)
    }

    /// Converts this color to 16-bit RGB565 with Bayer 8x8 ordered dithering at coordinate `(x, y)`.
    #[inline(always)]
    pub fn dither_to_rgb565(&self, x: i32, y: i32) -> u16 {
        dither_rgb888_to_rgb565(self.r, self.g, self.b, x, y)
    }

    #[inline(always)]
    pub fn is_opaque(&self) -> bool {
        self.a == 255
    }

    #[inline(always)]
    pub fn is_transparent(&self) -> bool {
        self.a == 0
    }
}

/// The classic Bayer 8x8 ordered-dither matrix: the raw thresholds, each of
/// 0..=63 appearing exactly once.
pub static BAYER8: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38,
    60, 28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57,
    25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
];

/// Precomputed continuous Bayer 8x8 dither offsets: `(63.5 - BAYER8[i]) / 64.0`.
pub static DITHER_OFFSETS: [f32; 64] = [
    0.9921875, 0.4921875, 0.8671875, 0.3671875, 0.9609375, 0.4609375, 0.8359375, 0.3359375,
    0.2421875, 0.7421875, 0.1171875, 0.6171875, 0.2109375, 0.7109375, 0.0859375, 0.5859375,
    0.8046875, 0.3046875, 0.9296875, 0.4296875, 0.7734375, 0.2734375, 0.8984375, 0.3984375,
    0.0546875, 0.5546875, 0.1796875, 0.6796875, 0.0234375, 0.5234375, 0.1484375, 0.6484375,
    0.9453125, 0.4453125, 0.8203125, 0.3203125, 0.9765625, 0.4765625, 0.8515625, 0.3515625,
    0.1953125, 0.6953125, 0.0703125, 0.5703125, 0.2265625, 0.7265625, 0.1015625, 0.6015625,
    0.7578125, 0.2578125, 0.8828125, 0.3828125, 0.7890625, 0.2890625, 0.9140625, 0.4140625,
    0.0078125, 0.5078125, 0.1328125, 0.6328125, 0.0390625, 0.5390625, 0.1640625, 0.6640625,
];

/// Convert continuous floating-point RGB (0.0..=255.0) to 16-bit RGB565 with Bayer 8x8 ordered dithering.
///
/// This eliminates color banding by using 64 continuous threshold levels directly from
/// floating-point color, preventing intermediate 8-bit integer quantization banding.
/// Subpixel chromatic phase decorrelation is applied across R, G, and B to disperse
/// luminance variance.
#[inline(always)]
pub fn dither_float_to_rgb565(r: f32, g: f32, b: f32, x: i32, y: i32) -> u16 {
    let idx_r = (((y & 7) << 3) | (x & 7)) as usize;
    let idx_g = ((((y + 4) & 7) << 3) | ((x + 2) & 7)) as usize;
    let idx_b = ((((y + 2) & 7) << 3) | ((x + 4) & 7)) as usize;

    let d_r = DITHER_OFFSETS[idx_r];
    let d_g = DITHER_OFFSETS[idx_g];
    let d_b = DITHER_OFFSETS[idx_b];

    let r5 = ((r.max(0.0) * 0.125 + d_r) as u32).min(31) as u16;
    let g6 = ((g.max(0.0) * 0.250 + d_g) as u32).min(63) as u16;
    let b5 = ((b.max(0.0) * 0.125 + d_b) as u32).min(31) as u16;

    (r5 << 11) | (g6 << 5) | b5
}

/// Convert 8-bit RGB to 16-bit RGB565 with Bayer 8x8 ordered dithering.
///
/// This eliminates color banding artifacts on RGB565 displays when rendering gradients.
/// The threshold lookup is indexed by `(x & 7, y & 7)`.
#[inline(always)]
pub fn dither_rgb888_to_rgb565(r: u8, g: u8, b: u8, x: i32, y: i32) -> u16 {
    dither_float_to_rgb565(r as f32, g as f32, b as f32, x, y)
}

/// Convert 8-bit RGB to 16-bit RGB565.
#[inline(always)]
pub const fn rgb888_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | ((b as u16) >> 3)
}

/// Convert 16-bit RGB565 to 8-bit RGB tuple (R, G, B).
#[inline(always)]
pub const fn rgb565_to_rgb888(c: u16) -> (u8, u8, u8) {
    let r = (((c >> 11) & 0x1F) * 255 / 31) as u8;
    let g = (((c >> 5) & 0x3F) * 255 / 63) as u8;
    let b = ((c & 0x1F) * 255 / 31) as u8;
    (r, g, b)
}

/// High-speed alpha blend between two RGB565 pixels:
/// result = src * alpha + dst * (255 - alpha).
#[inline(always)]
pub fn blend_rgb565(dst: u16, src: u16, alpha: u8) -> u16 {
    if alpha == 255 {
        return src;
    }
    if alpha == 0 {
        return dst;
    }
    let a = alpha as u32;
    let inv_a = 255 - a;

    let d = dst as u32;
    let s = src as u32;

    let r_d = (d >> 11) & 0x1F;
    let g_d = (d >> 5) & 0x3F;
    let b_d = d & 0x1F;

    let r_s = (s >> 11) & 0x1F;
    let g_s = (s >> 5) & 0x3F;
    let b_s = s & 0x1F;

    let r = ((r_s * a + r_d * inv_a + 127) / 255) as u16;
    let g = ((g_s * a + g_d * inv_a + 127) / 255) as u16;
    let b = ((b_s * a + b_d * inv_a + 127) / 255) as u16;

    (r << 11) | (g << 5) | b
}

/// Blend an RGB888 color with alpha onto an RGB565 destination pixel.
#[inline(always)]
pub fn blend_rgb888_onto_rgb565(dst: u16, r: u8, g: u8, b: u8, alpha: u8) -> u16 {
    if alpha == 255 {
        return rgb888_to_rgb565(r, g, b);
    }
    if alpha == 0 {
        return dst;
    }
    let a = alpha as u32;
    let inv_a = 255 - a;

    let d = dst as u32;
    let r_d = (d >> 11) & 0x1F;
    let g_d = (d >> 5) & 0x3F;
    let b_d = d & 0x1F;

    let r_s = (r as u32) >> 3;
    let g_s = (g as u32) >> 2;
    let b_s = (b as u32) >> 3;

    let r_out = ((r_s * a + r_d * inv_a + 127) / 255) as u16;
    let g_out = ((g_s * a + g_d * inv_a + 127) / 255) as u16;
    let b_out = ((b_s * a + b_d * inv_a + 127) / 255) as u16;

    (r_out << 11) | (g_out << 5) | b_out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColorU8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl ColorU8 {
    #[inline(always)]
    pub const fn red(&self) -> u8 {
        self.r
    }

    #[inline(always)]
    pub const fn green(&self) -> u8 {
        self.g
    }

    #[inline(always)]
    pub const fn blue(&self) -> u8 {
        self.b
    }

    #[inline(always)]
    pub const fn alpha(&self) -> u8 {
        self.a
    }
}
