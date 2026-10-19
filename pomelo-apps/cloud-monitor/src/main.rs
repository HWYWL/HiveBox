//! `cargo run` — the cloud account as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`CloudMonitor::new`], then owns the loop. The board is the HAL's
//! desktop simulator, which is the whole point of the window: its account has both products set up,
//! two servers on one and one on the other, and one of the two is stopped — so there is a list to
//! read, a row to pick, and a power button with something to do.
//!
//! The simulator only advances when somebody calls `tick`, and a desktop run has no firmware loop to do
//! it. The device's own backend is event-driven and its `tick` is a no-op, so this is a desktop-only
//! concern — which is why it lives in `main` and not in the app. Ten milliseconds rather than the
//! board's second: a desktop frame is the clock here, and the round trip is measured in the app's own
//! ticks.
//!
//! The subscription is the app's own ([`CloudMonitor::subscription`]) and it is the one thing the app
//! cannot do for itself: a window's clock is its frames, where the board's is the launcher's pump. The
//! app asks for them only while something is on its way — see [`CloudMonitor::waiting`].

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::Board;
use cloud_monitor::CloudMonitor;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    {
        let board = Arc::clone(&board);

        std::thread::spawn(move || loop {
            board.tick();
            std::thread::sleep(Duration::from_millis(10));
        });
    }

    iced::application(
        move || CloudMonitor::new(Arc::clone(&board)),
        CloudMonitor::update,
        CloudMonitor::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(CloudMonitor::theme)
    .subscription(CloudMonitor::subscription)
    .run()
}
