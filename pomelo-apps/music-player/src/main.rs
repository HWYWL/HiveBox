//! The music player, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The board is the HAL's desktop simulator: this project is a player with no hardware behind it,
//! which is the point of the model being separate from the widgets. The firmware is what injects
//! the ESP32-S3 board instead — see the crate documentation.

use std::sync::Arc;

use music_player::Player;
use pomelo_hal::Board;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated());

    iced::application(
        move || Player::new(Arc::clone(&board)),
        Player::update,
        Player::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Player::theme)
    .subscription(Player::subscription)
    .run()
}
