//! What a pixel costs, per kernel, on whatever machine this is run on.
//!
//! ```text
//! cargo run -p pomelo-gfx --release --features bench --example bench_spans
//! ```
//!
//! This binary is a shell. The measurement itself is in [`pomelo_gfx::bench`], because the number
//! that decides whether the vector work is worth doing has to come from the device, and the firmware
//! links the crate rather than an example. Keeping one copy of the kernels is what makes the two
//! sets of figures comparable; a second copy here would be a second measurement.
//!
//! Read the module's docs for what the figures mean and why `--release` is not optional.

fn main() {
    pomelo_gfx::bench::report();
}
