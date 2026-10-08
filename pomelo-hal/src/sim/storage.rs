//! Simulated storage: the built-in partition, and a card in the slot.

use crate::error::HalError;
use crate::traits::StorageBackend;
use crate::types::{FlashLayout, FlashRegion, FlashRegionKind, VolumeInfo, VolumeKind};

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

/// The flash chip: 16 MB of NOR, as the board's `partitions.csv` divides it.
///
/// The table is written out here rather than read, because on a host there is no partition table to
/// read — and it is written out *whole*, bootloader and trailing space included, because that is
/// what makes the desktop's flash map the device's. A simulator that drew only the partitions would
/// be drawing a page the board never shows, which is the one thing a simulator may not do.
const FLASH_TOTAL: u32 = 16 * 1024 * 1024;

/// Where the partition table's own entries begin. Everything below the first of them is the
/// bootloader and the table itself.
const BOOTLOADER_SIZE: u32 = 0x9000;

/// The regions of the chip, in address order.
///
/// `nvs`, `phy_init`, `factory` and `internal` are the four rows of `partitions.csv`; `bootloader`
/// and `unallocated` are the two stretches with no row at all. The kinds are the board's rule: an
/// app partition is the firmware, a data partition with a filesystem on it is writable, and the rest
/// is system — see `hal_storage_get_flash` in `board_storage.c`, which asks the same questions of
/// the real table.
fn flash_regions() -> Vec<FlashRegion> {
    let region = |label: &str, kind, size: u32| FlashRegion {
        label: String::from(label),
        kind,
        size,
    };

    vec![
        region("bootloader", FlashRegionKind::System, BOOTLOADER_SIZE),
        region("nvs", FlashRegionKind::System, 0x6000),
        region("phy_init", FlashRegionKind::System, 0x1000),
        region("factory", FlashRegionKind::Firmware, 0xC0_0000),
        region("internal", FlashRegionKind::Data, INTERNAL_TOTAL as u32),
        region(
            "unallocated",
            FlashRegionKind::Unallocated,
            FLASH_TOTAL - 0xC10000 - INTERNAL_TOTAL as u32,
        ),
    ]
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

    /// The board's own 16 MB of NOR Flash, as `partitions.csv` divides it.
    fn flash(&self) -> Result<FlashLayout, HalError> {
        Ok(FlashLayout {
            total_bytes: FLASH_TOTAL,
            regions: flash_regions(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map has to be the chip: regions that overlap, or leave a hole, are a bar the page draws
    /// with a gap in it — and the gap would be a lie about where the flash went.
    #[test]
    fn the_simulated_flash_tiles_the_whole_chip() {
        let layout = SimStorage::new().flash().expect("the simulator always answers");

        let sum: u32 = layout.regions.iter().map(|region| region.size).sum();
        assert_eq!(sum, layout.total_bytes, "the regions must be the chip");

        assert_eq!(layout.allocated_bytes(), FLASH_TOTAL - 0xF_0000);
    }

    /// The built-in volume is one region of the chip, and the map says so.
    ///
    /// This is the mismatch the page exists to show: 3 MB of writable space on a 16 MB chip.
    #[test]
    fn the_built_in_volume_is_a_region_of_the_flash() {
        let storage = SimStorage::new();
        let volumes = storage.volumes().expect("the simulator always answers");
        let layout = storage.flash().expect("the simulator always answers");

        let internal = volumes
            .first()
            .expect("the built-in volume is always there");

        let region = layout
            .regions
            .iter()
            .find(|region| region.kind == FlashRegionKind::Data)
            .expect("the filesystem region");

        assert_eq!(region.label, "internal");
        assert_eq!(region.size as u64, internal.total_bytes);
    }
}
