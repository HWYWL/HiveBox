//! The web manager, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with `WebManager::new`, then owns the loop. The board is the HAL's desktop
//! simulator — the firmware injects the ESP32-S3 one into the same `WebManager::new` — so the switch
//! flips a simulated status here: the same page, the same address rule, no server behind either.

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::Board;
use web_manager::WebManager;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    // The simulator only advances when somebody calls `tick`, and a desktop run has no firmware loop
    // to do it. The device's own backends are event-driven and their `tick` is a no-op, so this is a
    // desktop-only concern — which is why it lives in `main` and not in the app.
    {
        let board = Arc::clone(&board);

        std::thread::spawn(move || loop {
            board.tick();
            std::thread::sleep(Duration::from_millis(10));
        });
    }

    iced::application(
        move || WebManager::new(Arc::clone(&board)),
        WebManager::update,
        WebManager::view,
    )
    .theme(WebManager::theme)
    // The state badge and the note's glyph are Material Symbols. The launcher installs this font for
    // every app it hosts (`app_launcher::program`); a standalone run has to install it itself.
    .font(pomelo_material_symbols::FONT)
    .run()
}
