//! The NAS on the network beside this board.

use crate::error::HalError;
use crate::nas_credentials::NasCredentials;
use crate::types::NasReading;

/// Watching another computer's numbers from this one.
///
/// The board is a panel that sits on a desk next to a NAS, and the question it exists to answer is
/// "is that thing all right" — a question about a *different* machine, asked across the network, over
/// a protocol somebody has to log into. Everything awkward about that lives behind this trait: the
/// session, its key exchange, its host key and the commands whose output somebody has to read. An app
/// above it sees a [`NasReading`] and a way to point at a NAS.
///
/// # Start, then poll
///
/// [`NasBackend::refresh`] never blocks and reads nothing: it says "look again when you can" and
/// returns. That is not a style choice — a look at another machine is hundreds of milliseconds of key
/// exchange and shell output, and a UI frame cannot spend that. What the last look found is
/// [`NasBackend::reading`], which is a snapshot and cheap, so an app calls one on a timer and reads
/// the other once a frame. This is the start-and-poll shape the backends with slow work in them all
/// use, and it matters more here than anywhere else: the day the NAS is asleep, the panel keeps
/// drawing (an old reading, and a line saying how old) instead of stopping.
///
/// # The association is kept like a Wi-Fi network's
///
/// [`NasBackend::remember`] writes it down and [`NasBackend::watching`] reads it back, which is
/// [`WifiBackend`][crate::traits::WifiBackend]'s pair of the same names doing the same job — "this box
/// reaches that machine, and here is how". The file *shape* is shared too (see
/// [`crate::nas_credentials`]), and what is deliberately not shared is who touches it: an app asks
/// this backend, and a backend knows how to write to its own platform's flash. On the device that is
/// not `std::fs` at all — the C side has the file, for the reason `crate::music_settings` says at
/// length — and an app that had loaded the file itself would work on a desktop and quietly do nothing
/// on the board.
///
/// # Why this is a trait and not a socket
///
/// Because the *transport* is the part most likely to change its mind. It was going to be an HTTP
/// scrape of a metrics daemon on the NAS; it is now an SSH login to a box that is already running one.
/// Both are answers to "ask that machine for its numbers", and an app written against either would
/// have had to be rewritten for the other. An app written against `NasBackend` is not.
pub trait NasBackend: Send + Sync {
    /// The NAS this board is watching, or `None` if it is watching nothing.
    ///
    /// Read from where it was written down rather than cached, like the Wi-Fi credentials: a board
    /// that has been told to watch a machine says so, and one that never has says that instead.
    fn watching(&self) -> Option<NasCredentials>;

    /// Points this board at a NAS, and writes it down.
    ///
    /// Returns whatever the platform said if it cannot watch anything at all — a board with no SSH
    /// client in its image being the honest case, and one the app has to be able to say out loud
    /// rather than showing a machine that never answers.
    ///
    /// Pointing it at a different NAS drops whatever session the old one had and forgets the reading
    /// with it, because a number from the old machine drawn under the new machine's name is worse than
    /// no number at all.
    fn remember(&mut self, credentials: &NasCredentials) -> Result<(), HalError>;

    /// Forgets the NAS this board was watching. Already watching nothing is `Ok(())`.
    fn forget(&mut self) -> Result<(), HalError>;

    /// Asks for a fresh reading. Returns at once; the answer arrives when it arrives.
    ///
    /// A look already in flight is not a second look: the backend is watching one machine at one
    /// instant, and a caller on a timer that overlapped with a slow attempt would otherwise leave a
    /// queue of attempts, each overtaking the last one's answer. What a caller can rely on is
    /// [`NasBackend::reading`] being *a* look, and [`NasReading::age`] saying how recent.
    fn refresh(&mut self) -> Result<(), HalError>;

    /// What the last look found, and how it went.
    ///
    /// Cheap, snapshot-like, and safe to call every frame. A backend that has not been pointed at a
    /// NAS answers [`NasStatus::Unconfigured`][crate::types::NasStatus::Unconfigured], which is not a
    /// failure and should not be drawn as one.
    fn reading(&self) -> NasReading;
}
