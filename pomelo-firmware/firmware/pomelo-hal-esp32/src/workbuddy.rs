//! A thin `extern "C"` binding over `components/board_hal/board_workbuddy.c`, which holds the
//! token, POSTs to the account with `esp_http_client`, and renders the answer as ten short lines.
//! What crosses the boundary is an action, a state, a fault and the bytes of an answer — the same
//! things [`WorkBuddyBackend`] names, and nothing about TLS, headers or JWTs.
//!
//! The one piece of state that stays here is nothing at all: the C side owns the request, the worker
//! and the answer buffer, so there is no way for two callers to disagree about which request is
//! running.
//!
//! # The answer comes back as text, and is read here
//!
//! What `hal_workbuddy_take_answer` hands over is the document rather than a parsed struct, and the
//! parser is [`pomelo_hal::workbuddy_answer`] rather than anything in this file: it is a pure
//! function over a `&str`, it is tested on the host, and the only thing this side does with the
//! bytes is copy them out of the C side's buffer in the same breath as the call that offered them.
//!
//! # The token never comes back out
//!
//! `hal_workbuddy_credentials_get` answers with a mask and a host, so there is no route from a page
//! to the token: a page that could draw it would be a page that leaks it into a screenshot.

use core::ffi::c_int;
use std::ffi::CString;

use pomelo_hal::error::HalError;
use pomelo_hal::traits::WorkBuddyBackend;
use pomelo_hal::types::{
    WorkBuddyAccount, WorkBuddyAction, WorkBuddyAnswer, WorkBuddyFault, WorkBuddyState,
};
use pomelo_hal::workbuddy_answer;

/// The C header's vocabulary, mirrored whole.
///
/// A copy of a list in `board_hal.h`, read by comparing the two: a constant left out because
/// nothing branched on it yet would be a gap in that comparison.
#[allow(dead_code)]
mod ffi {
    use core::ffi::{c_char, c_int};

    /// `HAL_WORKBUDDY_ACTION_*`, from `board_hal.h`.
    pub const ACTION_STATUS: c_int = 0;
    pub const ACTION_CHECKIN: c_int = 1;

    /// `HAL_WORKBUDDY_*` states.
    pub const STATE_IDLE: c_int = 0;
    pub const STATE_RUNNING: c_int = 1;
    pub const STATE_DONE: c_int = 2;
    pub const STATE_FAILED: c_int = -1;

    /// `HAL_WORKBUDDY_FAULT_*`.
    pub const FAULT_NONE: c_int = 0;
    pub const FAULT_ARGS: c_int = 1;
    pub const FAULT_NO_CREDS: c_int = 2;
    pub const FAULT_NO_NETWORK: c_int = 3;
    pub const FAULT_UNREACHABLE: c_int = 4;
    pub const FAULT_TLS: c_int = 5;
    pub const FAULT_HTTP: c_int = 6;
    pub const FAULT_AUTH: c_int = 7;
    pub const FAULT_API: c_int = 8;
    pub const FAULT_STORAGE: c_int = 9;

    extern "C" {
        pub fn hal_workbuddy_start(action: c_int) -> c_int;
        pub fn hal_workbuddy_state() -> c_int;
        pub fn hal_workbuddy_fault() -> c_int;
        pub fn hal_workbuddy_take_answer(out: *mut c_char, capacity: usize) -> usize;
        pub fn hal_workbuddy_cancel();
        pub fn hal_workbuddy_credentials_get(
            out_masked: *mut c_char,
            capacity: usize,
            out_host: *mut c_char,
            host_capacity: usize,
        ) -> c_int;
        pub fn hal_workbuddy_credentials_set(
            token: *const c_char,
            refresh: *const c_char,
            host: *const c_char,
        ) -> c_int;
        pub fn hal_workbuddy_credentials_forget() -> c_int;
    }
}

/// `ESP_ERR_INVALID_ARG`, `ESP_ERR_INVALID_STATE` and `ESP_ERR_NOT_FOUND`, from `esp_err.h`.
///
/// Named here because they are the three the C side uses to say "your argument", "not now" and
/// "nothing there", which are three different things for a caller and one number for a page.
const ESP_ERR_INVALID_ARG: c_int = 0x102;
const ESP_ERR_INVALID_STATE: c_int = 0x103;
const ESP_ERR_NOT_FOUND: c_int = 0x105;

/// How much of an answer this side will take: the C side's `WB_ANSWER_MAX`.
const ANSWER_CAPACITY: usize = 1024;

/// The mask's buffer. Four characters and an ellipsis, and room does not hurt.
const MASKED_CAPACITY: usize = 16;

/// The host's buffer: `HAL_WORKBUDDY_HOST_MAX`.
const HOST_CAPACITY: usize = 64;

/// The board's WorkBuddy client, as the rest of the tree sees it.
///
/// A unit struct: every byte of the request's state lives on the C side, where the worker that owns
/// it can see it.
pub struct EspWorkBuddy;

impl EspWorkBuddy {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EspWorkBuddy {
    fn default() -> Self {
        Self::new()
    }
}

/// The C side's fault vocabulary, as the closed list this crate has for it.
fn fault_from(code: c_int) -> WorkBuddyFault {
    match code {
        ffi::FAULT_ARGS => WorkBuddyFault::Args,
        ffi::FAULT_NO_CREDS => WorkBuddyFault::NoToken,
        ffi::FAULT_NO_NETWORK => WorkBuddyFault::NoNetwork,
        ffi::FAULT_TLS => WorkBuddyFault::Tls,
        ffi::FAULT_HTTP => WorkBuddyFault::Http,
        ffi::FAULT_AUTH => WorkBuddyFault::Auth,
        ffi::FAULT_API => WorkBuddyFault::Api,
        ffi::FAULT_STORAGE => WorkBuddyFault::Storage,
        // `FAULT_UNREACHABLE` and anything a C side changed under this one might add: a failure
        // with no reason this build knows is still a failure.
        _ => WorkBuddyFault::Unreachable,
    }
}

/// The C string in a buffer, as a Rust string.
fn text(buffer: &[u8]) -> String {
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());

    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

/// What the three `esp_err_t` codes this file sees mean to a caller of the trait.
fn outcome(code: c_int) -> Result<(), HalError> {
    match code {
        0 => Ok(()),
        ESP_ERR_INVALID_ARG => Err(HalError::InvalidArg),
        ESP_ERR_INVALID_STATE => Err(HalError::Busy),
        // "Nothing there" is not a failure for `forget`: a board with no token is where every board
        // starts, and saying so twice is not an error.
        ESP_ERR_NOT_FOUND => Ok(()),
        other => Err(HalError::Internal(other)),
    }
}

impl WorkBuddyBackend for EspWorkBuddy {
    fn account(&self) -> WorkBuddyAccount {
        let mut masked = [0u8; MASKED_CAPACITY];
        let mut host = [0u8; HOST_CAPACITY];

        let code = unsafe {
            ffi::hal_workbuddy_credentials_get(
                masked.as_mut_ptr() as *mut _,
                masked.len(),
                host.as_mut_ptr() as *mut _,
                host.len(),
            )
        };

        // `ESP_ERR_NOT_FOUND` is "no token saved", which is the empty account rather than an error:
        // the buffers were zeroed, and an empty mask is what that screen is drawn from.
        if code != 0 {
            return WorkBuddyAccount::default();
        }

        WorkBuddyAccount {
            masked: text(&masked),
            host: text(&host),
        }
    }

    fn remember(
        &mut self,
        token: &str,
        refresh: Option<&str>,
        host: Option<&str>,
    ) -> Result<(), HalError> {
        // Bound outside the call so that they outlive the pointers taken to them: the C side copies
        // them before returning.
        let token = CString::new(token).map_err(|_| HalError::InvalidArg)?;
        let refresh = CString::new(refresh.unwrap_or("")).map_err(|_| HalError::InvalidArg)?;
        let host = CString::new(host.unwrap_or("")).map_err(|_| HalError::InvalidArg)?;

        let code = unsafe {
            ffi::hal_workbuddy_credentials_set(token.as_ptr(), refresh.as_ptr(), host.as_ptr())
        };

        outcome(code)
    }

    fn forget(&mut self) -> Result<(), HalError> {
        outcome(unsafe { ffi::hal_workbuddy_credentials_forget() })
    }

    fn start(&mut self, action: WorkBuddyAction) -> Result<(), HalError> {
        let action = match action {
            WorkBuddyAction::Status => ffi::ACTION_STATUS,
            WorkBuddyAction::Checkin => ffi::ACTION_CHECKIN,
        };

        outcome(unsafe { ffi::hal_workbuddy_start(action) })
    }

    fn state(&self) -> WorkBuddyState {
        match unsafe { ffi::hal_workbuddy_state() } {
            ffi::STATE_IDLE => WorkBuddyState::Idle,
            ffi::STATE_RUNNING => WorkBuddyState::Running,
            ffi::STATE_DONE => WorkBuddyState::Done,
            // `HAL_WORKBUDDY_FAILED`, and anything else: reading it as "idle" would leave a page
            // waiting for an answer that is never coming.
            _ => WorkBuddyState::Failed(fault_from(unsafe { ffi::hal_workbuddy_fault() })),
        }
    }

    fn take_answer(&mut self) -> Result<WorkBuddyAnswer, HalError> {
        let mut buffer = vec![0u8; ANSWER_CAPACITY];

        let length =
            unsafe { ffi::hal_workbuddy_take_answer(buffer.as_mut_ptr() as *mut _, buffer.len()) };

        // Zero for a request still in flight and for an answer already read, which are the same
        // thing to a page: there is nothing here.
        if length == 0 {
            return Err(HalError::Busy);
        }

        let length = length.min(buffer.len());

        Ok(workbuddy_answer::parse(&text(&buffer[..length])))
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        unsafe { ffi::hal_workbuddy_cancel() };
        Ok(())
    }
}
