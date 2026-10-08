//! Storage backend (the `internal` LittleFS partition and the microSD slot, via
//! `firmware/components/board_hal/board_storage.c`).
//!
//! Raw `extern "C"` bindings live in the private `ffi` module below; the safe trait impl is a thin
//! wrapper that maps the C status codes and `char[]` buffers into Rust types.
//!
//! Two C answers mean "nothing here" rather than "something broke", and both become `Ok`: a slot
//! with no card is the ordinary case on a board that may never have had one, and the trait says so
//! by leaving the volume out of the list rather than by failing to answer at all.

use std::ffi::c_char;

use pomelo_hal::{FlashLayout, FlashRegion, FlashRegionKind, HalError, StorageBackend, VolumeInfo, VolumeKind};

mod ffi {
    use std::ffi::c_char;

    pub const MOUNT_POINT_MAX_LEN: usize = 32;
    pub const FILESYSTEM_MAX_LEN: usize = 16;
    pub const FLASH_MAX_REGIONS: usize = 16;
    pub const FLASH_LABEL_MAX_LEN: usize = 16;

    /// Mirrors `hal_storage_volume_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalStorageVolume {
        pub mount_point: [c_char; MOUNT_POINT_MAX_LEN],
        pub filesystem: [c_char; FILESYSTEM_MAX_LEN],
        pub total_bytes: u64,
        pub free_bytes: u64,
    }

    /// The values of `hal_flash_region_kind_t`.
    pub mod flash_kind {
        pub const SYSTEM: u8 = 0;
        pub const FIRMWARE: u8 = 1;
        pub const DATA: u8 = 2;
        pub const UNALLOCATED: u8 = 3;
    }

    /// Mirrors `hal_flash_region_t`.
    ///
    /// The padding is spelled out rather than left to the compiler: this struct is read across the
    /// FFI, and a field order that made the two sides disagree about where `size` starts would be
    /// read as whatever happened to be there.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalFlashRegion {
        pub label: [c_char; FLASH_LABEL_MAX_LEN],
        pub kind: u8,
        pub _reserved: [u8; 3],
        pub size: u32,
    }

    /// Mirrors `hal_flash_layout_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalFlashLayout {
        pub total_bytes: u32,
        pub count: u32,
        pub regions: [HalFlashRegion; FLASH_MAX_REGIONS],
    }

    extern "C" {
        pub fn hal_storage_refresh() -> i32;
        pub fn hal_storage_get_internal(out: *mut HalStorageVolume) -> i32;
        pub fn hal_storage_get_card(out: *mut HalStorageVolume) -> i32;
        pub fn hal_storage_get_flash(out: *mut HalFlashLayout) -> i32;
    }
}

/// `ESP_ERR_NOT_FOUND`: the C side's "there is nothing mounted there" — an empty slot, or a
/// partition that was never mounted. Not a fault, and not a volume with no size in it.
const ESP_ERR_NOT_FOUND: i32 = 0x105;

/// A NUL-terminated C buffer as a `String`.
///
/// The same five lines `wifi.rs` has, and deliberately not hoisted: it is only the C side's spelling
/// of a string, and each file that has it is the only reader of its own `ffi` module.
fn c_buf_to_string(buf: &[c_char]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Read one volume, or nothing when nothing is mounted there.
///
/// A call that fails is not raised as an error: the card is a device a finger can take out, and a
/// page that drew nothing because the card had left would be hiding the built-in partition behind
/// it. What it is worth is a line in the log. The `kind` is the caller's, because it is the caller
/// that knows which slot it asked about.
fn read(
    call: unsafe extern "C" fn(*mut ffi::HalStorageVolume) -> i32,
    kind: VolumeKind,
) -> Option<VolumeInfo> {
    let mut raw = ffi::HalStorageVolume {
        mount_point: [0; ffi::MOUNT_POINT_MAX_LEN],
        filesystem: [0; ffi::FILESYSTEM_MAX_LEN],
        total_bytes: 0,
        free_bytes: 0,
    };

    let code = unsafe { call(&mut raw) };

    if code == ESP_ERR_NOT_FOUND {
        return None;
    }

    if code != 0 {
        eprintln!("[storage] {kind:?} could not be read: {code}");
        return None;
    }

    Some(VolumeInfo {
        kind,
        mount_point: c_buf_to_string(&raw.mount_point),
        filesystem: c_buf_to_string(&raw.filesystem),
        total_bytes: raw.total_bytes,
        free_bytes: raw.free_bytes,
    })
}

/// The C kind byte as the HAL's own type, or `None` for a kind this build does not know.
///
/// A `match` over the constants rather than a transmute: the two enums are a promise between the C
/// and the Rust side, and a byte that is not one of the four is the promise broken — worth a line in
/// the log rather than a region drawn in whatever colour that number happened to be.
fn region_kind(byte: u8) -> Option<FlashRegionKind> {
    match byte {
        ffi::flash_kind::SYSTEM => Some(FlashRegionKind::System),
        ffi::flash_kind::FIRMWARE => Some(FlashRegionKind::Firmware),
        ffi::flash_kind::DATA => Some(FlashRegionKind::Data),
        ffi::flash_kind::UNALLOCATED => Some(FlashRegionKind::Unallocated),
        _ => None,
    }
}

pub struct EspStorage;

impl EspStorage {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for EspStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for EspStorage {
    /// Bring up the SDMMC host and mount a card if there is one.
    ///
    /// Mounting *is* what bringing this backend up means: there is no clock to start and no cache to
    /// warm, and the built-in partition was mounted by the firmware before any of this ran. An empty
    /// slot is not a failure, so this returns `Ok` for one and the board boots exactly as it would
    /// with a card in it — with a volume fewer.
    fn init(&mut self) -> Result<(), HalError> {
        self.refresh()
    }

    fn volumes(&self) -> Result<Vec<VolumeInfo>, HalError> {
        let mut volumes = Vec::new();

        if let Some(internal) = read(ffi::hal_storage_get_internal, VolumeKind::Internal) {
            volumes.push(internal);
        }

        if let Some(card) = read(ffi::hal_storage_get_card, VolumeKind::Removable) {
            volumes.push(card);
        }

        Ok(volumes)
    }

    /// Re-probe the slot. `ESP_ERR_NOT_FOUND` is an empty slot, which is an answer rather than a
    /// fault; the C side bounds how often it will actually touch the SDMMC host.
    fn refresh(&mut self) -> Result<(), HalError> {
        match unsafe { ffi::hal_storage_refresh() } {
            0 | ESP_ERR_NOT_FOUND => Ok(()),
            code => Err(HalError::Internal(code)),
        }
    }

    /// The flash chip: its total size, and every region of it in address order.
    ///
    /// The whole chip is filled in before it is looked at, so the layout is initialised by the time
    /// it is read — but only on the success path, which is why the `assume_init` is behind the code
    /// check rather than beside the call.
    fn flash(&self) -> Result<FlashLayout, HalError> {
        let mut raw = std::mem::MaybeUninit::<ffi::HalFlashLayout>::uninit();

        let code = unsafe { ffi::hal_storage_get_flash(raw.as_mut_ptr()) };
        if code != 0 {
            return Err(HalError::Internal(code));
        }

        let raw = unsafe { raw.assume_init() };
        let count = (raw.count as usize).min(ffi::FLASH_MAX_REGIONS);

        let mut regions = Vec::with_capacity(count);

        for entry in raw.regions.iter().take(count) {
            let Some(kind) = region_kind(entry.kind) else {
                return Err(HalError::Io(format!(
                    "the flash map has an unknown region kind: {}",
                    entry.kind
                )));
            };

            regions.push(FlashRegion {
                label: c_buf_to_string(&entry.label),
                kind,
                size: entry.size,
            });
        }

        // A map with no regions in it is not a board with no flash: it is a map the page would draw
        // as an empty bar under a total it cannot account for.
        if regions.is_empty() {
            return Err(HalError::Io(String::from("the flash map came back empty")));
        }

        Ok(FlashLayout {
            total_bytes: raw.total_bytes,
            regions,
        })
    }
}
