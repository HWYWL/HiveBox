//! The counter, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The whole of it: iced builds the state with `Default`, then owns the loop.

use demo_counter::Counter;

fn main() -> iced::Result {
    iced::application(Counter::new, Counter::update, Counter::view)
        .theme(Counter::theme)
        .run()
}
