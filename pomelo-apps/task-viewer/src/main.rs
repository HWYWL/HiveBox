//! `cargo run` — the task viewer as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`TaskViewer::new`], then owns the loop. The board is the HAL's desktop
//! simulator — the firmware injects the ESP32-S3 one into the same `TaskViewer::new` — so the list
//! here is the simulator's set of tasks: the names this board really runs, with the readings moving
//! the way `SimSystem` moves them.
//!
//! The subscription is the one thing this file does that the app cannot do for itself, and it is the
//! app's own ([`TaskViewer::subscription`]): a window's clock is its frames, where the board's is the
//! launcher's pump. The app throttles either of them to one reading a second.

use std::sync::Arc;

use pomelo_hal::Board;
use task_viewer::TaskViewer;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || TaskViewer::new(Arc::clone(&board)),
        TaskViewer::update,
        TaskViewer::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(TaskViewer::theme)
    .subscription(TaskViewer::subscription)
    .run()
}
