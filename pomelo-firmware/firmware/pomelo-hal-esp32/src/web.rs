//! Management-server backend (ESP-IDF `esp_http_server`, via
//! `firmware/components/board_hal/board_web.c`).
//!
//! The thinnest file in this directory, and it is meant to be. The server, the page and every route
//! are C, because that is where the sockets are, where the filesystem is and where the Wi-Fi state
//! already lives. What crosses this line is a switch and a port — the trait's whole surface.
//!
//! The consequence worth knowing: **the device is the only place this does anything.** On a host
//! there is no page to serve and no network to serve it on, which is what `sim::SimWeb` keeps in
//! step with the app rather than with the hardware.

use pomelo_hal::{HalError, WebBackend, WebStatus};

mod ffi {
    extern "C" {
        pub fn hal_web_start(port: u16) -> i32;
        pub fn hal_web_stop() -> i32;
        pub fn hal_web_is_running() -> bool;
        pub fn hal_web_get_port() -> u16;
    }
}

/// The board's HTTP management server.
///
/// No state of its own: whether the server is up is the C side's, and a Rust copy of it would be a
/// second answer to the same question — the one thing this facade must not be.
pub struct EspWeb;

impl EspWeb {
    pub const fn new() -> Self {
        Self
    }
}

impl WebBackend for EspWeb {
    fn start(&mut self, port: u16) -> Result<(), HalError> {
        // The C side refuses zero too, but a caller asking for no port should hear it as the
        // argument it got wrong rather than as a socket error it cannot read.
        if port == 0 {
            return Err(HalError::InvalidArg);
        }

        unsafe { HalError::from_code(ffi::hal_web_start(port)) }
    }

    fn stop(&mut self) -> Result<(), HalError> {
        unsafe { HalError::from_code(ffi::hal_web_stop()) }
    }

    fn status(&self) -> WebStatus {
        // Both reads are of the same value under the same lock on the C side, so the pair cannot be
        // a running server on the port of the one that was stopped between the two calls.
        unsafe {
            WebStatus {
                running: ffi::hal_web_is_running(),
                port: ffi::hal_web_get_port(),
            }
        }
    }
}
