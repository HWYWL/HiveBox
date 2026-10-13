//! `cargo run` — the list of games, and whichever of them is played, as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The shortest `main` in the workspace, and the difference is worth saying out loud: there is nothing
//! to poll and nothing to pump. The Wi-Fi and weather pages are requests, and a request needs a clock —
//! frames in a window, the launcher's pump on the board — so their demos carry a tick thread and their
//! apps ask for a subscription. 2048 asks the board exactly one question, once, when it starts (how
//! long have you been up, which is how it decides which board to deal — see `Games::seed`), and after
//! that it is arithmetic on sixteen numbers.
//!
//! Snake is the one that does have a clock, and this window already has one: the frames iced hands out.
//! So the app asks for them ([`Games::subscription`]) and steps the snake on them, and reads the board's
//! accelerometer on the same frames — the simulator answers flat, so on a desktop the arrows below are
//! still the only way to steer. Arrow keys play either game; on a board there is no keyboard at all, and
//! the pad the page draws, and the tilt of the box, are the ways in there.

use std::sync::Arc;

use games::Games;
use pomelo_hal::Board;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || Games::new(Arc::clone(&board)),
        Games::update,
        Games::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Games::theme)
    .subscription(Games::subscription)
    .run()
}
