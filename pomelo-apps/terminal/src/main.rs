//! The terminal, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The subscription is what the app is told its size by: a window on a desktop, the panel on the
//! board. Without it the terminal still draws — at the design size, see `SCREEN` — which is why a
//! resize is a message and not a constructor argument.
//!
//! This window is the shell in this crate and nothing else. A session on a machine across the network
//! needs a board to open it through — `Terminal::with_board` and `Terminal::connect`, which is how the
//! launcher starts one when the NAS monitor asks it to.

use terminal::Terminal;

fn main() -> iced::Result {
    iced::application(Terminal::new, Terminal::update, Terminal::view)
        .theme(Terminal::theme)
        .subscription(Terminal::subscription)
        .run()
}
