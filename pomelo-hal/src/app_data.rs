//! Where the apps' own files live.
//!
//! Two roots, one per platform, and neither of them belongs to an app: the Wi-Fi files are under
//! `WIFI/`, the music player's under `MUSIC/`, and the directory they share is the same one. That is
//! why the pair sits here rather than in the module that needed it first — the second user of a
//! constant is when it stops belonging to the first, and a copy of a path is a copy that can drift.
//!
//! # A missing root is a write's problem, not a read's
//!
//! Mounting the board's root is the firmware's job, not this crate's. A board where nothing mounted
//! it fails on the *write*, not on the read: a file that is not there reads as `Ok(None)`, because
//! "never saved anything" and "nowhere to save it" must not look the same — only the second is a
//! fault, and only the second is the firmware's.

/// The app-data root on the board: the `internal` partition, mounted by the firmware.
///
/// `/internal` is a 3 MB SPIFFS partition (`partitions.csv`).
pub const BOARD_APP_DATA: &str = "/internal/AppData";

/// The app-data root on a desktop, under the home directory.
///
/// A desktop has no partition to mount, so the same shape sits one directory down instead. The
/// simulators do not use it — see `sim::wifi` for why — but it is here so that the two platforms are
/// one line apart rather than one design apart.
pub const DESKTOP_APP_DATA: &str = ".pomelo/AppData";
