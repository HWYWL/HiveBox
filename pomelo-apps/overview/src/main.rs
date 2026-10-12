//! `cargo run` — the overview as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`Overview::new`], then owns the loop. The board is the HAL's desktop
//! simulator, which is the whole point of the window: it has a gauge, a radio, two filesystems, a heap
//! with a pool beside it, and a NAS that has not been configured — so every card on the page has
//! something real to draw, including the two states a real board is usually in and this page exists
//! for (a slot with no card, and a server nobody has told it about).
//!
//! The subscription is the app's own ([`Overview::subscription`]) and it is the one thing the app
//! cannot do for itself: a window's clock is its frames, where the board's is the launcher's pump. The
//! app throttles them itself — see [`overview::SAMPLE`] — because sixty reads a second of five
//! backends would be five locks a frame for numbers that change on the scale of minutes.

use std::sync::Arc;

use overview::Overview;
use pomelo_hal::Board;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || Overview::new(Arc::clone(&board)),
        Overview::update,
        Overview::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Overview::theme)
    .subscription(Overview::subscription)
    .run()
}
