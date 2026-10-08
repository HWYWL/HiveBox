//! The signature, as a program you can run.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! The animation is entirely in the subscription: `window::frames()` is one message per drawn
//! frame, and the app stops asking for frames when its cycle is over.

use hello::Hello;

fn main() -> iced::Result {
    iced::application(Hello::new, Hello::update, Hello::view)
        .theme(Hello::theme)
        .subscription(Hello::subscription)
        .run()
}
