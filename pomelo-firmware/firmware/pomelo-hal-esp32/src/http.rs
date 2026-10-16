//! The board as a client of somebody else's HTTP service.
//!
//! A thin `extern "C"` binding over `components/board_hal/board_http.c`, which does the fetch on a
//! task of its own with `esp_http_client` and ESP-TLS. What crosses the boundary is a URL, where the
//! answer goes, a state, a fault and the bytes of an answer — the same things [`HttpBackend`] names,
//! and nothing about sockets, certificates or redirects. The one piece of state that stays here is
//! nothing at all: the C side owns the request, the worker and the body buffer, so there is no way for
//! two tasks to disagree about which request is running.
//!
//! A [`Sink::File`] request is the one case where the answer does not cross at all: the C side writes
//! it to the path with `fopen` as the bytes arrive, and hands back a NULL body. The length is still
//! the answer's, and the file is the only place its bytes are.
//!
//! A [`Sink::Ota`] request is the other: `esp_ota_write` takes the bytes into the app slot this image
//! is not running from, and the NULL body that comes back is the same shape. The one thing this side
//! has to know about it is that it is not asked for by a *path* — which partition is `esp_ota_ops`'s
//! answer and not a caller's, so the C side is handed no path at all. See `board_http.c` for what
//! makes a slot bootable, and for why a request that fails leaves the boot partition where it was.
//!
//! # Why not Rust
//!
//! The NAS backend ([`crate::nas`]) served itself over `std::net` + `sunset` — SSH written in Rust,
//! over a socket that `board_socket.c` hands to `std::net`. The same route was not taken here because
//! HTTPS is not a socket with a protocol on top of it but a *certificate chain*: verifying one means
//! carrying a root bundle and an X.509 parser, and ESP-IDF already carries both for the management
//! page's server. `esp_http_client` adds redirects, chunked bodies and content-length handling on top
//! of that, all of it already in the image.
//!
//! # The clock, and why an https request can fail before it starts
//!
//! mbedTLS judges every certificate against the system clock, and a board that has never been told
//! the time reads 1970 — before every certificate ever issued. The C side answers such a request with
//! [`HttpFault::ClockNotSet`] without opening a socket at all, and `board_time.c` is what stops that
//! happening: it starts SNTP the moment the station has an address.

use core::ffi::c_int;
use core::ptr;
use std::ffi::CString;

use pomelo_hal::error::HalError;
use pomelo_hal::traits::HttpBackend;
use pomelo_hal::types::{HttpFault, HttpProgress, HttpResponse, HttpState, Sink};

mod ffi {
    use core::ffi::{c_char, c_int};

    /// `HAL_HTTP_*`, from `board_hal.h`.
    pub const STATE_IDLE: c_int = 0;
    pub const STATE_RUNNING: c_int = 1;
    pub const STATE_DONE: c_int = 2;

    /// `HAL_HTTP_SINK_*`, from `board_hal.h`.
    pub const SINK_MEMORY: c_int = 0;
    pub const SINK_FILE: c_int = 1;
    pub const SINK_OTA: c_int = 2;

    /// `HAL_HTTP_FAULT_*`, from `board_hal.h`.
    ///
    /// `HAL_HTTP_FAULT_NONE` is the one code with no name here: a fault is only ever read off a
    /// failure, and a success has no reason to go looking for one.
    pub const FAULT_NO_NETWORK: c_int = 1;
    pub const FAULT_CLOCK: c_int = 2;
    pub const FAULT_UNREACHABLE: c_int = 3;
    pub const FAULT_TIMEOUT: c_int = 4;
    pub const FAULT_TLS: c_int = 5;
    pub const FAULT_TOO_LARGE: c_int = 6;
    pub const FAULT_STORAGE: c_int = 7;

    extern "C" {
        pub fn hal_http_start_to(url: *const c_char, sink: c_int, path: *const c_char) -> c_int;
        pub fn hal_http_get_state() -> c_int;
        pub fn hal_http_get_fault() -> c_int;
        pub fn hal_http_get_progress(out_received: *mut usize, out_total: *mut usize) -> c_int;
        pub fn hal_http_take(
            out_status: *mut c_int,
            out_body: *mut *const u8,
            out_length: *mut usize,
        ) -> c_int;
        pub fn hal_http_cancel() -> c_int;
    }
}

/// The board's HTTP client, as the rest of the tree sees it.
///
/// A unit struct: every byte of the request's state lives on the C side, where the worker that owns
/// it can see it.
pub struct EspHttp;

impl EspHttp {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EspHttp {
    fn default() -> Self {
        Self::new()
    }
}

/// An `esp_err_t` as the HAL's error, for the three codes this boundary can actually return.
///
/// `ESP_ERR_NO_MEM` is left as [`HalError::Internal`] with its code, which is the honest answer for a
/// request that could not be started at all: there is nothing a page can do about it, and the code is
/// what a person reading a log needs. See `board_hal.h` for which failure each code means.
fn error_from(code: c_int) -> HalError {
    match code {
        0x102 => HalError::InvalidArg, // ESP_ERR_INVALID_ARG
        0x103 => HalError::Busy,       // ESP_ERR_INVALID_STATE
        0x107 => HalError::Timeout,    // ESP_ERR_TIMEOUT
        other => HalError::Internal(other),
    }
}

/// The C side's fault code as the closed list the pages draw.
fn fault_from(code: c_int) -> HttpFault {
    match code {
        ffi::FAULT_NO_NETWORK => HttpFault::NoNetwork,
        ffi::FAULT_CLOCK => HttpFault::ClockNotSet,
        ffi::FAULT_TIMEOUT => HttpFault::Timeout,
        ffi::FAULT_TLS => HttpFault::Tls,
        ffi::FAULT_TOO_LARGE => HttpFault::TooLarge,
        ffi::FAULT_UNREACHABLE => HttpFault::Unreachable,
        ffi::FAULT_STORAGE => HttpFault::Storage,
        // `FAULT_NONE` cannot be reached from a failure and anything else is a code this side has not
        // been taught. Both are "we could not reach it", which is the fault that sends a person to
        // look at the network rather than at anything this board controls.
        _ => HttpFault::Unreachable,
    }
}

impl HttpBackend for EspHttp {
    fn start_to(&mut self, url: &str, sink: Sink) -> Result<(), HalError> {
        let url = CString::new(url).map_err(|_| HalError::InvalidArg)?;

        // The path is bound outside the call so that it outlives the pointer taken to it — the C side
        // copies it before returning, and the worker reads that copy from then on.
        let (sink_code, path) = match &sink {
            Sink::Memory => (ffi::SINK_MEMORY, None),
            Sink::File(path) => (
                ffi::SINK_FILE,
                Some(CString::new(path.as_str()).map_err(|_| HalError::InvalidArg)?),
            ),
            // No path, and not because one was forgotten: which slot an update goes into is the C
            // side's to pick, since only it can ask the partition table which one is not in use.
            Sink::Ota => (ffi::SINK_OTA, None),
        };
        let raw_path = path.as_ref().map_or(ptr::null(), |path| path.as_ptr());

        let code = unsafe { ffi::hal_http_start_to(url.as_ptr(), sink_code, raw_path) };
        if code == 0 {
            Ok(())
        } else {
            Err(error_from(code))
        }
    }

    fn state(&self) -> HttpState {
        let raw = unsafe { ffi::hal_http_get_state() };

        match raw {
            ffi::STATE_IDLE => HttpState::Idle,
            ffi::STATE_RUNNING => HttpState::Running,
            ffi::STATE_DONE => HttpState::Done,
            // `HAL_HTTP_FAILED`, and anything a C side that has been changed under this one might
            // send: a failure with no reason named is still a failure, and reading it as "idle" or
            // "still running" would leave a page waiting for an answer that is never coming.
            _ => HttpState::Failed(fault_from(unsafe { ffi::hal_http_get_fault() })),
        }
    }

    fn progress(&self) -> Option<HttpProgress> {
        let mut received: usize = 0;
        let mut total: usize = 0;

        let code = unsafe { ffi::hal_http_get_progress(&mut received, &mut total) };
        if code != 0 {
            // Nothing in flight. The C side says so itself rather than this side guessing from a pair
            // of zeros, which is also what a download that has *started* but not yet read its headers
            // looks like — and that one is a `Some(0)` a bar should draw empty.
            return None;
        }

        Some(HttpProgress {
            received: received as u64,
            total: total as u64,
        })
    }

    fn take_result(&mut self) -> Result<HttpResponse, HalError> {
        let mut status: c_int = 0;
        let mut body: *const u8 = ptr::null();
        let mut length: usize = 0;

        let code = unsafe { ffi::hal_http_take(&mut status, &mut body, &mut length) };
        if code != 0 {
            return Err(error_from(code));
        }

        // Copied out of the C side's buffer in the same breath as the call that offered it: the
        // pointer is only good until the next request, which is what `hal_http_take` documents.
        let body = if body.is_null() || length == 0 {
            Vec::new()
        } else {
            unsafe { core::slice::from_raw_parts(body, length) }.to_vec()
        };

        Ok(HttpResponse {
            // `u16` rather than `c_int`: the status line is three digits by definition, and the
            // truncation is the C side's `int32_t` being wider than it needs to be, not a real range.
            status: status as u16,
            body,
        })
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        HalError::from_code(unsafe { ffi::hal_http_cancel() })
    }
}
