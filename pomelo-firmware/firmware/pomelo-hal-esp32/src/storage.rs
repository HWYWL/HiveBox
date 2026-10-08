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

use pomelo_hal::{HalError, StorageBackend, VolumeInfo, VolumeKind};

mod ffi {
    use std::ffi::c_char;

    pub const MOUNT_POINT_MAX_LEN: usize = 32;
    pub const FILESYSTEM_MAX_LEN: usize = 16;

    /// Mirrors `hal_storage_volume_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalStorageVolume {
        pub mount_point: [c_char; MOUNT_POINT_MAX_LEN],
        pub filesystem: [c_char; FILESYSTEM_MAX_LEN],
        pub total_bytes: u64,
        pub free_bytes: u64,
    }

    extern "C" {
        pub fn hal_storage_refresh() -> i32;
        pub fn hal_storage_get_internal(out: *mut HalStorageVolume) -> i32;
        pub fn hal_storage_get_card(out: *mut HalStorageVolume) -> i32;
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
}
