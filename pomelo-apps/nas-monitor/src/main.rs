//! `cargo run` — the NAS monitor as a desktop window, watching the simulator.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`NasMonitor::new`], then owns the loop. The board is the HAL's desktop
//! simulator, whose NAS answers with numbers that look like a four-bay box's — so what is on the panel
//! here is what the page will look like on the board, minus the machine.
//!
//! The simulator is *pointed at a NAS* here, which the device's board is not: a board knows its NAS
//! because somebody wrote it down (see `pomelo_hal::nas_credentials`), and a desktop window that
//! nobody has configured would only ever draw the empty state. That is the one thing this file does
//! beyond starting the loop, and it is the demo's whole job.
//!
//! The subscription is the app's own ([`NasMonitor::subscription`]): a window's clock is its frames,
//! where the board's is the launcher's pump. The app throttles either of them to the interval the
//! credentials name.

use std::sync::Arc;

use nas_monitor::NasMonitor;
use pomelo_hal::{Board, NasCredentials};

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    board
        .nas()
        .remember(&NasCredentials {
            host: String::from("192.168.1.10"),
            port: 22,
            user: String::from("nasstat"),
            password: String::from("hunter2"),
            interval_secs: 5,
            host_key: None,
        })
        .expect("the simulator takes a NAS");

    iced::application(
        move || NasMonitor::new(Arc::clone(&board)),
        NasMonitor::update,
        NasMonitor::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(NasMonitor::theme)
    .subscription(NasMonitor::subscription)
    .run()
}
