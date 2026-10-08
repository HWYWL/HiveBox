//! What a desktop frame costs, measured: the app, with its frames counted.
//!
//! ```text
//! cargo run --release --example frame_rate               # a 1024x768 window: iced's default
//! HELLO_WINDOW=480x480 cargo run --release --example frame_rate
//! ```
//!
//! iced runs this app from `iced::window::frames()` -- one message per frame the platform draws --
//! so counting `Message::Tick` counts frames. A thread prints the rate every 500 ms and says when
//! the app stopped asking for frames at all (the animation is over: the subscription is empty).
//!
//! Measured on 2026-09-29 (WSLg, X11, `env -u WAYLAND_DISPLAY`, iced's `tiny-skia` renderer, a
//! 1024x768 window -- the 480x480 panel this artwork was drawn for has 3.4x fewer pixels):
//!
//! | profile | blank canvas | while the stroke is drawn | once it is finished |
//! |---|---|---|---|
//! | debug (`cargo run`) | ~1,700 fps | 4-6 fps | (no frames) |
//! | `--release` | ~3,100 fps | 220-270 fps | ~1,400 fps |
//!
//! The app caches the wash and every piece that has finished and records only the piece still
//! growing (`canvas`, and the module docs there), so these are the cost of one stroke and of the
//! damage between two frames. What they were before those caches: 196, 214, 146, 50, 18, 18, 10 fps
//! in the `--release` run -- a whole-window repaint every frame, because a canvas is one indivisible
//! item to iced's damage diff and a live one never compares equal. A *finished* picture was the
//! worst of it: 10 fps of repainting a picture that had not changed.
//!
//! What is left in these numbers is the profile (debug is two orders of magnitude slower on the one
//! stroke that cannot be cached) and the window (the default is not the panel).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hello::{Hello, Message};
use iced::{Element, Subscription, Theme};

/// The ticks seen so far, written by the app and read by the counter thread.
static FRAMES: AtomicU64 = AtomicU64::new(0);

/// The app, with the frames counted.
struct Counted(Hello);

impl Counted {
    fn new() -> Self {
        Self(Hello::new())
    }

    fn update(&mut self, message: Message) {
        if let Message::Tick(_) = message {
            FRAMES.fetch_add(1, Ordering::Relaxed);
        }

        self.0.update(message);
    }

    fn view(&self) -> Element<'_, Message> {
        self.0.view()
    }

    fn theme(&self) -> Theme {
        self.0.theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        self.0.subscription()
    }
}

/// The window to ask for: `HELLO_WINDOW=WxH`, or iced's own default.
///
/// It is a knob because the cost is per pixel: the panel the artwork was designed for is 480x480,
/// and the default desktop window has 3.4x its pixels.
fn window_size() -> iced::Size {
    let Ok(spec) = std::env::var("HELLO_WINDOW") else {
        return iced::Size::new(1024.0, 768.0);
    };

    let (width, height) = spec.split_once('x').expect("HELLO_WINDOW=WxH");

    iced::Size::new(
        width.parse().expect("a number, in HELLO_WINDOW=WxH"),
        height.parse().expect("a number, in HELLO_WINDOW=WxH"),
    )
}

fn main() -> iced::Result {
    let start = Instant::now();
    let size = window_size();

    eprintln!("window {size:?} -- this process's start is t=0");

    std::thread::spawn(move || {
        let mut last = 0;
        let mut quiet = 0;

        loop {
            std::thread::sleep(Duration::from_millis(500));

            let now = FRAMES.load(Ordering::Relaxed);
            let frames = now - last;
            last = now;

            let at = start.elapsed().as_secs_f32() * 1000.0;

            if frames == 0 {
                quiet += 1;
                if quiet == 3 {
                    eprintln!("[+{at:>6.0} ms] idle: no frames");
                }
            } else {
                quiet = 0;
                eprintln!(
                    "[+{at:>6.0} ms] {frames} frames in 500 ms = {} fps",
                    frames * 2
                );
            }
        }
    });

    iced::application(Counted::new, Counted::update, Counted::view)
        .theme(Counted::theme)
        .subscription(Counted::subscription)
        .window_size(size)
        .run()
}
