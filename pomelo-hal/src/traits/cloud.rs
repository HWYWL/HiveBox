//! The board as a client of a cloud provider's API.
//!
//! The third of the network traits, beside [`NasBackend`][crate::traits::NasBackend] (one machine
//! this box was told about) and [`HttpBackend`][crate::traits::HttpBackend] (a name in a URL). It is
//! separate from both because what is on the other end is neither: the answer is a *document about
//! somebody's account* — how many servers they have, what each is doing — and the request has to be
//! signed with a key, aimed at a region, and written in the provider's own protocol. None of that is
//! a socket, and a page must never see any of it.
//!
//! # Start, then poll
//!
//! [`CloudBackend::start`] never blocks and reads nothing: it says "ask this when you can" and
//! returns. What came of it is [`CloudBackend::state`], which is cheap and safe to call once a
//! frame, and the answer is [`CloudBackend::take_answer`], called on the frame the state says
//! [`CloudState::Done`][crate::types::CloudState::Done] or
//! [`CloudState::Failed`][crate::types::CloudState::Failed]. That is the same shape the radio's scan
//! and the HTTP client have, for the same reason: a signed round trip is a second or two of
//! somebody else's time, and a page that waited for one would be a page that stopped drawing.
//!
//! One request at a time, and [`HalError::Busy`] is what a second one gets while the first is in
//! flight. A power command and a list are the same question to this trait — see
//! [`CloudAction`][crate::types::CloudAction] — because they share the one answer slot.
//!
//! # Two products, one list
//!
//! A key is saved per product underneath, and a merged list asks both: what one product's API can
//! tell this board says nothing about the other's. [`CloudBackend::key`] is how a page finds out
//! whether anything is set up, and the page that sets it up is the management page, not this one.

use crate::error::HalError;
use crate::types::{CloudAction, CloudAnswer, CloudKey, CloudProvider, CloudState};

/// Managing servers in a cloud account: what there is, and turning it on and off.
pub trait CloudBackend: Send + Sync {
    /// The key saved for `provider`, as much of it as comes back out.
    ///
    /// Cheap and safe to call every frame, and the only thing a page knows about the credentials:
    /// an empty [`CloudKey`] means nothing is saved for that product, which is a screen rather than
    /// a failure. See [`CloudKey::configured`][crate::types::CloudKey::configured].
    fn key(&self, provider: CloudProvider) -> CloudKey;

    /// Asks `provider` to do `action` to `instance`, and returns at once.
    ///
    /// `instance` is the provider's own id — `ins-…` or `lhins-…` — and is ignored by
    /// [`CloudAction::List`][crate::types::CloudAction::List], which is the one action that is not
    /// about a machine.
    ///
    /// `region` is the data centre the machine is in, and is what
    /// [`CloudInstance::region`][crate::types::CloudInstance::region] carries on a list row: the
    /// same account's servers are spread across data centres, and a power command is addressed to
    /// one. Empty is "whatever the key was saved with", which is all a backend that has no row can
    /// know.
    ///
    /// [`HalError::Busy`] while a request is already in flight, and [`HalError::InvalidArg`] for an
    /// action with no instance to aim at. Neither is a fault of the *provider* — what the last
    /// request came to is [`CloudBackend::state`].
    fn start(
        &mut self,
        provider: CloudProvider,
        action: CloudAction,
        instance: &str,
        region: &str,
    ) -> Result<(), HalError>;

    /// How the request is going, as a snapshot.
    ///
    /// Cheap, and safe to call every frame.
    /// [`CloudState::Idle`][crate::types::CloudState::Idle] before anything has been asked for and
    /// again once an answer has been taken.
    fn state(&self) -> CloudState;

    /// Takes the answer of a request that finished, and clears it.
    ///
    /// Called on the frame [`CloudBackend::state`] stops saying
    /// [`CloudState::Running`][crate::types::CloudState::Running], and not on any other. The one
    /// place a failure is turned into words: a request that failed hands over a
    /// [`CloudAnswer::Refused`][crate::types::CloudAnswer::Refused] with the reason in it, so a page
    /// that reads this and nothing else has everything it needs to draw what happened.
    ///
    /// [`HalError::Busy`] when there is nothing to take — the request is still running, or its
    /// answer has already been read. Taking it leaves the backend
    /// [`CloudState::Idle`][crate::types::CloudState::Idle] as if nothing had ever been asked for.
    fn take_answer(&mut self) -> Result<CloudAnswer, HalError>;

    /// Abandons the request in flight. Nothing to abandon is `Ok(())`.
    ///
    /// What a page does when it goes away mid-request: the answer is on somebody else's network, and
    /// the box has no reason to keep a slot for a question nobody is waiting for.
    fn cancel(&mut self) -> Result<(), HalError>;

    /// Advances a simulated request's state. Native, event-driven backends can ignore this.
    ///
    /// The same knob the radio and the HTTP client have, and for the same reason: the device's
    /// client is woken by the network, and a desktop's has no network to be woken by — see
    /// `sim::cloud`.
    fn tick(&mut self) {}
}
