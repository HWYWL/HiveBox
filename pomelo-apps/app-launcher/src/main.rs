//! The launcher, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The board is the HAL's desktop simulator — there is no audio hardware behind this window, so the
//! music player has nothing to play. The firmware injects the ESP32-S3 board into the same
//! `app_launcher::program`.

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::Board;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    // As in the settings app's `main`: the simulator advances on `tick`, and a desktop run has no
    // firmware loop to call it. The hosted apps share this board, so the settings app's Wi-Fi page
    // scans behind this window the way it does behind the panel.
    {
        let board = Arc::clone(&board);

        std::thread::spawn(move || loop {
            board.tick();
            std::thread::sleep(Duration::from_millis(10));
        });
    }

    app_launcher::program(board).run()
}
