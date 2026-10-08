//! Simulated storage: the built-in partition, and a card in the slot.

use crate::error::HalError;
use crate::traits::StorageBackend;
use crate::types::{VolumeInfo, VolumeKind};

/// The built-in partition: the 3 MB `internal` partition of `partitions.csv`.
const INTERNAL_TOTAL: u64 = 3 * 1024 * 1024;

/// What the firmware's own `welcome.txt` takes up, so the bar is not a flat zero.
const INTERNAL_USED: u64 = 8_192;

/// A card in the slot: 32 GB as the label says — the decimal size a card is sold in, not the binary
/// one a filesystem reports, which is why the page's number will not read `32.0 GB`.
const CARD_TOTAL: u64 = 32_000_000_000;

/// A little over half of it, so the card is a bar with something in it rather than a second copy of
/// the built-in volume.
const CARD_FREE: u64 = 18_200_000_000;

/// The desktop stand-in for the board's storage.
///
/// The address and the format are the board's — `/internal` on LittleFS, `/sdcard` on FATFS — because
/// what a page draws is a mount point and a filesystem name, and a simulator that invented its own
/// would be drawing a screen the device never shows.
///
/// A card is in the slot until [`SimStorage::eject`] takes it out. That is the *only* way the slot
/// changes here: [`StorageBackend::refresh`] has nothing to probe on a host, so unlike the board — where
/// a card really can appear while the box is running — the desktop answer is decided by whoever built
/// the backend. [`SimStorage::without_card`] is how a test asks for the empty slot.
pub struct SimStorage {
    /// Whether a card is in the slot.
    card: bool,
}

impl SimStorage {
    /// A board with a card in the slot.
    pub fn new() -> Self {
        Self { card: true }
    }

    /// The same board with an empty slot — the state every page has to survive.
    pub fn without_card() -> Self {
        Self { card: false }
    }

    /// Takes the card out.
    pub fn eject(&mut self) {
        self.card = false;
    }

    /// Puts one back in.
    pub fn insert(&mut self) {
        self.card = true;
    }

    /// Whether a card is in the slot right now.
    pub fn has_card(&self) -> bool {
        self.card
    }
}

impl Default for SimStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// The built-in partition, as the firmware mounts it.
fn internal() -> VolumeInfo {
    VolumeInfo {
        kind: VolumeKind::Internal,
        mount_point: "/internal".to_string(),
        filesystem: "LittleFS".to_string(),
        total_bytes: INTERNAL_TOTAL,
        free_bytes: INTERNAL_TOTAL - INTERNAL_USED,
    }
}

/// The card, as `bsp_sdcard_mount` mounts it.
fn card() -> VolumeInfo {
    VolumeInfo {
        kind: VolumeKind::Removable,
        mount_point: "/sdcard".to_string(),
        filesystem: "FATFS".to_string(),
        total_bytes: CARD_TOTAL,
        free_bytes: CARD_FREE,
    }
}

impl StorageBackend for SimStorage {
    /// Nothing to bring up: the simulator's filesystems are values, not drivers.
    fn init(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    fn volumes(&self) -> Result<Vec<VolumeInfo>, HalError> {
        let mut volumes = vec![internal()];

        if self.card {
            volumes.push(card());
        }

        Ok(volumes)
    }

    /// Nothing to probe. On the board this is where a card that appeared is mounted; here the slot
    /// changes only when [`SimStorage::eject`] or [`SimStorage::insert`] says so, and the next
    /// `volumes` already agrees with it.
    fn refresh(&mut self) -> Result<(), HalError> {
        Ok(())
    }
}
