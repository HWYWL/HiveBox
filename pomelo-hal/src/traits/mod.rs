//! Platform-agnostic hardware traits — the HAL **interface** layer.
//!
//! Every backend is `Send + Sync` so it can live behind a [`std::sync::Mutex`]
//! inside [`crate::Board`] and be shared between the UI thread and background
//! tasks. Long-running operations follow a *start + poll* pattern: the caller
//! kicks off the work and then polls a status method, which never blocks.
//!
//! One file per hardware domain. A new peripheral is added in three places, one per layer:
//! a module here, its simulator in [`crate::sim`], and its ESP32-S3 backend in
//! `firmware/pomelo-hal-esp32/src/` — which is outside this crate on purpose, so the
//! interface can be read, built and tested without any of the hardware code.

mod audio;
mod cloud;
mod http;
mod imu;
mod input;
mod mic;
mod nas;
mod power;
mod storage;
mod system;
mod web;
mod wifi;
mod workbuddy;
mod deepseek;

pub use audio::AudioBackend;
pub use cloud::CloudBackend;
pub use http::HttpBackend;
pub use imu::ImuBackend;
pub use input::InputBackend;
pub use mic::MicBackend;
pub use nas::NasBackend;
pub use power::PowerBackend;
pub use storage::StorageBackend;
pub use system::SystemBackend;
pub use web::WebBackend;
pub use wifi::WifiBackend;
pub use workbuddy::WorkBuddyBackend;

/// 一个「这块板子没有它」的 DeepSeek 后端。
pub use deepseek::{DeepSeekBackend, NoDeepSeek};
