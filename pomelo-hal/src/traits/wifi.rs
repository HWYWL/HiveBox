//! Wi-Fi interface.

use crate::error::HalError;
use crate::types::{ApInfo, ScanState, WifiStatus};

/// Wi-Fi station management.
pub trait WifiBackend: Send + Sync {
    fn init(&mut self) -> Result<(), HalError>;

    /// Do what the file says: read it, and act on what it said.
    ///
    /// The radio comes up **on** unless the file says the switch was turned off, and one connection is
    /// attempted only if the file names a network that is wanted. A board with no file has nothing to
    /// join and comes up on anyway: the switch is where a person starts, and a radio that came up down
    /// would have to be found before it could look for anything.
    ///
    /// Called at two moments, and both of them are "the board is coming up": once by the firmware
    /// when the panel has been drawn, and once by the app when the switch is turned back on. The
    /// second is the same question as the first — the radio is up and the file knows where it
    /// belongs — and asking it any other way would be a second path to a connection, which is what
    /// [`WifiBackend::set_enabled`] is deliberately not.
    ///
    /// Idempotent, because the second call follows a switch that was just flipped: a radio already
    /// on stays on, and a connection already up is connected to again rather than refused.
    ///
    /// No default: a backend that cannot do this must say so, and an `Ok(())` that did nothing is
    /// the one answer nobody could tell from success.
    fn autoconnect(&mut self) -> Result<(), HalError>;

    /// Enable or disable the radio.
    fn set_enabled(&mut self, on: bool) -> Result<(), HalError>;
    fn is_enabled(&self) -> bool;

    /// Begin an asynchronous scan for nearby access points.
    fn scan_start(&mut self) -> Result<(), HalError>;
    /// Current scan progress.
    fn scan_state(&self) -> ScanState;
    /// Results of the most recent completed scan.
    fn scan_results(&self) -> Result<Vec<ApInfo>, HalError>;

    /// Connect to an access point. `password` may be empty for open networks.
    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), HalError>;
    fn disconnect(&mut self) -> Result<(), HalError>;
    /// Current connection status snapshot.
    fn status(&self) -> WifiStatus;

    /// The network this board remembers, if any.
    ///
    /// `None` for a board that has never been on one. The credentials are the **app's**, not the
    /// driver's: `esp_wifi_set_storage(WIFI_STORAGE_RAM)` keeps IDF from writing its own copy, and
    /// what this returns is the file described in [`crate::wifi_credentials`].
    fn saved(&self) -> Option<crate::wifi_credentials::WifiCredentials>;

    /// Remembers `credentials`, replacing whatever was there.
    ///
    /// Only worth calling once a connection has actually come up: the file is what the board does at
    /// boot, and writing it on every attempt would mean a mistyped password destroys the one that
    /// worked.
    fn remember(&mut self, credentials: &crate::wifi_credentials::WifiCredentials)
        -> Result<(), HalError>;

    /// Forgets the remembered network. Not an error if there was none.
    fn forget(&mut self) -> Result<(), HalError>;

    /// Advance simulated asynchronous state. Native event-driven backends can
    /// ignore this (default no-op).
    fn tick(&mut self) {}
}
