//! `cargo run` — the update page as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`Updater::new`], then owns the loop. The board is the HAL's desktop
//! simulator, with its HTTP client swapped for one holding a *manifest* — [`MANIFEST`] below, which is
//! what the build script writes and what the repository really serves. That swap is the whole point of
//! the window: without it the simulator's client answers `200` with an empty body, the page reads it as
//! something that was not a manifest, and the demo draws the fault line that case deserves rather than
//! the two versions it is for.
//!
//! What the window *cannot* show is either row being installable, and that is honest: this image has no
//! download backend, so pressing either button draws the line that says so — see
//! [`Status::Unsupported`](updater::Status::Unsupported).
//!
//! The simulator only advances when somebody calls `tick`, and a desktop run has no firmware loop to do
//! it. The device's own client is driven by the network and its `tick` does nothing, so this is a
//! desktop-only concern — which is why it lives in `main` and not in the app.
//!
//! The subscription is the app's own ([`Updater::subscription`]) and it is the one thing the app cannot
//! do for itself: a window's clock is its frames, where the board's is the launcher's pump. The app asks
//! for them only while a request is in flight — see [`Updater::waiting`].

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::sim::SimHttp;
use pomelo_hal::Board;
use updater::Updater;

/// What the repository would have sent.
///
/// A real document rather than a convenient one: two members, the version in each file's name, the size
/// and digest beside it, and the font's second half under `"face"` — exactly as
/// `tools/publish_bin.py --card-dir` writes it. Keeping it real is what makes the demo worth running:
/// the reader, the version's own read, and the two requests a package's press makes are all exercised
/// by what the page will meet on the network.
const MANIFEST: &str = r#"{
    "generated": "2026-10-16T09:00:00Z",
    "firmware": {
        "version": "0.7.0",
        "file": "hivebox-firmware-v0.7.0.bin",
        "size": 7222464,
        "sha256": "3f2a1c0e5b8d47a96f0c2e1d4b7a8c93e5f16a2d4c8b9e0f3a7d5c1b2e4f6a8d"
    },
    "font": {
        "version": "2",
        "file": "hivebox-font-24px-v2.bin",
        "size": 2268084,
        "sha256": "9c4137b0e2d54a86f1b3c7e0d2a4986fb5e31c70a2d849e6b1f3c5a7d90e2b48",
        "face": {
            "file": "hivebox-face-v2.otf",
            "size": 1910512,
            "sha256": "1cfb6b3e3b688b8480148fe466540421707ed819bad1786dda6a3ffd18ae0e54"
        }
    }
}"#;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated().with_http(Box::new(SimHttp::answering(MANIFEST))));

    {
        let board = Arc::clone(&board);

        std::thread::spawn(move || loop {
            board.tick();
            std::thread::sleep(Duration::from_millis(10));
        });
    }

    iced::application(
        move || Updater::new(Arc::clone(&board)),
        Updater::update,
        Updater::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Updater::theme)
    .subscription(Updater::subscription)
    .run()
}
