//! `cargo run` — the battery log as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`PowerLog::new`], then owns the loop. The board is the HAL's desktop
//! simulator, whose gauge and PMIC temperature both move on their own — the temperature swings either
//! side of the bench board's idle reading — so a window left open draws a curve rather than a flat
//! line, and the high-temperature band is something that can actually be reached without warming a
//! chip up.
//!
//! The log file is written at `.pomelo/AppData/POWER/history.csv` under the current directory, the
//! desktop's stand-in for the board's `/internal/AppData/POWER/history.csv`. Deleting it is how a
//! first-run window is got back.
//!
//! The subscription is the app's own ([`PowerLog::subscription`]) and it is the one thing the app
//! cannot do for itself: a window's clock is its frames, where the board's is the launcher's pump.

use std::sync::Arc;

use pomelo_hal::Board;
use power_log::PowerLog;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || PowerLog::new(Arc::clone(&board)),
        PowerLog::update,
        PowerLog::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(PowerLog::theme)
    .subscription(PowerLog::subscription)
    .run()
}
