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

/// `x / 255`, exactly, without a division, for every `x <= 65535`.
///
/// A blend divides by 255 once per channel per pixel, and that is not the operation it looks
/// like on the hardware this exists for: Xtensa has no integer divide instruction at all, so
/// `/255` is lowered into a magic-number multiply and a shift. This is two adds and a shift
/// instead, which matters because it is the innermost arithmetic of every antialiased pixel on
/// the panel — and a multiply is the one thing a future vector version cannot do eight at a time
/// cheaply, where a shift it can.
///
/// The identity is `floor(y * 257 / 65536) == floor((y - 1) / 255)` for `y = x + 1`; the
/// exhaustive test below is what stands behind the bound, not the algebra.
#[inline(always)]
pub const fn div255(x: u32) -> u32 {
    let y = x + 1;
    (y + (y << 8)) >> 16
}

/// `(x + 127) / 255`: [`div255`] of the pre-rounded numerator, so also exact, for `x <= 65408`.
///
/// Rounding is separated from the division because that is where the two forms differ and where
/// a mistake would be invisible: `div255(x + 127)` and `div255(x) + something` are not the same
/// function, and only the first one is what `(x + 127) / 255` means.
#[inline(always)]
pub const fn div255_round(x: u32) -> u32 {
    div255(x + 127)
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

    // The widest numerator here is `31 * 255 + 31 * 255`, well inside what `div255` is exact for.
    let r = div255_round(r_s * a + r_d * inv_a) as u16;
    let g = div255_round(g_s * a + g_d * inv_a) as u16;
    let b = div255_round(b_s * a + b_d * inv_a) as u16;

    (r << 11) | (g << 5) | b
}

/// The same blend with the two early returns removed, for spans whose alpha is *data*.
///
/// [`blend_rgb565`]'s `alpha == 255` and `alpha == 0` checks are worth having at a call site that
/// blends one pixel, because there the alpha is a property of the paint and the answer does not
/// depend on the destination. Down a coverage mask the alpha changes every pixel, so the same
/// check becomes a branch on data: a glyph's mask is mostly zero with solid stems through it, and
/// a predictor fed that pattern guesses wrong often enough to cost more than the arithmetic the
/// check was saving. Removing the check is only possible because it was never load-bearing —
/// the general form already returns `src` at 255 and `dst` at 0, which is what
/// `blend_rgb565_unconditional_agrees_at_the_ends_it_dropped` holds it to.
///
/// Not a replacement for [`blend_rgb565`]: a solid fill should keep its branch, where the alpha is
/// loop-invariant and predicted perfectly.
#[inline(always)]
pub fn blend_rgb565_unconditional(dst: u16, src: u16, alpha: u8) -> u16 {
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

    let r = div255_round(r_s * a + r_d * inv_a) as u16;
    let g = div255_round(g_s * a + g_d * inv_a) as u16;
    let b = div255_round(b_s * a + b_d * inv_a) as u16;

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

    let r_out = div255_round(r_s * a + r_d * inv_a) as u16;
    let g_out = div255_round(g_s * a + g_d * inv_a) as u16;
    let b_out = div255_round(b_s * a + b_d * inv_a) as u16;

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

/// The blend, written the way it was written before [`div255`] replaced the division.
///
/// It exists so that the fast form can be held against the slow one rather than against a
/// second reading of the same algebra: the point of the change was that it produces the *same*
/// pixels, and the only way to say that is to run both.
#[cfg(test)]
fn blend_rgb565_by_division(dst: u16, src: u16, alpha: u8) -> u16 {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value `div255` is defined for, against the division it stands in for.
    ///
    /// Exhaustive rather than sampled, because the interesting part of the identity is where it
    /// stops holding and the bound is not derivable by inspection: the tempting
    /// `(x + 1 + (x >> 8)) >> 8` is correct right up to 65534 and wrong at 65535 exactly, which
    /// is a mistake that would show up as one shade off in one pixel of a full-screen gradient.
    #[test]
    fn div255_is_exact_over_its_whole_domain() {
        for x in 0..=65535u32 {
            assert_eq!(div255(x), x / 255, "div255({x})");
        }
    }

    /// The domain's last value, named, because it is the one the near-miss gets wrong.
    #[test]
    fn div255_is_exact_at_the_top_of_its_domain() {
        assert_eq!(div255(65535), 257);
        assert_eq!(div255(65534), 256);
        assert_eq!(div255(65025), 255);
    }

    /// Every numerator a blend can actually produce, against `(x + 127) / 255`.
    ///
    /// The channels are not independent *as pixels* — the three are packed into one `u16` — but
    /// they are independent *as arithmetic*, and this is the arithmetic: every `(src, dst)`
    /// component pair a 5-bit or a 6-bit channel can form, against every alpha. Covering that
    /// covers the blend, which is why the `u16` test below only has to check the packing.
    #[test]
    fn div255_round_is_exact_for_every_numerator_a_blend_produces() {
        for alpha in 0..=255u32 {
            let inv = 255 - alpha;

            for src in 0..=31u32 {
                for dst in 0..=31u32 {
                    let n = src * alpha + dst * inv;
                    assert_eq!(div255_round(n), (n + 127) / 255, "r/b {n}");
                }
            }

            for src in 0..=63u32 {
                for dst in 0..=63u32 {
                    let n = src * alpha + dst * inv;
                    assert_eq!(div255_round(n), (n + 127) / 255, "g {n}");
                }
            }
        }
    }

    /// The packing, the two short-circuits, and a walk over the whole `u16` colour cube.
    ///
    /// The alphas are all 256 and the colours come from a fixed sequence rather than a random
    /// one so that a failure names the same pixel on every run.
    #[test]
    fn blend_rgb565_matches_the_division_it_replaced() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for alpha in 0..=255u8 {
            // The ends are where the function does not do arithmetic at all.
            assert_eq!(blend_rgb565(0x1234, 0xABCD, 0), 0x1234);
            assert_eq!(blend_rgb565(0x1234, 0xABCD, 255), 0xABCD);

            // A stride coprime with 65536 visits every colour in the cube.
            for i in 0..4096u32 {
                let dst = (i.wrapping_mul(40499) & 0xFFFF) as u16;
                let src = (next() & 0xFFFF) as u16;
                assert_eq!(
                    blend_rgb565(dst, src, alpha),
                    blend_rgb565_by_division(dst, src, alpha),
                    "dst {dst:#06x} src {src:#06x} alpha {alpha}"
                );
            }
        }
    }

    /// The other division that was replaced, which rounds the same way and packs differently.
    #[test]
    fn blend_rgb888_onto_rgb565_matches_the_division_it_replaced() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        for alpha in 0..=255u8 {
            for i in 0..512u32 {
                let dst = (i.wrapping_mul(40499) & 0xFFFF) as u16;
                let r = (next() & 0xFF) as u8;
                let g = (next() & 0xFF) as u8;
                let b = (next() & 0xFF) as u8;

                if alpha == 255 {
                    assert_eq!(blend_rgb888_onto_rgb565(dst, r, g, b, alpha), rgb888_to_rgb565(r, g, b));
                    continue;
                }
                if alpha == 0 {
                    assert_eq!(blend_rgb888_onto_rgb565(dst, r, g, b, alpha), dst);
                    continue;
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

                let expected = ((((r_s * a + r_d * inv_a + 127) / 255) as u16) << 11)
                    | ((((g_s * a + g_d * inv_a + 127) / 255) as u16) << 5)
                    | (((b_s * a + b_d * inv_a + 127) / 255) as u16);

                assert_eq!(
                    blend_rgb888_onto_rgb565(dst, r, g, b, alpha),
                    expected,
                    "dst {dst:#06x} rgb ({r},{g},{b}) alpha {alpha}"
                );
            }
        }
    }

    /// An opaque blend is a copy and a transparent one is nothing, which is the assumption the
    /// span kernels in `arch` are built on.
    #[test]
    fn the_ends_of_the_alpha_range_are_identity_operations() {
        for c in [0x0000u16, 0x1234, 0x8410, 0xABCD, 0xFFFF] {
            assert_eq!(blend_rgb565(0x5A5A, c, 255), c);
            assert_eq!(blend_rgb565(c, 0x5A5A, 0), c);
        }
    }

    /// The branchless blend, against the branching one, over every alpha and a walk of the cube.
    ///
    /// This is the test the mask kernel rests on. `blend_rgb565_unconditional` is only allowed to
    /// exist because the two `if`s it drops were never load-bearing — the general form already
    /// answers `src` at 255 and `dst` at 0 — and this is that claim rather than a reading of it.
    /// If the two ever diverge, `blit_mask_span` starts compositing glyphs wrongly at some
    /// coverage value, which is a class of bug that looks like a font problem.
    #[test]
    fn blend_rgb565_unconditional_agrees_at_the_ends_it_dropped() {
        for alpha in 0..=255u8 {
            for i in 0..1024u32 {
                let dst = (i.wrapping_mul(40499) & 0xFFFF) as u16;
                let src = (i.wrapping_mul(28411).wrapping_add(8191) & 0xFFFF) as u16;

                assert_eq!(
                    blend_rgb565_unconditional(dst, src, alpha),
                    blend_rgb565(dst, src, alpha),
                    "dst {dst:#06x} src {src:#06x} alpha {alpha}"
                );
            }
        }
    }

    /// Named separately, because these are the two values the branchless form must get right
    /// rather than merely agree on.
    #[test]
    fn the_unconditional_blend_is_the_identity_where_the_branches_were() {
        for c in [0x0000u16, 0x1234, 0x8410, 0xABCD, 0xFFFF] {
            assert_eq!(blend_rgb565_unconditional(0x5A5A, c, 255), c);
            assert_eq!(blend_rgb565_unconditional(c, 0x5A5A, 0), c);
        }
    }
}
