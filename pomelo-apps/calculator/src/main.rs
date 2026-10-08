//! The calculator, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! This is the whole of it. `iced::run` builds the state with `Default`, then owns the loop: the
//! window or the panel, the event queue, the renderer and the frames. Which platform it gets those
//! from is decided by whoever builds the crate: here it is iced's own `iced_winit` over winit, and
//! a window. In the firmware's graph the same `.run()` lands on `vendor/iced-pomelo-winit` (the
//! repository patches `iced_winit`) and drives the board's panel instead — but nothing *runs* an
//! app on its own there: the launcher hosts it as a widget inside the image.

use calculator::Calculator;

fn main() -> iced::Result {
    iced::application(Calculator::default, Calculator::update, Calculator::view)
        .theme(Calculator::theme)
        .run()
}
