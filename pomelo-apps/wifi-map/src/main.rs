//! `cargo run` — the channel map as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`WifiMap::new`], then owns the loop. The board is the HAL's desktop
//! simulator, which is the whole point of the window: its radio has five access points on five
//! channels, two of them in the band's three clean ones — so the chart has a shape, the map has
//! squares, and the page has a recommendation to make.
//!
//! The simulator only advances when somebody calls `tick`, and a desktop run has no firmware loop to do
//! it. The device's own backends are event-driven and their `tick` is a no-op, so this is a
//! desktop-only concern — which is why it lives in `main` and not in the app.
//!
//! The subscription is the app's own ([`WifiMap::subscription`]) and it is the one thing the app cannot
//! do for itself: a window's clock is its frames, where the board's is the launcher's pump. The app asks
//! for them only while a scan is in flight — see [`WifiMap::waiting`].

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::Board;
use wifi_map::WifiMap;

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
        move || WifiMap::new(Arc::clone(&board)),
        WifiMap::update,
        WifiMap::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(WifiMap::theme)
    .subscription(WifiMap::subscription)
    .run()
}
