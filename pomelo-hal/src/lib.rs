//! # pomelo-hal
//!
//! The hardware **interface** for the Pomelo OS board, plus the desktop simulator that
//! implements it.
//!
//! The crate names no hardware. It holds the traits every platform implements, the shared
//! types and errors, the [`Board`] facade that groups them, and a pure-Rust simulator for
//! developing on a host. The ESP32-S3 implementation is **not** here: it lives in
//! `firmware/pomelo-hal-esp32`, next to the C drivers it declares, and the composition root
//! (`rust_main`) builds it and injects the resulting board into the apps.
//!
//! Layers, one file per hardware domain (`power`, `wifi`, `audio`, `mic`, `imu`, `storage`, `web`,
//! `system`):
//!
//! * [`traits`] — the hardware **interface** every platform must implement.
//! * [`sim`]    — the desktop **simulation** backends (non-`espidf` platforms only).
//! * `firmware/pomelo-hal-esp32` — the real hardware, deliberately outside this crate.
//!
//! Application code should depend only on the [`Board`] facade and the traits, never on
//! `extern "C"` declarations directly. See
//! `issues-and-todo/zh/260926-03-hardware-interfaces-architecture.md` for the full
//! architecture and phased roadmap.
//!
//! On the name: "HAL" here means *board-level services* — the peripherals this board has —
//! not the register-level HAL that `esp-hal` means. Nothing in here talks to a register.

pub mod app_data;
pub mod board;
pub mod error;
pub mod music_settings;
pub mod pcm;
pub mod probe;
pub mod traits;
pub mod types;
pub mod wav;
pub mod wifi_credentials;

/// MP3 and FLAC decoding — behind a feature, because it is the only dependency in the crate that
/// exists for the device's sake rather than the simulator's.
#[cfg(feature = "decode")]
pub mod decode;

/// Desktop simulator backends, on every platform that is not the device itself.
#[cfg(not(target_os = "espidf"))]
pub mod sim;

pub use board::Board;
pub use error::HalError;
pub use music_settings::MusicSettings;
pub use traits::{
    AudioBackend, ImuBackend, InputBackend, MicBackend, PowerBackend, StorageBackend, SystemBackend,
    WebBackend, WifiBackend,
};
pub use types::{
    ApInfo, AudioMeta, ChipInfo, FirmwareInfo, FlashLayout, FlashRegion, FlashRegionKind,
    InputAction, MemoryInfo, ScanState, SystemEvent, Vec3, VolumeInfo, VolumeKind, WebStatus,
    WifiState, WifiStatus,
};
pub use wifi_credentials::WifiCredentials;

/// Convenience import for application crates.
pub mod prelude {
    pub use crate::board::Board;
    pub use crate::error::HalError;
    pub use crate::traits::{
        AudioBackend, ImuBackend, InputBackend, MicBackend, PowerBackend, StorageBackend,
        SystemBackend, WebBackend, WifiBackend,
    };
    pub use crate::types::{
        ApInfo, AudioMeta, ChipInfo, FirmwareInfo, FlashLayout, FlashRegion, FlashRegionKind,
        InputAction, MemoryInfo, ScanState, SystemEvent, Vec3, VolumeInfo, VolumeKind, WebStatus,
        WifiState, WifiStatus,
    };
    pub use crate::music_settings::MusicSettings;
    pub use crate::wifi_credentials::WifiCredentials;
}
