//! Simulated storage: the built-in partition, and a card in the slot.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;

use crate::error::HalError;
use crate::traits::StorageBackend;
use crate::types::{FlashLayout, FlashRegion, FlashRegionKind, VolumeInfo, VolumeKind};

/// An app slot: one of the two 6 MB rows of `partitions.csv`, `ota_0` and `ota_1`.
///
/// Two of them and no `factory`, because the board updates by writing the *other* slot and rebooting
/// into it. The size is what the image needs plus the room a release grows into, and it is the largest
/// slot that still leaves a filesystem worth having — every byte of it comes out of `internal`.
const OTA_SLOT_TOTAL: u32 = 0x60_0000;

/// The built-in partition: the `internal` partition of `partitions.csv`, which is the rest of the chip
/// after the second app slot.
const INTERNAL_TOTAL: u64 = 0x3E_0000;

/// The OTA selector: which of the two slots the bootloader boots. Two 4 KB sectors, as ESP-IDF writes
/// it, and blank on a chip that has only ever been flashed — which is why the first boot is `ota_0`.
const OTADATA_SIZE: u32 = 0x2000;

/// The two rows between the partition table and the first app slot.
const NVS_SIZE: u32 = 0x6000;
const PHY_SIZE: u32 = 0x1000;

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

/// Where `ota_0` starts, and so where the bootloader, the table, NVS, the OTA selector and the PHY blob
/// end. An app partition has to begin on a 64 KB boundary, so the stretch between the last of those and
/// here is the table's one gap.
const FIRST_APP_OFFSET: u32 = 0x2_0000;

/// The regions of the chip, in address order.
///
/// `nvs`, `otadata`, `phy_init`, `ota_0`, `ota_1` and `internal` are the six rows of `partitions.csv`;
/// `bootloader` and the `unallocated` stretches are the ones with no row at all. The kinds are the
/// board's rule: an app partition is the firmware, a data partition with a filesystem on it is
/// writable, and the rest is system — see `hal_storage_get_flash` in `board_storage.c`, which asks the
/// same questions of the real table.
///
/// The first `unallocated` is the 56 KB the 64 KB app alignment leaves above the PHY blob. The second
/// is zero: the table reaches the end of the chip, so nothing is left over. It stays in the list
/// because it is the case the page's bar has to lay out either way — a zero-byte segment that must not
/// be handed the whole bar — and dropping the row would be drawing a chip the board does not have.
fn flash_regions() -> Vec<FlashRegion> {
    let region = |label: &str, kind, size: u32| FlashRegion {
        label: String::from(label),
        kind,
        size,
    };

    let below_apps = BOOTLOADER_SIZE + NVS_SIZE + OTADATA_SIZE + PHY_SIZE;
    let used = FIRST_APP_OFFSET + 2 * OTA_SLOT_TOTAL + INTERNAL_TOTAL as u32;

    vec![
        region("bootloader", FlashRegionKind::System, BOOTLOADER_SIZE),
        region("nvs", FlashRegionKind::System, NVS_SIZE),
        region("otadata", FlashRegionKind::System, OTADATA_SIZE),
        region("phy_init", FlashRegionKind::System, PHY_SIZE),
        region(
            "unallocated",
            FlashRegionKind::Unallocated,
            FIRST_APP_OFFSET - below_apps,
        ),
        region("ota_0", FlashRegionKind::Firmware, OTA_SLOT_TOTAL),
        region("ota_1", FlashRegionKind::Firmware, OTA_SLOT_TOTAL),
        region("internal", FlashRegionKind::Data, INTERNAL_TOTAL as u32),
        region(
            "unallocated",
            FlashRegionKind::Unallocated,
            FLASH_TOTAL - used,
        ),
    ]
}

/// A filesystem error as the HAL's own.
///
/// `HalError::Io` carries the message rather than a code, because the message is the whole of what a
/// filesystem says: `NotFound`, `PermissionDenied` and `StorageFull` are three different screens, and
/// a code that meant "something went wrong" would draw all three the same.
fn io_error(error: std::io::Error) -> HalError {
    HalError::Io(error.to_string())
}

/// Make the directories above `path`, so that a first write needs no second call.
///
/// A path with no parent — a bare file name — has nothing to make, and `parent()` saying so is not an
/// error. The call is unconditional otherwise: `create_dir_all` on a directory that exists is `Ok`,
/// which is what makes it safe to run on every write.
fn make_parent(path: &str) -> Result<(), HalError> {
    match Path::new(path).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent).map_err(io_error),
        _ => Ok(()),
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

    /// The board's own 16 MB of NOR Flash, as `partitions.csv` divides it.
    fn flash(&self) -> Result<FlashLayout, HalError> {
        Ok(FlashLayout {
            total_bytes: FLASH_TOTAL,
            regions: flash_regions(),
        })
    }

    /// A plain `std::fs` write, which is exactly what the *device* cannot do.
    ///
    /// This is the one place the simulator is more capable than the board, and it is worth saying out
    /// loud rather than leaving as a difference someone discovers on hardware: what the device lacks is
    /// not the ability but the syscall — see the note above `read_credentials` in the ESP32 backend.
    /// A desktop app under this backend therefore exercises the same call and the same path, against a
    /// filesystem that answers it for real.
    fn write_file(&self, path: &str, data: &[u8]) -> Result<(), HalError> {
        make_parent(path)?;
        fs::write(path, data).map_err(io_error)
    }

    fn append_file(&self, path: &str, data: &[u8]) -> Result<(), HalError> {
        make_parent(path)?;

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io_error)?;

        file.write_all(data).map_err(io_error)
    }

    fn remove_file(&self, path: &str) -> Result<(), HalError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), HalError> {
        make_parent(to)?;
        fs::rename(from, to).map_err(io_error)
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

        // One stretch is nobody's: the 56 KB an app partition's 64 KB alignment leaves above the PHY
        // blob, where nothing may be put. The trailing `unallocated` is the zero-length one, and it
        // stays in the list because the page's bar has to lay out a zero-length segment either way.
        assert_eq!(layout.allocated_bytes(), FLASH_TOTAL - 0xE000);
    }

    /// The built-in volume is one region of the chip, and the map says so.
    ///
    /// This is the mismatch the page exists to show: 3.88 MB of writable space on a 16 MB chip.
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

    // -------------------------------------------------------------------------------------------
    // The write side.
    // -------------------------------------------------------------------------------------------

    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory of this test's own, under the system's temp directory.
    ///
    /// Named after the process and a counter so that two tests running at once cannot pick the same
    /// one, and removed and remade so that a run after a crash starts from nothing.
    fn scratch(name: &str) -> String {
        static NEXT: AtomicU32 = AtomicU32::new(0);

        let dir = std::env::temp_dir().join(format!(
            "pomelo-sim-storage-{}-{}-{}",
            name,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");

        dir.to_str().expect("a utf-8 scratch path").to_string()
    }

    /// The four calls are one story: what was written is what is read, on both halves of the pair
    /// that matters — this backend's writer, and the `std::fs` reader every app uses on the device.
    #[test]
    fn what_the_backend_writes_is_what_std_fs_reads() {
        let storage = SimStorage::new();
        let root = scratch("roundtrip");
        let path = format!("{root}/AppData/CLOCK/chime.wav");

        storage
            .write_file(&path, b"RIFF....WAVE")
            .expect("a write into a directory that does not exist yet");

        assert_eq!(
            fs::read(&path).expect("the file the write made"),
            b"RIFF....WAVE",
            "and the directories above it were made on the way"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// The whole reason `append_file` exists: a log's newest line must not cost a rewrite of every
    /// line before it.
    #[test]
    fn an_append_adds_to_the_end_rather_than_replacing() {
        let storage = SimStorage::new();
        let root = scratch("append");
        let path = format!("{root}/AppData/BATTERY/log.csv");

        storage.append_file(&path, b"a\n").expect("the first line");
        storage.append_file(&path, b"b\n").expect("the second");

        assert_eq!(fs::read_to_string(&path).unwrap(), "a\nb\n");

        let _ = fs::remove_dir_all(&root);
    }

    /// Removing a file that is not there is the caller's request already granted, and an app that
    /// means "make sure this is gone" is not served by being told it never existed.
    #[test]
    fn removing_something_that_is_not_there_is_not_a_failure() {
        let storage = SimStorage::new();
        let root = scratch("remove");

        assert!(
            storage
                .remove_file(&format!("{root}/AppData/NOTHING/none.conf"))
                .is_ok(),
            "a path with no file at the end of it"
        );

        let path = format!("{root}/AppData/CLOCK/old.conf");
        storage.write_file(&path, b"x").unwrap();
        storage.remove_file(&path).expect("and a file that is");

        assert!(!Path::new(&path).exists(), "which is gone afterwards");

        let _ = fs::remove_dir_all(&root);
    }

    /// A rename carries the bytes across and leaves nothing behind — the pair of facts a file manager
    /// draws as one line moving up a list.
    #[test]
    fn a_rename_moves_the_file_and_leaves_no_second_copy() {
        let storage = SimStorage::new();
        let root = scratch("rename");
        let from = format!("{root}/AppData/CLOCK/chime.wav");
        let to = format!("{root}/AppData/CLOCK/renamed.wav");

        storage.write_file(&from, b"RIFF").unwrap();
        storage.rename(&from, &to).expect("the move");

        assert!(!Path::new(&from).exists(), "the old name is free again");
        assert_eq!(fs::read(&to).unwrap(), b"RIFF");

        let _ = fs::remove_dir_all(&root);
    }
}
