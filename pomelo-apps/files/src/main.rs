//! `cargo run` — the file manager as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`Files::new`], then owns the loop. The board is the HAL's desktop
//! simulator, whose two volumes are real directories under the system's temp directory, so the
//! window browses files that are actually there.
//!
//! The app takes no subscription: nothing about a file manager changes on its own. A card that is
//! put in or taken out is noticed when the page opens and when the person asks it to look again,
//! which is the same moment the board would notice it.

use std::sync::Arc;

use files::Files;
use pomelo_hal::Board;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || Files::new(Arc::clone(&board)),
        Files::update,
        Files::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Files::theme)
    .run()
}
