//! Per-page view functions, split one file per page.
//!
//! Every function here is `pub(super)` — callers outside this crate only touch
//! [`Settings::view`], not the individual page builders.
//!
//! The shared layout helpers (`card`, `body`, `separator`, …) live in
//! [`common`] and are re-exported here for the page modules to use freely.

pub mod battery;
pub mod card;
pub mod common;
pub mod dialog;
pub mod main_page;
pub mod memory;
pub mod ring;
pub mod storage;
pub mod summary;
pub mod system;
pub mod theme;
pub mod time;
pub mod wifi;
