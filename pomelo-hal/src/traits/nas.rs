//! The machines on the network beside this board.

use crate::error::HalError;
use crate::nas_credentials::NasCredentials;
use crate::types::{NasReading, NasShell};

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
///
/// # A terminal is the other kind of session
///
/// A look ([`NasBackend::refresh`]) opens a connection, runs one command and closes it again — *every
/// time*, on purpose. A session kept between looks would have to survive the NAS rebooting, the router
/// dropping a connection it considers idle, and the server's own `ClientAliveInterval`; all three are
/// silent failures, and what they would surface as is an empty panel on somebody's desk.
///
/// A terminal cannot work that way. `cd /volume1` typed once has to still be in effect for the next
/// line, and a login paid for on every keystroke is not a terminal — so
/// [`NasBackend::shell_open`] keeps one, and pays the price the look refuses: it has to be watched for
/// the day it dies, and [`NasShellStatus::Lost`][crate::types::NasShellStatus::Lost] is where a session
/// that has died says so out loud.
///
/// The two are separate connections rather than one shared session, and that is deliberate: a page
/// watching a machine's numbers should carry on doing it while somebody types in a terminal beside it,
/// and neither should be able to break the other. A backend that can only afford one at a time is a
/// backend whose terminal is a feature of its monitor, which is a smaller thing than this trait
/// describes.
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

    /// 把这一眼挪到 `host` 上 —— [`NasBackend::devices`] 里的一个 —— 什么也不写下来。
    ///
    /// 人点一行就是这件事：[`NasBackend::remember`] 也是同一个开关，只是它顺带把这一台记进
    /// 文件。机器已经在清单里了，点它一下不是再写一遍的理由。清单里没有 `host` 是
    /// [`HalError::InvalidArg`]，并且什么也不换：换到一台不存在的机器上，等于板子盯着一片空。
    ///
    /// [`HalError::InvalidArg`]: crate::error::HalError::InvalidArg
    fn point_at(&mut self, host: &str) -> Result<(), HalError>;

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

    /// Opens an interactive session on `host`: a shell at the other end, behind a pseudo-terminal,
    /// that [`NasBackend::shell_write`] types into and [`NasBackend::shell_take`] reads back.
    ///
    /// `columns` and `rows` are the *screen's* size in characters, and they are the caller's to state
    /// because the caller is what knows: a pseudo-terminal is how a program at the other end asks
    /// whether it is talking to a person and how wide the answers should be, so a terminal that got
    /// this wrong would be sent lines that do not fit the panel drawing them. A backend that cannot
    /// honour a size should pass it on as far as it reaches rather than guessing.
    ///
    /// Like [`NasBackend::refresh`], this only *asks*: a login is a key exchange and a shell's first
    /// prompt, and a frame cannot wait for either. What the session is doing is
    /// [`NasShell::status`][crate::types::NasShell::status], read with [`NasBackend::shell_take`] —
    /// [`NasShellStatus::Opening`][crate::types::NasShellStatus::Opening] until it is up, and
    /// [`NasShellStatus::Ready`][crate::types::NasShellStatus::Ready] once it is.
    ///
    /// Opening a session while one is open replaces it, on that host or on another: there is one
    /// terminal and therefore one session, and a second [`NasBackend::shell_open`] is somebody pointing
    /// the terminal at a different machine rather than asking for two.
    ///
    /// Returns the platform's error only for the things it can tell at once — a host the board has
    /// never been told about is [`HalError::InvalidArg`]. Whether the machine *answers* is not one of
    /// them: that arrives as a status, like everything else here.
    fn shell_open(&mut self, _host: &str, _columns: u16, _rows: u16) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    /// Types `bytes` into the open session.
    ///
    /// Bytes and not a line, because that is what a keyboard makes: the `\r` that ends a command is one
    /// of them, the `\x03` that interrupts one is another, and the three bytes an arrow key is made of
    /// are three more. A call that could only take whole lines could send none of those, and a terminal
    /// that cannot interrupt a command that will not stop is not one anybody can use.
    ///
    /// Nothing is echoed here. A pseudo-terminal echoes what is typed at *its* end, so what comes back
    /// is ordinary output and arrives through [`NasBackend::shell_take`] like everything else — which is
    /// what keeps a session's echo and a session's output in one order rather than two.
    ///
    /// Never blocks: the bytes are queued for the backend's own thread to put on the wire, and the
    /// whole reason the queue exists is that the thread is in the middle of a read.
    fn shell_write(&mut self, _bytes: &[u8]) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    /// Everything the session has said since the last call, and where it stands.
    ///
    /// Cheap, snapshot-like, and safe to call every frame. It *takes*, and the name says so: what comes
    /// back is what has arrived since the last call, so two calls in one frame give the bytes to one of
    /// them and an empty answer to the other, rather than the same bytes twice.
    ///
    /// A backend with no session answers [`NasShellStatus::Closed`][crate::types::NasShellStatus::Closed]
    /// and nothing to read, which is what a terminal that has not asked for one should be told.
    fn shell_take(&mut self) -> NasShell {
        NasShell::default()
    }

    /// Hangs the session up, if there is one. Nothing open is `Ok(())`.
    ///
    /// The state after this is [`NasShellStatus::Closed`][crate::types::NasShellStatus::Closed] and not
    /// [`NasShellStatus::Lost`][crate::types::NasShellStatus::Lost]: a session somebody ended is not a
    /// session that went wrong, and a terminal that drew the two the same way would leave its own clean
    /// exit looking like a fault.
    fn shell_close(&mut self) -> Result<(), HalError> {
        Ok(())
    }
}
