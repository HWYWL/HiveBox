//! `cargo run` — the clock as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`Clock::new`], then owns the loop. The board is the HAL's desktop
//! simulator, which is what makes this window worth opening without the panel: the simulator's clock is
//! set, so the dial turns, and a pomodoro counts down against a real uptime.
//!
//! The subscription is the one thing the app cannot do for itself, and it is the app's own
//! ([`Clock::subscription`]): a window's clock is its frames, where the board's is the launcher's pump.
//! Unlike the task viewer the app does not throttle them, because a second hand moving sixty times a
//! second is the thing a clock is for.

use std::sync::Arc;

use clock::Clock;
use pomelo_hal::Board;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || Clock::new(Arc::clone(&board)),
        Clock::update,
        Clock::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Clock::theme)
    .subscription(Clock::subscription)
    .run()
}
