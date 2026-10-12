//! The machines on the network beside this board.

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
/// # The associations are kept like Wi-Fi networks'
///
/// [`NasBackend::devices`] reads them back and [`NasBackend::remember`] writes one down, which is
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
    /// Every machine this board has been told about, in the order it was told about them.
    ///
    /// A list rather than one, because one board can watch more than one box: the app draws this and a
    /// person picks from it. Read from where they were written down rather than cached, like the Wi-Fi
    /// credentials — a board that has been told about a machine says so. A board that has never been
    /// told about any answers an empty list, which is the same state as [`NasStatus::Unconfigured`]
    /// and not a failure.
    ///
    /// [`NasStatus::Unconfigured`]: crate::types::NasStatus::Unconfigured
    fn devices(&self) -> Vec<NasCredentials>;

    /// The machine this board is *looking at*, or `None` if the list is empty.
    ///
    /// One of [`NasBackend::devices`], once one of them has been chosen — which is what
    /// [`NasBackend::remember`] does, and what a person does by picking a row. Which one is *not*
    /// written down: the choice is made again each time somebody opens the page, and a board that
    /// watched nothing until then is what the start-and-poll note above describes anyway.
    fn watching(&self) -> Option<NasCredentials>;

    /// Adds a machine to the list, or updates the one already there, and watches it.
    ///
    /// "Updates the one already there" is by host: saving the same address twice is one device whose
    /// details were corrected, not two devices one of which is a mistake.
    ///
    /// Returns whatever the platform said if it cannot watch anything at all — a board with no SSH
    /// client in its image being the honest case, and one the app has to be able to say out loud
    /// rather than showing a machine that never answers.
    ///
    /// Watching a *different* machine drops whatever session the old one had and forgets the reading
    /// with it, because a number from the old machine drawn under the new machine's name is worse than
    /// no number at all.
    fn remember(&mut self, credentials: &NasCredentials) -> Result<(), HalError>;

    /// Drops `host` from the list. A host that is not in it is `Ok(())`.
    ///
    /// Dropping the machine being watched leaves the board watching nothing: the next
    /// [`NasBackend::watching`] answers `None` and [`NasBackend::reading`] says
    /// [`NasStatus::Unconfigured`] — a board whose list ran out, rather than one whose machine went
    /// wrong.
    ///
    /// [`NasStatus::Unconfigured`]: crate::types::NasStatus::Unconfigured
    fn forget(&mut self, host: &str) -> Result<(), HalError>;

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

    /// Asks for every machine in the list to be checked, so that [`NasBackend::answering`] can say which
    /// of them are there.
    ///
    /// A check is not a look. A look logs in, runs a command and reads what it printed — a session, and a
    /// second of one machine's time. A check opens a socket to the machine's SSH port and closes it
    /// again, and what it answers is whether the board can reach the machine at all. That is the question
    /// a *list* of machines is asked before any other, and it is cheap enough to keep asking while the
    /// list is on screen — where a look is not, which is why the two are separate calls rather than one
    /// with a flag.
    ///
    /// Like [`NasBackend::refresh`], this only *asks*: the sockets are opened and closed on the backend's
    /// own thread, and the answers are read with [`NasBackend::answering`]. A backend that cannot check
    /// anything does nothing here, and has nothing to say there.
    fn check(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    /// Whether `host` answered the last check: `Some(true)` if the board reached it, `Some(false)` if it
    /// did not, and `None` if no check has been made — or if this backend does not make them.
    ///
    /// Cheap, like [`NasBackend::reading`], and safe to call for every row of a list on every frame.
    ///
    /// A machine whose *session* also fails is described better by [`NasBackend::reading`] — a port that
    /// answers is not a login that works — so a page with both in hand says the reading's fault, and
    /// falls back to this for the machines it has no reading for. Which is most of them: the backend
    /// looks at one machine at a time and checks all of them.
    fn answering(&self, _host: &str) -> Option<bool> {
        None
    }
}
