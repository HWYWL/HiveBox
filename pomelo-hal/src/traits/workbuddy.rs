//! The board as a watcher of one WorkBuddy account.
//!
//! The fourth of the network traits, and the smallest: beside [`NasBackend`][crate::traits::NasBackend]
//! (a machine somebody owns) and [`CloudBackend`][crate::traits::CloudBackend] (a provider's
//! account) this one knows *one* token and asks *two* questions — what today's check-in looks like,
//! and to do it. What a page draws from it is a status light and a handful of numbers, which is
//! exactly what the account's own panel shows.
//!
//! # Start, then poll
//!
//! [`WorkBuddyBackend::start`] returns at once; what came of it is
//! [`WorkBuddyBackend::state`], and the answer is [`WorkBuddyBackend::take_answer`] on the frame the
//! state stops saying [`WorkBuddyState::Running`][crate::types::WorkBuddyState::Running]. A round
//! trip is a TLS handshake and a POST — a second or two of somebody else's time — and a page that
//! waited for one would be a page that stopped drawing.
//!
//! One request at a time; a second gets [`HalError::Busy`].
//!
//! # The token
//!
//! The token is two kilobytes of JWT and nobody types it on a touch keyboard, so it arrives through
//! the management page (`/api/workbuddy/token`) or a file on the card. [`WorkBuddyBackend::account`]
//! hands back a mask and a host, never the token: nothing a page draws needs it, and a page that
//! could draw it would be a page that leaks it into a screenshot.
//!
//! # What this is not
//!
//! It is not a client. Nothing here sends a prompt or spends a credit: the board watches an account
//! that is used somewhere else, which is the only sensible shape for a monitor that has no keyboard
//! and no screen worth typing on.

use crate::error::HalError;
use crate::types::{WorkBuddyAccount, WorkBuddyAction, WorkBuddyAnswer, WorkBuddyState};

/// Watching one WorkBuddy account: is today checked in, and how long does the token last.
pub trait WorkBuddyBackend: Send + Sync {
    /// The token on file, as much of it as a page gets to see.
    ///
    /// Cheap, and safe every frame. An empty
    /// [`masked`][crate::types::WorkBuddyAccount::masked] means nothing is saved, which is a screen
    /// ("paste one on the management page") rather than a failure — see
    /// [`WorkBuddyAccount::configured`][crate::types::WorkBuddyAccount::configured].
    fn account(&self) -> WorkBuddyAccount;

    /// Saves a token, and the token that renews it, and the host they belong to.
    ///
    /// `refresh` is optional: a token without one can still answer both questions until it expires.
    /// `host` is optional and defaults to the one the desktop client uses.
    ///
    /// [`HalError::InvalidArg`] for a token that is not a token — printable ASCII, no quotes, no
    /// spaces — and [`HalError::Busy`] while a request is in flight, because the worker reads the
    /// token in place.
    fn remember(
        &mut self,
        token: &str,
        refresh: Option<&str>,
        host: Option<&str>,
    ) -> Result<(), HalError>;

    /// Forgets the token. Nothing saved is `Ok(())`.
    fn forget(&mut self) -> Result<(), HalError>;

    /// Asks `action`, and returns at once.
    ///
    /// [`HalError::Busy`] while another request is in flight. A request that cannot be *sent* — no
    /// token, no network — is not an error here: it is a request that was asked for, and its reason
    /// arrives through [`WorkBuddyBackend::take_answer`] like any other.
    fn start(&mut self, action: WorkBuddyAction) -> Result<(), HalError>;

    /// How the request is going. Cheap, and safe every frame.
    fn state(&self) -> WorkBuddyState;

    /// Takes the answer of a finished request and clears it.
    ///
    /// [`HalError::Busy`] when there is nothing to take. The one place a failure becomes words: a
    /// request that failed hands over a
    /// [`WorkBuddyAnswer::Refused`][crate::types::WorkBuddyAnswer::Refused] with the reason in it.
    fn take_answer(&mut self) -> Result<WorkBuddyAnswer, HalError>;

    /// Abandons the request in flight. Nothing to abandon is `Ok(())`.
    fn cancel(&mut self) -> Result<(), HalError>;

    /// Advances a simulated request. Native, event-driven backends can ignore this.
    fn tick(&mut self) {}
}
