//! System information (the chip, the image, the timer and the heap, via
//! `firmware/components/board_hal/board_system.c`).
//!
//! Raw `extern "C"` bindings live in the private `ffi` module below; the safe trait impl is a thin
//! wrapper that maps the C status codes and `char[]` buffers into Rust types.
//!
//! # Nothing here owns anything
//!
//! Every other backend in this crate wraps a device it had to bring up. This one wraps calls into
//! things the chip and the image were already doing — the part number the silicon reports, the
//! description the build stamped into the image, the microsecond timer and the heap that the C
//! runtime has had since the first line of `app_main`. So there is no state, no `init`, and
//! `EspSystem` is a unit struct on purpose: there is nothing to hold.
//!
//! # Why the strings cross as buffers
//!
//! The C side fills a `char[]` inside a struct rather than returning a `const char *`, and the
//! difference matters: a returned pointer would be borrowed from someone else's memory whose
//! lifetime Rust cannot check, and turning it into a `&'static str` would be a promise the Rust
//! side has no way to verify. A caller-owned buffer needs no such promise.
//!
//! Same five-line NUL-terminated-buffer conversion as `storage.rs` and `wifi.rs`, and deliberately
//! not hoisted for the reason those two give: each file is the only reader of its own `ffi` module.

use std::ffi::c_char;
use std::mem::MaybeUninit;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pomelo_hal::{
    ChipInfo, FirmwareInfo, HalError, MemoryInfo, SystemBackend, TaskInfo, TaskState,
};

mod ffi {
    use std::ffi::c_char;

    /// Mirrors `HAL_SYSTEM_CHIP_MODEL_MAX_LEN`.
    pub const CHIP_MODEL_MAX_LEN: usize = 16;
    /// Mirrors `HAL_SYSTEM_FIRMWARE_NAME_MAX_LEN`.
    pub const FIRMWARE_NAME_MAX_LEN: usize = 24;
    /// Mirrors `HAL_SYSTEM_VERSION_MAX_LEN`.
    pub const VERSION_MAX_LEN: usize = 32;
    /// Mirrors `HAL_SYSTEM_BUILD_MAX_LEN` — and `esp_app_desc_t`'s own `date` and `time`.
    pub const BUILD_MAX_LEN: usize = 16;
    /// Mirrors `HAL_SYSTEM_TASK_NAME_MAX_LEN`.
    pub const TASK_NAME_MAX_LEN: usize = 16;
    /// Mirrors `HAL_SYSTEM_MAX_TASKS`.
    pub const MAX_TASKS: usize = 32;

    /// Mirrors `hal_system_chip_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalSystemChip {
        pub model: [c_char; CHIP_MODEL_MAX_LEN],
        pub cores: u8,
        pub revision: u16,
    }

    /// Mirrors `hal_system_firmware_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalSystemFirmware {
        pub name: [c_char; FIRMWARE_NAME_MAX_LEN],
        pub version: [c_char; VERSION_MAX_LEN],
        pub date: [c_char; BUILD_MAX_LEN],
        pub time: [c_char; BUILD_MAX_LEN],
    }

    /// Mirrors `hal_system_memory_t`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalSystemMemory {
        pub total_bytes: u64,
        pub free_bytes: u64,
    }

    /// Mirrors `hal_system_task_t`.
    ///
    /// The state crosses as a `u8` and not as an enum: a C enum's width is the compiler's business,
    /// and a field two languages disagree about the width of reads as garbage on the Rust side.
    /// [`task_state`] is what puts a name to it.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalSystemTask {
        pub name: [c_char; TASK_NAME_MAX_LEN],
        pub state: u8,
        pub priority: u8,
        /// `0xff` for a task the scheduler may run on either core.
        pub core: u8,
        pub current: u8,
        pub stack_free_bytes: u32,
    }

    /// Mirrors `hal_system_tasks_t`.
    ///
    /// A fixed array with a count, like the flash layout: the caller brings the buffer and the C side
    /// says how much of it it filled. Nothing here allocates, on either side of the FFI.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct HalSystemTasks {
        pub count: u32,
        pub tasks: [HalSystemTask; MAX_TASKS],
    }

    extern "C" {
        pub fn hal_system_get_chip(out: *mut HalSystemChip) -> i32;
        pub fn hal_system_get_firmware(out: *mut HalSystemFirmware) -> i32;
        pub fn hal_system_get_uptime_us() -> i64;
        pub fn hal_system_get_memory(out: *mut HalSystemMemory) -> i32;
        pub fn hal_system_get_tasks(out: *mut HalSystemTasks) -> i32;
        pub fn hal_system_get_psram(out: *mut HalSystemMemory) -> i32;
        pub fn hal_system_get_epoch(out_epoch: *mut i64) -> i32;
    }
}

/// `ESP_ERR_INVALID_STATE`: the C side's "the board has no time yet" — a PCF85063A that came up
/// holding nothing sane, with nothing having set it since. Not a fault, and not 1970 either.
const ESP_ERR_INVALID_STATE: i32 = 0x103;

/// `ESP_ERR_NOT_FOUND`: the C side's "this board has none of that" — used by the external RAM read on
/// a board without any, which is a fact about the board rather than a failure of the read.
const ESP_ERR_NOT_FOUND: i32 = 0x105;

/// A NUL-terminated C buffer as a `String`.
fn c_buf_to_string(buf: &[c_char]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// `hal_system_task_t::state` as the enum it stands for.
///
/// Anything this does not know — a state a newer IDF has added — is reported as blocked rather than
/// as running: three of the five scheduler states are ways of waiting, so it is the answer that is
/// wrong least often, and it is not the one a reader would act on.
fn task_state(state: u8) -> TaskState {
    match state {
        0 => TaskState::Running,
        2 => TaskState::Suspended,
        3 => TaskState::Deleted,
        _ => TaskState::Blocked,
    }
}

/// The board's own account of itself.
pub struct EspSystem;

impl EspSystem {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for EspSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemBackend for EspSystem {
    /// The chip, from `esp_chip_info`.
    ///
    /// The model is spelled out by the C side rather than derived from the enum here, because the
    /// enum's values are the vendor's and its names are C's: `CHIP_ESP32S3` is not a part number
    /// anybody writes on a box.
    fn chip(&self) -> Result<ChipInfo, HalError> {
        let mut raw = MaybeUninit::<ffi::HalSystemChip>::uninit();

        let code = unsafe { ffi::hal_system_get_chip(raw.as_mut_ptr()) };
        if code != 0 {
            return Err(HalError::Internal(code));
        }

        let raw = unsafe { raw.assume_init() };

        Ok(ChipInfo {
            model: c_buf_to_string(&raw.model),
            cores: raw.cores,
            revision: raw.revision,
        })
    }

    /// The running image's own description, and when it was built.
    ///
    /// `ESP_ERR_NOT_FOUND` is an image with no description — one not built by this toolchain —
    /// which becomes an `Io` error rather than an empty `FirmwareInfo`: a card drawing a blank
    /// version would be showing a board that has one and cannot say it.
    ///
    /// The stamp comes out of the same description, in the two fields the compiler filled: `date`
    /// and `time`, which [`FirmwareInfo::compile_stamp`] respells into one instant. An image that
    /// carries neither leaves the reading absent rather than blank, which is the one thing this
    /// backend and the C side have to agree about.
    fn firmware(&self) -> Result<FirmwareInfo, HalError> {
        let mut raw = MaybeUninit::<ffi::HalSystemFirmware>::uninit();

        let code = unsafe { ffi::hal_system_get_firmware(raw.as_mut_ptr()) };
        if code != 0 {
            return Err(HalError::Internal(code));
        }

        let raw = unsafe { raw.assume_init() };

        Ok(FirmwareInfo {
            name: c_buf_to_string(&raw.name),
            version: c_buf_to_string(&raw.version),
            built: FirmwareInfo::compile_stamp(
                &c_buf_to_string(&raw.date),
                &c_buf_to_string(&raw.time),
            ),
        })
    }

    /// How long the microsecond timer has been counting, which is how long this has been up.
    ///
    /// A negative reading is not something the chip can report — the timer starts at zero — so it
    /// is treated as no answer rather than as an uptime before the epoch: `as u64` on it would turn
    /// a fault into 584,942 years.
    fn uptime(&self) -> Result<Duration, HalError> {
        let micros = unsafe { ffi::hal_system_get_uptime_us() };

        if micros < 0 {
            return Err(HalError::Io(format!(
                "the uptime timer read {micros} µs, which is before the boot"
            )));
        }

        Ok(Duration::from_micros(micros as u64))
    }

    /// The default heap: every byte of it, and what is left.
    fn memory(&self) -> Result<MemoryInfo, HalError> {
        let mut raw = MaybeUninit::<ffi::HalSystemMemory>::uninit();

        let code = unsafe { ffi::hal_system_get_memory(raw.as_mut_ptr()) };
        if code != 0 {
            return Err(HalError::Internal(code));
        }

        let raw = unsafe { raw.assume_init() };

        Ok(MemoryInfo {
            total_bytes: raw.total_bytes,
            free_bytes: raw.free_bytes,
        })
    }

    /// The tasks the scheduler is running, read out of `uxTaskGetSystemState`.
    ///
    /// An empty `Vec` is a board with nothing to say — an image built without the trace facility —
    /// rather than a failure: the C side answers `ESP_OK` with a count of zero for exactly that case,
    /// and a page drawing "no tasks" is drawing the truth about such an image. The core comes across
    /// as `0xff` for "either, or not reported", which is the one value a real core number cannot be.
    fn tasks(&self) -> Result<Vec<TaskInfo>, HalError> {
        let mut raw = MaybeUninit::<ffi::HalSystemTasks>::uninit();

        let code = unsafe { ffi::hal_system_get_tasks(raw.as_mut_ptr()) };
        if code != 0 {
            return Err(HalError::Internal(code));
        }

        let raw = unsafe { raw.assume_init() };
        // The count is the C side's, and this is what keeps a count larger than the array — which
        // cannot happen from this C code and could from another — from walking off the end of it.
        let count = (raw.count as usize).min(ffi::MAX_TASKS);

        Ok(raw.tasks[..count]
            .iter()
            .map(|task| TaskInfo {
                name: c_buf_to_string(&task.name),
                state: task_state(task.state),
                priority: task.priority,
                stack_free_bytes: task.stack_free_bytes,
                core: (task.core < 2).then_some(task.core),
                current: task.current != 0,
            })
            .collect())
    }

    /// The external RAM, which this board has: the pool the framebuffer and the log ring come out of.
    ///
    /// `ESP_ERR_NOT_FOUND` is the C side saying the board has none, and becomes `Ok(None)` — a fact
    /// about a board rather than a failure to read one. See the note on the C function for why this is
    /// a separate reading from [`SystemBackend::memory`] and never a sum with it.
    fn psram(&self) -> Result<Option<MemoryInfo>, HalError> {
        let mut raw = MaybeUninit::<ffi::HalSystemMemory>::uninit();

        match unsafe { ffi::hal_system_get_psram(raw.as_mut_ptr()) } {
            0 => {}
            ESP_ERR_NOT_FOUND => return Ok(None),
            code => return Err(HalError::Internal(code)),
        }

        let raw = unsafe { raw.assume_init() };

        Ok(Some(MemoryInfo {
            total_bytes: raw.total_bytes,
            free_bytes: raw.free_bytes,
        }))
    }

    /// The board's clock, or nothing when it has never been told the time.
    ///
    /// `ESP_ERR_INVALID_STATE` is the C side saying exactly that, and it becomes `Ok(None)` rather
    /// than an error: a board a minute out of the box has no time, which is an answer about the
    /// board and not a failure of the read. An epoch of zero that the C side happened to let
    /// through is treated the same way, because `UNIX_EPOCH + 0` is a 1970 the board never lived in.
    fn clock(&self) -> Result<Option<SystemTime>, HalError> {
        let mut epoch: i64 = 0;

        match unsafe { ffi::hal_system_get_epoch(&mut epoch) } {
            0 if epoch > 0 => Ok(Some(UNIX_EPOCH + Duration::from_secs(epoch as u64))),
            0 => Ok(None),
            ESP_ERR_INVALID_STATE => Ok(None),
            code => Err(HalError::Internal(code)),
        }
    }
}
