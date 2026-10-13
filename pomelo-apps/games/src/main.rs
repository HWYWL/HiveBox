//! `cargo run` — the list of games, and 2048, as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The shortest `main` in the workspace, and the difference is worth saying out loud: there is nothing
//! to poll and nothing to pump. The Wi-Fi and weather pages are requests, and a request needs a clock —
//! frames in a window, the launcher's pump on the board — so their demos carry a tick thread and their
//! apps ask for a subscription. A game asks the board exactly one question, once, when it starts (how
//! long have you been up, which is how it decides which game to deal — see `Games::seed`), and after
//! that it is arithmetic on sixteen numbers.
//!
//! Arrow keys play. That is the app's own subscription ([`Games::subscription`]) and the only thing in
//! here that is not a line of setup: on the board there is no keyboard at all, and the pad the page
//! draws is the way in there.

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
