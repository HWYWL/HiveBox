//! The terminal, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The subscription is what the app is told its size by: a window on a desktop, the panel on the
//! board. Without it the terminal still draws — at the design size, see `SCREEN` — which is why a
//! resize is a message and not a constructor argument.

use terminal::Terminal;

fn main() -> iced::Result {
    iced::application(Terminal::new, Terminal::update, Terminal::view)
        .theme(Terminal::theme)
        .subscription(Terminal::subscription)
        .run()
}
