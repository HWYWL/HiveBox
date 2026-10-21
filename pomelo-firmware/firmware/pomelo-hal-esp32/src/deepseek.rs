//! A thin `extern "C"` binding over `components/board_hal/board_deepseek.c`, which holds the two
//! credentials, GETs two billing documents, and renders the answer as ten short lines.
//!
//! What crosses the boundary is a state, a fault and the bytes of an answer — the same things
//! [`DeepSeekBackend`] names, and nothing about TLS, headers or bearer tokens.
//!
//! The credentials never come back out: `hal_deepseek_credentials` answers with a mask and a bool,
//! so there is no route from a page to a key.

use core::ffi::c_int;
use std::ffi::CString;

use pomelo_hal::deepseek_answer;
use pomelo_hal::error::HalError;
use pomelo_hal::traits::DeepSeekBackend;
use pomelo_hal::types::{DeepSeekAccount, DeepSeekAnswer, DeepSeekFault, DeepSeekState};

/// `board_hal.h` 的那套词，整套抄过来 —— 抄漏一个就是一个对不上的地方。
#[allow(dead_code)]
mod ffi {
    use core::ffi::{c_char, c_int};

    pub const STATE_IDLE: c_int = 0;
    pub const STATE_RUNNING: c_int = 1;
    pub const STATE_DONE: c_int = 2;
    pub const STATE_FAILED: c_int = -1;

    pub const FAULT_NONE: c_int = 0;
    pub const FAULT_FAILED: c_int = 1;
    pub const FAULT_ARGS: c_int = 2;
    pub const FAULT_NO_CREDS: c_int = 3;
    pub const FAULT_NO_NETWORK: c_int = 4;
    pub const FAULT_UNREACHABLE: c_int = 5;
    pub const FAULT_TLS: c_int = 6;
    pub const FAULT_HTTP: c_int = 7;
    pub const FAULT_AUTH: c_int = 8;
    pub const FAULT_API: c_int = 9;
    pub const FAULT_STORAGE: c_int = 10;

    extern "C" {
        pub fn hal_deepseek_start() -> c_int;
        pub fn hal_deepseek_state() -> c_int;
        pub fn hal_deepseek_fault() -> c_int;
        pub fn hal_deepseek_take_answer(out: *mut c_char, capacity: usize) -> c_int;
        pub fn hal_deepseek_cancel() -> c_int;
        pub fn hal_deepseek_credentials_set(
            key: *const c_char,
            user_token: *const c_char,
        ) -> c_int;
        pub fn hal_deepseek_credentials_forget() -> c_int;
        pub fn hal_deepseek_credentials(
            masked: *mut c_char,
            capacity: usize,
            has_user_token: *mut bool,
        );
    }
}

/// `esp_err.h` 里的三个码：它们是 C 侧说「你的参数」「现在不行」「没有」的三种方式。
const ESP_ERR_INVALID_ARG: c_int = 0x102;
const ESP_ERR_INVALID_STATE: c_int = 0x103;
const ESP_ERR_NOT_FOUND: c_int = 0x105;

/// C 侧的 `DS_ANSWER_MAX`。
const ANSWER_CAPACITY: usize = 1024;

/// 掩码的缓冲：三个字符、一个省略号、四个字符。
const MASKED_CAPACITY: usize = 20;

/// 板子上的 DeepSeek 客户端。
pub struct EspDeepSeek;

impl EspDeepSeek {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EspDeepSeek {
    fn default() -> Self {
        Self::new()
    }
}

fn fault_from(code: c_int) -> DeepSeekFault {
    match code {
        ffi::FAULT_NO_CREDS => DeepSeekFault::NoKey,
        ffi::FAULT_NO_NETWORK => DeepSeekFault::NoNetwork,
        ffi::FAULT_UNREACHABLE => DeepSeekFault::Unreachable,
        ffi::FAULT_TLS => DeepSeekFault::Tls,
        ffi::FAULT_HTTP => DeepSeekFault::Http,
        ffi::FAULT_AUTH => DeepSeekFault::Auth,
        ffi::FAULT_API => DeepSeekFault::Api,
        ffi::FAULT_STORAGE => DeepSeekFault::Storage,
        // 不认识的码还是一个失败：这一版没有更细的说法，但那不是「没事」。
        _ => DeepSeekFault::Failed,
    }
}

fn error_of(code: c_int) -> Result<(), HalError> {
    match code {
        ESP_ERR_INVALID_ARG => Err(HalError::InvalidArg),
        ESP_ERR_INVALID_STATE => Err(HalError::Busy),
        ESP_ERR_NOT_FOUND => Err(HalError::Internal(ESP_ERR_NOT_FOUND)),
        _ => Ok(()),
    }
}

impl DeepSeekBackend for EspDeepSeek {
    fn account(&self) -> DeepSeekAccount {
        let mut masked = [0 as core::ffi::c_char; MASKED_CAPACITY];
        let mut has_user_token = false;

        unsafe {
            ffi::hal_deepseek_credentials(
                masked.as_mut_ptr(),
                MASKED_CAPACITY,
                &mut has_user_token,
            );
        }

        let text = unsafe { core::ffi::CStr::from_ptr(masked.as_ptr()) }
            .to_string_lossy()
            .into_owned();

        DeepSeekAccount {
            masked_key: text,
            has_user_token,
        }
    }

    fn remember(&mut self, key: &str, user_token: Option<&str>) -> Result<(), HalError> {
        let key = CString::new(key).map_err(|_| HalError::InvalidArg)?;
        let user_token = match user_token {
            Some(token) => Some(CString::new(token).map_err(|_| HalError::InvalidArg)?),
            None => None,
        };

        let code = unsafe {
            ffi::hal_deepseek_credentials_set(
                key.as_ptr(),
                user_token
                    .as_ref()
                    .map_or(core::ptr::null(), |token| token.as_ptr()),
            )
        };

        error_of(code)
    }

    fn forget(&mut self) -> Result<(), HalError> {
        error_of(unsafe { ffi::hal_deepseek_credentials_forget() })
    }

    fn start(&mut self) -> Result<(), HalError> {
        error_of(unsafe { ffi::hal_deepseek_start() })
    }

    fn state(&self) -> DeepSeekState {
        match unsafe { ffi::hal_deepseek_state() } {
            ffi::STATE_RUNNING => DeepSeekState::Running,
            ffi::STATE_DONE => DeepSeekState::Done,
            ffi::STATE_FAILED => {
                DeepSeekState::Failed(fault_from(unsafe { ffi::hal_deepseek_fault() }))
            }
            _ => DeepSeekState::Idle,
        }
    }

    fn take_answer(&mut self) -> Result<DeepSeekAnswer, HalError> {
        let mut answer = [0 as core::ffi::c_char; ANSWER_CAPACITY];
        let code = unsafe { ffi::hal_deepseek_take_answer(answer.as_mut_ptr(), ANSWER_CAPACITY) };

        error_of(code)?;

        let text = unsafe { core::ffi::CStr::from_ptr(answer.as_ptr()) }.to_string_lossy();

        Ok(deepseek_answer::parse(&text))
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        error_of(unsafe { ffi::hal_deepseek_cancel() })
    }
}
