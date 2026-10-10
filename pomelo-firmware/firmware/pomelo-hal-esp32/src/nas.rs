//! The NAS this board watches, over SSH — **not yet implemented, and it says so**.
//!
//! The trait is here and answers honestly: [`EspNas::watch`] returns
//! [`HalError::NotSupported`][pomelo_hal::HalError::NotSupported] rather than accepting a NAS it
//! cannot reach, and [`EspNas::reading`] reports nothing. That is the whole of this file's job at the
//! moment — the board assembles, the app has something to talk to, and the panel can say "this build
//! cannot do that yet" instead of showing a machine that is永远 "connecting".
//!
//! # What lands here
//!
//! A thin `extern "C"` binding over `components/board_hal/board_nas.c`, which is where the work is:
//! libssh2 over the mbedTLS this image already has, one session kept open across looks (a handshake
//! per look would cost more than the look), and the readings taken by running a handful of commands
//! and reading their output. The raw declarations go in a private `ffi` module at the top of this
//! file, immediately above the safe wrapper that consumes them, like every other domain here.
//!
//! Until then the app of the same name draws its offline state, which is a state it needs anyway: a
//! NAS is a machine that sleeps.

use pomelo_hal::error::HalError;
use pomelo_hal::nas_credentials::NasCredentials;
use pomelo_hal::traits::NasBackend;
use pomelo_hal::types::{NasFault, NasReading, NasStatus};

/// This board's NAS-watching backend. See the module documentation: it does not watch anything yet.
#[derive(Debug, Default)]
pub struct EspNas;

impl EspNas {
    /// The backend. Infallible, like every constructor here: what a board can do is answered by the
    /// calls, not by the assembly, so that a subsystem that has not landed yet is one app's problem
    /// rather than a boot that fails.
    pub fn new() -> Self {
        Self
    }
}

impl NasBackend for EspNas {
    fn watching(&self) -> Option<NasCredentials> {
        None
    }

    fn remember(&mut self, _credentials: &NasCredentials) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    fn forget(&mut self) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    fn refresh(&mut self) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    /// Not [`NasReading::default`]: a board that *cannot* watch a NAS is not a board that has not been
    /// told to. The two want different screens — one is a setting somebody has to fill in, the other is
    /// a firmware somebody has to flash — so this says which it is.
    fn reading(&self) -> NasReading {
        NasReading {
            status: NasStatus::Offline(NasFault::Unsupported),
            ..NasReading::default()
        }
    }
}
