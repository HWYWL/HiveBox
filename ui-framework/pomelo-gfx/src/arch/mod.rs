//! The pixel kernels, behind one seam.
//!
//! Every antialiased pixel on the panel goes through one of the two functions here, and they are
//! the only place where the *shape* of the loop matters: eight destination pixels blended against
//! one colour is a loop the compiler can widen, and it is not a loop the callers should each
//! reinvent. So the callers — the rectangle fillers, the rounded corners, the circle spans, the
//! glyph blit — hand over a row and a colour, and this module owns what happens to it.
//!
//! # Why a seam and not a `#[cfg]` in the callers
//!
//! The target is an ESP32-S3, whose Xtensa LX7 core has 128-bit vector registers no Rust
//! auto-vectorizer will fill, and the host is an x86 machine that runs the tests. A kernel that
//! is only correct on one of those is worse than no kernel, so the callers name these functions
//! and nothing else; which implementation answers is `cfg`'s business and is decided once,
//! below, rather than at each of the dozen call sites.
//!
//! # Where the vector version goes
//!
//! This is step two of four. The third adds
//!
//! ```text
//! #[cfg(target_arch = "xtensa")]
//! mod xtensa_pie;
//!
//! #[cfg(target_arch = "xtensa")]
//! pub(crate) use xtensa_pie::{blend_span_rgb565, blit_mask_span};
//! ```
//!
//! and writes the two kernels as PIE inline assembly, leaving [`fallback`] to every other
//! target. Until then every target compiles through it, which is what keeps the firmware
//! building while the assembly is being written — a seam that has never had a second
//! implementation behind it is the kind that turns out not to be one.

mod fallback;

pub(crate) use fallback::{blend_span_rgb565, blit_mask_span};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::blend_rgb565;
    use crate::geometry::Rect;
    use crate::pixmap::Pixmap565;
    use crate::Color;

    /// A deterministic mask: the interesting coverage values are the ends and the middle, and a
    /// hash gives every one of them without the test depending on a random seed.
    fn mask_of(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| {
                let h = (i as u32).wrapping_mul(2654435761);
                match i % 7 {
                    0 => 0,
                    1 => 255,
                    _ => (h >> 24) as u8,
                }
            })
            .collect()
    }

    fn pixmap_of(pixels: &[u16], width: u32) -> Pixmap565 {
        Pixmap565::from_vec(width, (pixels.len() as u32) / width, pixels.to_vec()).expect("pixmap")
    }

    #[test]
    fn a_span_at_full_alpha_is_a_fill_and_one_at_zero_is_nothing() {
        let original: Vec<u16> = (0..64).map(|i| (i * 977) as u16).collect();

        let mut opaque = original.clone();
        blend_span_rgb565(&mut opaque, 0xABCD, 255);
        assert!(opaque.iter().all(|&px| px == 0xABCD));

        let mut invisible = original.clone();
        blend_span_rgb565(&mut invisible, 0xABCD, 0);
        assert_eq!(invisible, original);
    }

    #[test]
    fn a_span_matches_the_pixel_at_a_time_blend_it_replaced() {
        let original: Vec<u16> = (0..64).map(|i| (i * 977) as u16).collect();

        for alpha in 1..255u8 {
            let mut span = original.clone();
            blend_span_rgb565(&mut span, 0x3C9F, alpha);

            let expected: Vec<u16> = original
                .iter()
                .map(|&px| blend_rgb565(px, 0x3C9F, alpha))
                .collect();

            assert_eq!(span, expected, "alpha {alpha}");
        }
    }

    /// The opaque short-circuit has to be invisible: the same coverage through the general path
    /// and through the one that skips combining the alphas must land on the same pixels.
    #[test]
    fn the_opaque_mask_path_is_the_general_path_at_255() {
        let original: Vec<u16> = (0..256).map(|i| (i * 251) as u16).collect();
        let mask = mask_of(original.len());

        let mut through_the_shortcut = original.clone();
        blit_mask_span(&mut through_the_shortcut, &mask, 0x07E0, 255);

        let mut through_the_general = original.clone();
        for (px, &coverage) in through_the_general.iter_mut().zip(&mask) {
            let effective = ((255u32 * coverage as u32) / 255) as u8;
            if effective > 0 {
                *px = blend_rgb565(*px, 0x07E0, effective);
            }
        }

        assert_eq!(through_the_shortcut, through_the_general);
    }

    /// A partly transparent colour still has to be *blended* through the mask, and the masked
    /// value is the one the division produced.
    #[test]
    fn a_mask_blend_matches_the_division_it_replaced() {
        let original: Vec<u16> = (0..256).map(|i| (i * 251) as u16).collect();
        let mask = mask_of(original.len());

        for alpha in [1u8, 2, 64, 128, 200, 254] {
            let mut got = original.clone();
            blit_mask_span(&mut got, &mask, 0xF81F, alpha);

            let expected: Vec<u16> = original
                .iter()
                .zip(&mask)
                .map(|(&px, &coverage)| {
                    if coverage == 0 {
                        return px;
                    }
                    let effective = (((alpha as u32) * (coverage as u32)) / 255) as u8;
                    if effective == 0 {
                        px
                    } else {
                        blend_rgb565(px, 0xF81F, effective)
                    }
                })
                .collect();

            assert_eq!(got, expected, "alpha {alpha}");
        }
    }

    /// Coverage of zero leaves the destination alone, which is most of a glyph's bounding box.
    #[test]
    fn zero_coverage_touches_nothing() {
        let original: Vec<u16> = (0..32).map(|i| (i * 4099) as u16).collect();
        let mut got = original.clone();
        blit_mask_span(&mut got, &[0; 32], 0xFFFF, 128);
        assert_eq!(got, original);
    }

    /// A mask shorter than the run blends what it covers. Clipping a glyph at the panel edge
    /// produces exactly this, and it should not panic or write past the mask.
    #[test]
    fn a_short_mask_blends_what_it_covers_and_stops() {
        let original: Vec<u16> = vec![0x0000; 16];
        let mut got = original.clone();
        blit_mask_span(&mut got, &[255; 4], 0xFFFF, 255);

        assert!(got[..4].iter().all(|&px| px == 0xFFFF));
        assert!(got[4..].iter().all(|&px| px == 0x0000));
    }

    /// The refactor that turned `raster::blit_mask` into one call per row must not have moved a
    /// pixel — including where the clip cuts the glyph, which is the case the row slicing made
    /// different from the pixel loop it replaced.
    #[test]
    fn the_row_slicing_in_blit_mask_is_transparent() {
        const W: u32 = 24;
        const H: u32 = 12;

        // A mask whose coverage is a function of position, so a row read at the wrong offset
        // cannot coincidentally look right.
        let mask: Vec<u8> = (0..W * H).map(|i| ((i * 37) % 251) as u8).collect();
        let base: Vec<u16> = (0..W * H).map(|i| (i * 811) as u16).collect();

        let cases = [
            (0i32, 0i32, None),
            (2, 3, None),
            (-4, -2, None),
            (18, 8, None),
            (2, 3, Some(Rect::from_ltwh(4.0, 4.0, 12.0, 6.0))),
            (-4, -2, Some(Rect::from_ltwh(0.0, 0.0, 10.0, 10.0))),
        ];

        for &(x, y, clip) in &cases {
            for alpha in [255u8, 128, 1] {
                let mut pixmap = pixmap_of(&base, W);
                crate::raster::blit_mask(&mut pixmap.as_mut(), clip, x, y, W, H, &mask, Color::from_rgba(0xFF, 0x00, 0xFF, alpha));

                // The same clipping, spelled out, with the per-pixel blend the old loop used.
                let pix_w = W as i32;
                let pix_h = H as i32;
                let clip_box = clip.unwrap_or(Rect::from_ltwh(0.0, 0.0, pix_w as f32, pix_h as f32));
                let cx1 = (clip_box.x.floor() as i32).max(0);
                let cy1 = (clip_box.y.floor() as i32).max(0);
                let cx2 = (clip_box.right().ceil() as i32).min(pix_w);
                let cy2 = (clip_box.bottom().ceil() as i32).min(pix_h);

                let x1 = x.max(cx1);
                let y1 = y.max(cy1);
                let x2 = (x + W as i32).min(cx2);
                let y2 = (y + H as i32).min(cy2);

                let mut expected = base.clone();
                for py in y1.max(0)..y2 {
                    for px in x1.max(0)..x2 {
                        let coverage = mask[(py - y) as usize * W as usize + (px - x) as usize];
                        if coverage == 0 {
                            continue;
                        }
                        let effective = ((alpha as u32 * coverage as u32) / 255) as u8;
                        if effective > 0 {
                            let idx = py as usize * W as usize + px as usize;
                            expected[idx] = blend_rgb565(expected[idx], Color::from_rgb(0xFF, 0x00, 0xFF).to_rgb565(), effective);
                        }
                    }
                }

                assert_eq!(
                    pixmap.data().to_vec(),
                    expected,
                    "x {x} y {y} clip {clip:?} alpha {alpha}"
                );
            }
        }
    }
}
