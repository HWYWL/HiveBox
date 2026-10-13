//! What a pixel costs, per kernel, on whatever machine this is run on.
//!
//! ```text
//! cargo run -p pomelo-gfx --release --features bench --example bench_spans
//! ```
//!
//! # Why this lives in the crate and not in `benches/`
//!
//! For the same reason it is not a `criterion` benchmark: this crate has no dependencies, and a
//! harness is a dependency. But there is a second reason, and it is the one that shaped the module.
//! The number that decides whether `arch::xtensa_pie` is worth writing has to come from the *device*,
//! and the only way the device's number and the host's number are comparable is if the same code
//! took both of them. An `examples/` binary cannot be linked into the firmware — the firmware's
//! CMake builds `rust_main` and nothing else — so the measurement had to move down here, where both
//! sides can reach it.
//!
//! # Reading the numbers
//!
//! On a host, treat them as a ratio chart and not as a specification. The panel's frame buffer is
//! in PSRAM and a host's is in the cache, so an absolute figure here is optimistic by an amount that
//! depends on the machine — the same caveat the README puts on its own figures. What transfers is
//! the *ordering* and the arithmetic: a kernel that costs twice another one here because of the work
//! it does per pixel is likely to cost twice there too.
//!
//! On the device the absolute figures are the point, and they are the ones to compare against the
//! `[phase]` line the `render-debug` feature prints for real frames. If a full-screen `fill_rect`
//! measures tens of nanoseconds per pixel here but the device's `clear` phase says otherwise, the
//! difference is memory and not arithmetic, and that is the answer that decides the vector work.
//!
//! `--release` is not optional. A debug build measures the iterator adapters.

use std::hint::black_box;
use std::time::{Duration, Instant};

use crate::{Canvas, Color, Pixmap565, RRect, Radius, Rect};

/// The panel this targets.
pub const WIDTH: u32 = 480;
/// The panel this targets.
pub const HEIGHT: u32 = 480;

/// One measured kernel: what it drew, and what that cost.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    name: &'static str,
    pixels: u64,
    seconds: f64,
}

impl Reading {
    /// The kernel's name, as printed.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// How many destination pixels the timed work covered.
    pub fn pixels(&self) -> u64 {
        self.pixels
    }

    /// Seconds per call of the timed body — one fill, or one batch of sixty blits.
    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    pub fn nanos_per_pixel(&self) -> f64 {
        self.seconds / self.pixels as f64 * 1e9
    }

    /// What a full 480x480 of this kernel would cost, which is the column the profile's own
    /// `[phase]` line is read against.
    pub fn millis_per_full_frame(&self) -> f64 {
        self.nanos_per_pixel() * (WIDTH * HEIGHT) as f64 / 1e6
    }
}

/// How much work each timed run aims to contain, once a single call has been timed.
const BUDGET: Duration = Duration::from_millis(50);

/// A ceiling on iterations, for a kernel so cheap that the budget would ask for millions.
const MAX_ITERATIONS: u32 = 200_000;

/// Seconds per call of `body`, warmed up, taking the best of a few runs.
///
/// The best run rather than the mean, because both hosts and devices are noisy places to measure a
/// microsecond in and an interrupt during a frame only ever makes it look slower. Nothing that
/// matters can hide in that: every kernel here runs for hundreds of times the clock's own
/// resolution.
///
/// # Why the iteration count is derived and not passed in
///
/// It was a parameter — a divisor applied to counts tuned on a host — and it was wrong, in the way
/// that a constant chosen from one machine is always wrong. The kernels here differ in cost by
/// three orders of magnitude from each other, and every one of them is three to four orders of
/// magnitude more expensive on the panel than on a host. A single divisor cannot be right for both
/// ends of that spread: on the device one that made the expensive kernels tolerable left the cheap
/// ones measured a handful of times.
///
/// Worse, it was a *fixed* divisor, so it was only ever right for one pair of machines. The count
/// came out wrong in a second way: a `blit_mask` call is six hundred blits, which on the device is
/// a tenth of a second, so forty of them is seventeen seconds of tight loop with no yield in it —
/// long enough to starve the idle task and trip the task watchdog mid-boot. That is a defect the
/// numbers do not show and the console does.
///
/// Timing one call and then asking for `BUDGET` worth of them fixes both without a constant: the
/// run is as long as it needs to be to resolve the clock, and never much longer. The worst case is
/// bounded by the budget itself, so no kernel can hold a core for longer than a few hundred
/// milliseconds, which is what keeps the watchdog off a diagnostic it has no business firing on.
fn seconds_per_call(budget: Duration, mut body: impl FnMut()) -> f64 {
    body();
    body();

    let start = Instant::now();
    body();
    let one = start.elapsed().as_secs_f64();

    let iterations = if one <= 0.0 {
        MAX_ITERATIONS
    } else {
        ((budget.as_secs_f64() / one) as u32).clamp(1, MAX_ITERATIONS)
    };

    let mut best = f64::INFINITY;

    for _ in 0..3 {
        let start = Instant::now();
        for _ in 0..iterations {
            body();
        }
        best = best.min(start.elapsed().as_secs_f64() / f64::from(iterations));
    }

    best
}

/// A coverage mask with the shape a glyph's has: mostly nothing, a solid stem, antialiased
/// shoulders. Spread over the whole 0..=255 range, so the blend is never given only one of the
/// cases it has to handle.
///
/// `floor` knocks out every pixel below it, which is the knob the mask kernel actually cares
/// about: how often the coverage is zero decides whether a branch on it is predictable, and that
/// is what decides how the kernel has to be written. `floor = 0` is the mask as drawn,
/// `floor = 200` is a hairline glyph, and a mask that is zero everywhere is the other end —
/// which is not a degenerate case but the ordinary result of clipping a glyph to a panel edge.
fn mask_of(w: u32, h: u32, floor: u8) -> Vec<u8> {
    let mut mask = vec![0u8; (w * h) as usize];

    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let ink = (1.0 - ((fx - 0.4).abs() * 4.0).min(1.0)) * (1.0 - fy * fy);

            let coverage = (ink.clamp(0.0, 1.0) * 255.0) as u8;
            mask[(y * w + x) as usize] = if coverage < floor { 0 } else { coverage };
        }
    }

    mask
}

/// Runs every kernel and returns what each cost, or `None` if the frame buffer could not be
/// allocated.
///
/// Every kernel is timed for the same wall-clock budget rather than for a fixed number of
/// iterations, which is what lets the host and the device call this identically. See
/// [`seconds_per_call`] for why the count has to be derived rather than chosen.
pub fn readings() -> Option<Vec<Reading>> {
    // A frame buffer that is neither all black nor all white, so no kernel gets a shortcut it
    // would not get on a device.
    let mut pixmap = Pixmap565::new(WIDTH, HEIGHT)?;
    for (i, px) in pixmap.data_mut().iter_mut().enumerate() {
        *px = (i as u16).wrapping_mul(2654).wrapping_add(0x1084);
    }

    let full = Rect::from_ltwh(0.0, 0.0, WIDTH as f32, HEIGHT as f32);
    let glyph = mask_of(28, 24, 0);
    let hairline = mask_of(28, 24, 200);
    let large = mask_of(400, 400, 0);
    let full_screen_pixels = (WIDTH * HEIGHT) as u64;

    let mut readings = Vec::new();

    {
        let mut canvas = Canvas::new(pixmap.as_mut());

        readings.push(Reading {
            name: "fill_rect opaque",
            pixels: full_screen_pixels,
            seconds: seconds_per_call(BUDGET, || {
                canvas.draw_rect(full, Color::from_rgb(0x20, 0x80, 0xC0));
            }),
        });

        readings.push(Reading {
            name: "fill_rect alpha 180",
            pixels: full_screen_pixels,
            seconds: seconds_per_call(BUDGET, || {
                canvas.draw_rect(full, Color::from_rgba(0x20, 0x80, 0xC0, 180));
            }),
        });

        // A pill: the straight middle is the span kernel and the caps are the SDF, which is the
        // mixture a rounded button is actually made of.
        let pill = RRect::from_rect_radius(
            Rect::from_ltwh(20.0, 180.0, 440.0, 120.0),
            Radius::circular(60.0),
        );

        readings.push(Reading {
            name: "fill_rrect opaque",
            pixels: full_screen_pixels,
            seconds: seconds_per_call(BUDGET, || {
                canvas.draw_rrect(pill, Color::from_rgb(0xC0, 0x40, 0x20));
            }),
        });

        // The glyph kernel at the two alphas that take different branches: opaque text goes
        // through the path that skips combining the two alphas, a fading label does not.
        let text_pixels = 600 * glyph.len() as u64;

        readings.push(Reading {
            name: "blit_mask opaque (text)",
            pixels: text_pixels,
            seconds: seconds_per_call(BUDGET, || {
                for i in 0..600 {
                    canvas.blit_mask(
                        (i % 15) as i32 * 30,
                        ((i / 15) % 20) as i32 * 24,
                        28,
                        24,
                        black_box(&glyph),
                        Color::WHITE,
                    );
                }
            }),
        });

        readings.push(Reading {
            name: "blit_mask alpha 180",
            pixels: text_pixels,
            seconds: seconds_per_call(BUDGET, || {
                for i in 0..600 {
                    canvas.blit_mask(
                        (i % 15) as i32 * 30,
                        ((i / 15) % 20) as i32 * 24,
                        28,
                        24,
                        black_box(&glyph),
                        Color::from_rgba(0xFF, 0xFF, 0xFF, 180),
                    );
                }
            }),
        });

        // The same kernel with almost nothing to blend. This is not a corner case: thinning a
        // glyph's coverage is what a light font weight is, and it is also what a clip does when
        // it cuts a glyph's transparent margin away — which is most of what a clip ever cuts.
        // It is the row where the kernel's branch has an easy time of it, and it is here to keep
        // the row above from being read as a win that holds everywhere.
        readings.push(Reading {
            name: "blit_mask hairline ink",
            pixels: text_pixels,
            seconds: seconds_per_call(BUDGET, || {
                for i in 0..600 {
                    canvas.blit_mask(
                        (i % 15) as i32 * 30,
                        ((i / 15) % 20) as i32 * 24,
                        28,
                        24,
                        black_box(&hairline),
                        Color::WHITE,
                    );
                }
            }),
        });

        // A 400x400 glyph under a 40x40 clip, which is the small-dirty-region case that decides
        // whether a scrolling list feels smooth — and, as the numbers show, a case where the
        // window the clip leaves is *entirely empty coverage*. Reading it as "40x40 of blending"
        // would be wrong twice over: nothing is blended, and what is measured is how cheaply the
        // kernel can decide that. That is the whole justification for the row-level empty test in
        // `arch::fallback::blit_mask_span`, and the reason this row exists at all.
        //
        // The clip is set once, outside the timed loop, so what is measured is the blit and not
        // `save`/`restore`.
        const BLITS_PER_CALL: u64 = 40;
        const VISIBLE: u64 = 40 * 40;

        canvas.save();
        canvas.clip_rect(Rect::from_ltwh(0.0, 0.0, 40.0, 40.0));

        readings.push(Reading {
            name: "blit_mask over empty coverage",
            pixels: BLITS_PER_CALL * VISIBLE,
            seconds: seconds_per_call(BUDGET, || {
                for _ in 0..BLITS_PER_CALL {
                    canvas.blit_mask(0, 0, 400, 400, black_box(&large), Color::WHITE);
                }
            }),
        });

        canvas.restore();
    }

    black_box(pixmap.data());

    Some(readings)
}

/// Runs every kernel and prints the table.
///
/// Prints rather than panics when the frame buffer does not fit: a diagnostic on a device must not
/// be the thing that takes the device down, and the firmware calls this at boot, where a panic is a
/// boot loop.
pub fn report() {
    let Some(readings) = readings() else {
        println!("[bench] no room for a {WIDTH}x{HEIGHT} RGB565 frame buffer; skipped");
        return;
    };

    println!("pomelo-gfx span kernels on {WIDTH}x{HEIGHT} RGB565, best of 3 runs");
    println!();
    println!(
        "{:<31} {:>11} {:>16} {:>13}",
        "kernel", "ns/pixel", "ms/full frame", "px measured"
    );
    println!("{}", "-".repeat(74));

    for r in &readings {
        println!(
            "{:<31} {:>11.3} {:>16.2} {:>13}",
            r.name,
            r.nanos_per_pixel(),
            r.millis_per_full_frame(),
            r.pixels
        );
    }

    println!();
    println!("The three blit_mask rows are one kernel at three ink densities, and they are not");
    println!("ordered the way the kernels above them are. On a mask with ink spread through it the");
    println!("ns/pixel is a blend cost; on the empty-coverage row almost nothing is blended, so its");
    println!("ms/full-frame column is the cost of *deciding* not to blend and is not a frame time.");
    println!("Read the blit rows against each other, not against the fills.");
    println!();
    println!("What that spread of densities is for: a branch on the coverage is worth having when the");
    println!("answer is nearly always the same and worth removing when it is not, and which of those");
    println!("holds depends on the mask. The kernel resolves it by having no branch per pixel and one");
    println!("per row, so the empty-coverage row is fast for a different reason than the text row is.");
    println!();
    println!("Host figures order the work; the device is what judges it. See `arch::mod`.");
}
