//! The management page, served over HTTP.

use crate::error::HalError;
use crate::types::WebStatus;

/// The board's HTTP management server.
///
/// The box is a filesystem and a Wi-Fi client attached to a five-centimetre keyboard, and a browser
/// is the better keyboard. This is the switch that puts a server on the network so that one can be
/// used, and nothing else: *what* is served is the backend's business — the device's is `board_web.c`,
/// whose page manages files and the network — and no part of a page is a type here. Which is why the
/// trait has no method that returns a file, a request, or a route.
///
/// **There is no authentication, and that is a decision rather than an omission.** Anyone on the same
/// network can read and write these files and change this network. The server is off until
/// [`WebBackend::start`] is called, which is why the app that calls it says so on screen before it
/// does: the person holding the box is the one who decides when the box is that open.
pub trait WebBackend: Send + Sync {
    /// Bring the server up on `port`, serving its page and API.
    ///
    /// Idempotent: already up on the same port is `Ok(())` and nothing is restarted. A different port
    /// restarts it there, because a caller asking for 8080 while 80 is held means 8080.
    ///
    /// Returns [`HalError::InvalidArg`] for port 0, and whatever the platform said when the socket
    /// cannot be bound — a port already in use being the one a caller can do something about.
    fn start(&mut self, port: u16) -> Result<(), HalError>;

    /// Take it down, closing every connection it holds — including the browser that asked for the
    /// page it was serving. Already down is `Ok(())`.
    fn stop(&mut self) -> Result<(), HalError>;

    /// Whether it is up, and on which port.
    ///
    /// Cheap and snapshot-like: it is read to draw a switch and an address, not to wait for anything.
    fn status(&self) -> WebStatus;
}
