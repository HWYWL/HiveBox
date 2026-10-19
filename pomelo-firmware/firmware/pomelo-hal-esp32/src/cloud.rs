//! The board as a client of a cloud provider's API.
//!
//! A thin `extern "C"` binding over `components/board_hal/board_cloud.c`, which signs the request
//! with TC3-HMAC-SHA256, sends it with `esp_http_client`, and renders the answer as a small JSON
//! document. What crosses the boundary is a provider, an action, an instance id, a state, a fault
//! and the bytes of an answer — the same things [`CloudBackend`] names, and nothing about
//! signatures, regions or certificates.
//!
//! The one piece of state that stays here is nothing at all: the C side owns the request, the worker
//! and the answer buffer, so there is no way for two callers to disagree about which request is
//! running.
//!
//! # The answer comes back as text, and is read here
//!
//! What `hal_cloud_take_answer` hands over is the document rather than a parsed struct, and the
//! parser is [`pomelo_hal::cloud_answer`] rather than anything in this file: it is a pure function
//! over a `&str`, it is tested on the host, and the only thing this side does with the bytes is
//! copy them out of the C side's buffer in the same breath as the call that offered them.
//!
//! # Why the region is sent per command
//!
//! `hal_cloud_start` takes one, and it is the row's own: a merged list is every data centre at once,
//! so the machine a button names may not be in the data centre the key was saved with. A list passes
//! an empty region, which the C side reads as "the region saved with the key" — a list scans all of
//! them anyway.

use core::ffi::c_int;
use std::ffi::CString;

use pomelo_hal::cloud_answer;
use pomelo_hal::error::HalError;
use pomelo_hal::traits::CloudBackend;
use pomelo_hal::types::{
    CloudAction, CloudAnswer, CloudFault, CloudKey, CloudProvider, CloudState,
};

/// The C header's vocabulary, mirrored whole.
///
/// `#[allow(dead_code)]` because a code with no arm that reads it yet is still worth having next to
/// the others: this block is a copy of a list in `board_hal.h`, it is read by comparing the two, and
/// a constant left out because nothing branched on it this phase would be a gap in that comparison.
#[allow(dead_code)]
mod ffi {
    use core::ffi::{c_char, c_int};

    /// `HAL_CLOUD_*` providers, from `board_hal.h`.
    pub const PROVIDER_CVM: c_int = 0;
    pub const PROVIDER_LIGHTHOUSE: c_int = 1;
    /// `HAL_CLOUD_ALL`: not a product and the owner of no key, only the target of a merged list.
    /// Named here because [`CloudProvider`] can send it and the C side has to recognise it.
    pub const PROVIDER_ALL: c_int = 2;

    /// `HAL_CLOUD_ACTION_*`, from `board_hal.h`.
    pub const ACTION_LIST: c_int = 0;
    pub const ACTION_START: c_int = 1;
    pub const ACTION_STOP: c_int = 2;
    pub const ACTION_REBOOT: c_int = 3;

    /// What `hal_cloud_state` answers with.
    pub const STATE_IDLE: c_int = 0;
    pub const STATE_RUNNING: c_int = 1;
    pub const STATE_DONE: c_int = 2;

    /// `HAL_CLOUD_FAULT_*`, from `board_hal.h`.
    ///
    /// `HAL_CLOUD_FAULT_NONE` is the one code with no name here: a fault is only ever read off a
    /// failure, and a success has no reason to go looking for one.
    pub const FAULT_ARGS: c_int = 1;
    pub const FAULT_NO_CREDS: c_int = 2;
    pub const FAULT_NO_NETWORK: c_int = 3;
    pub const FAULT_CLOCK: c_int = 4;
    pub const FAULT_UNREACHABLE: c_int = 5;
    pub const FAULT_TLS: c_int = 6;
    pub const FAULT_HTTP: c_int = 7;
    pub const FAULT_API: c_int = 8;
    pub const FAULT_STORAGE: c_int = 9;

    extern "C" {
        pub fn hal_cloud_start(
            provider: c_int,
            action: c_int,
            region: *const c_char,
            instance: *const c_char,
        ) -> c_int;
        pub fn hal_cloud_state() -> c_int;
        pub fn hal_cloud_fault() -> c_int;
        pub fn hal_cloud_take_answer(out: *mut c_char, capacity: usize) -> usize;
        pub fn hal_cloud_cancel();
        pub fn hal_cloud_credentials_get(
            provider: c_int,
            out_masked: *mut c_char,
            capacity: usize,
            out_region: *mut c_char,
            region_capacity: usize,
        ) -> c_int;
    }
}

/// How much of an answer this side will take.
///
/// The C side's own `CLOUD_ANSWER_MAX`, and it has to be at least that or a long list would be
/// truncated on the way across: `hal_cloud_take_answer` copies what fits and says how much it
/// copied, so a smaller buffer here is a list with its last instances missing. On the heap rather
/// than on the stack — six kilobytes is a large frame on a task with eight kilobytes of stack.
const ANSWER_CAPACITY: usize = 6144;

/// The masked secret id's buffer: `HAL_CLOUD_SECRET_ID_MAX`, from `board_hal.h`.
const MASKED_CAPACITY: usize = 64;

/// The region's buffer: `HAL_CLOUD_REGION_MAX`, from `board_hal.h`.
const REGION_CAPACITY: usize = 32;

/// The board's cloud client, as the rest of the tree sees it.
///
/// A unit struct: every byte of the request's state lives on the C side, where the worker that owns
/// it can see it.
pub struct EspCloud;

impl EspCloud {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EspCloud {
    fn default() -> Self {
        Self::new()
    }
}

/// The C side's fault code as the closed list the pages draw.
fn fault_from(code: c_int) -> CloudFault {
    match code {
        ffi::FAULT_ARGS => CloudFault::Args,
        ffi::FAULT_NO_CREDS => CloudFault::NoCredentials,
        ffi::FAULT_NO_NETWORK => CloudFault::NoNetwork,
        ffi::FAULT_CLOCK => CloudFault::ClockNotSet,
        ffi::FAULT_TLS => CloudFault::Tls,
        ffi::FAULT_HTTP => CloudFault::Http,
        ffi::FAULT_API => CloudFault::Api,
        ffi::FAULT_STORAGE => CloudFault::Storage,
        // `FAULT_UNREACHABLE` and everything a C side that has been changed under this one might
        // send: a request that did not come back is a request that did not come back, and the fault
        // that sends a person to look at the network is the honest one to guess.
        _ => CloudFault::Unreachable,
    }
}

/// The text in a NUL-terminated C buffer, as a `String`.
///
/// Lossy rather than strict, like every other string crossing this boundary: a name that is not
/// valid UTF-8 is a name this board cannot draw, and a question mark in a list is a better answer
/// than an empty row.
fn text(buffer: &[u8]) -> String {
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());

    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

impl CloudBackend for EspCloud {
    fn key(&self, provider: CloudProvider) -> CloudKey {
        let mut masked = [0u8; MASKED_CAPACITY];
        let mut region = [0u8; REGION_CAPACITY];

        let code = unsafe {
            ffi::hal_cloud_credentials_get(
                provider.index() as c_int,
                masked.as_mut_ptr() as *mut _,
                masked.len(),
                region.as_mut_ptr() as *mut _,
                region.len(),
            )
        };

        // `ESP_ERR_NOT_FOUND` is the answer for a provider nobody has saved a key for, and it is
        // not an error: an empty `CloudKey` is what that screen is drawn from. So is every other
        // code, since the two buffers were zeroed and read as empty.
        if code != 0 {
            return CloudKey::default();
        }

        CloudKey {
            masked: text(&masked),
            region: text(&region),
        }
    }

    fn start(
        &mut self,
        provider: CloudProvider,
        action: CloudAction,
        instance: &str,
        region: &str,
    ) -> Result<(), HalError> {
        // Bound outside the call so that it outlives the pointer taken to it: the C side copies it
        // before returning, and the worker reads that copy from then on.
        let instance = CString::new(instance).map_err(|_| HalError::InvalidArg)?;
        let region = CString::new(region).map_err(|_| HalError::InvalidArg)?;

        let code = unsafe {
            ffi::hal_cloud_start(
                provider.index() as c_int,
                action.index() as c_int,
                region.as_ptr(),
                instance.as_ptr(),
            )
        };

        HalError::from_code(code)
    }

    fn state(&self) -> CloudState {
        match unsafe { ffi::hal_cloud_state() } {
            ffi::STATE_IDLE => CloudState::Idle,
            ffi::STATE_RUNNING => CloudState::Running,
            ffi::STATE_DONE => CloudState::Done,
            // `HAL_CLOUD_FAILED`, and anything else: a failure with no reason named is still a
            // failure, and reading it as "idle" or "still running" would leave a page waiting for an
            // answer that is never coming.
            _ => CloudState::Failed(fault_from(unsafe { ffi::hal_cloud_fault() })),
        }
    }

    fn take_answer(&mut self) -> Result<CloudAnswer, HalError> {
        let mut buffer = vec![0u8; ANSWER_CAPACITY];

        let length = unsafe {
            ffi::hal_cloud_take_answer(buffer.as_mut_ptr() as *mut _, buffer.len())
        };

        // Zero for a request still in flight and for an answer that has already been read, which
        // are the same thing to a page: there is nothing here.
        if length == 0 {
            return Err(HalError::Busy);
        }

        let length = length.min(buffer.len());
        Ok(cloud_answer::parse(&text(&buffer[..length])))
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        unsafe { ffi::hal_cloud_cancel() };
        Ok(())
    }
}
