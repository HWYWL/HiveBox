//! The portable kernels: safe Rust, no intrinsics, correct on every target.
//!
//! This is not the "slow" path to be deleted later; it is the definition the vector path is
//! held against. Whatever `xtensa_pie` produces has to produce these pixels, and the tests in
//! [`super`] are written against the properties these two functions promise rather than against
//! the loop that currently implements them.

use crate::color::{blend_rgb565, blend_rgb565_unconditional, div255};

/// Blends one colour over a run of destination pixels.
///
/// `alpha == 255` is spelled out because it is not a blend: the answer does not depend on what
/// was in the destination, so the run is a fill and goes through the same word-at-a-time writer
/// a solid rectangle uses. `alpha == 0` is the same argument the other way, and it is cheap
/// enough to check here that no caller has to.
pub(crate) fn blend_span_rgb565(dst: &mut [u16], src: u16, alpha: u8) {
    if alpha == 0 || dst.is_empty() {
        return;
    }

    if alpha == 255 {
        crate::raster::fill_u16_slice(dst, src);
        return;
    }

    for px in dst.iter_mut() {
        *px = blend_rgb565(*px, src, alpha);
    }
}

/// Blends one colour through an 8-bit coverage mask over a run of destination pixels.
///
/// Entry `i` of `mask` is the coverage of `dst[i]` — they are indexed in step, so a run shorter
/// than the mask blends the part the mask covers and a mask shorter than the run leaves the rest
/// alone. A glyph clipped by the panel edge arrives here that way, and neither half should be a
/// panic.
///
/// # Why the loop has no `if`s in it, but the function has one
///
/// The obvious shape — skip `coverage == 0`, skip `effective == 0`, once per pixel — was written,
/// measured, and removed. It is right on the arithmetic and wrong on the machine: a glyph's mask
/// is zero across most of its bounding box with the ink in solid runs through it, so both tests are
/// branches on data whose pattern no predictor can learn, and the mispredictions cost more than the
/// two operations they were avoiding. Measured at 1.03 → 0.64 ns/pixel on this machine's
/// `blit_mask`, which is a bigger win than the exact division in [`crate::color`] was.
///
/// Both tests were removable rather than merely replaceable: `coverage == 0` and `effective == 0`
/// both come out of the blend as the destination unchanged, and `alpha == 0` is handled once
/// outside the loop. What is left is arithmetic with no decisions in it.
///
/// Taking them *out* of the pixel loop is not the same as taking them out of the kernel, though,
/// and the difference is the whole reason this function is shaped the way it is. A branch that is
/// right most of the time is worth having; a branch that is right half the time is not. Per row,
/// "is this row entirely empty" is the first kind: glyphs have empty rows above the ascender,
/// below the baseline and down both side margins — and a clipped window can be *all* empty, which
/// is a common case rather than a corner, since most of what a clip removes is the glyph's own
/// transparent margin. Measured on a run with no coverage at all in it, skipping that at one
/// branch per row beats blending it unconditionally by 2.1x, where skipping it per pixel had only
/// beaten it by 1.2x.
///
/// So: one predictable test per row, and no test at all within one. That is also exactly the shape
/// the PIE kernel needs, since a vector register holds eight pixels of a row and nothing else.
pub(crate) fn blit_mask_span(dst: &mut [u16], mask: &[u8], src: u16, alpha: u8) {
    if alpha == 0 {
        return;
    }

    // One branch per row, on a question whose answer is usually the same row to row — and which
    // short-circuits on the first inked pixel, so a row that has any ink in it pays for a byte or
    // two and not for the scan.
    if !mask.iter().any(|&coverage| coverage != 0) {
        return;
    }

    if alpha == 255 {
        // The colour is opaque, so the coverage *is* the alpha: no combining, and nothing to
        // test — a coverage of zero asks for the destination, which is what the blend returns.
        for (dst_px, &coverage) in dst.iter_mut().zip(mask) {
            *dst_px = blend_rgb565_unconditional(*dst_px, src, coverage);
        }
        return;
    }

    // A fading colour: the two alphas have to be combined first, and that combination is
    // arithmetic on the coverage rather than a test of it. Exact, since both are at most 255.
    for (dst_px, &coverage) in dst.iter_mut().zip(mask) {
        let effective = div255(alpha as u32 * coverage as u32) as u8;
        *dst_px = blend_rgb565_unconditional(*dst_px, src, effective);
    }
}
