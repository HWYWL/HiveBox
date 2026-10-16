//! The board as a *client* of somebody else's HTTP service.
//!
//! The boards' other network trait, [`NasBackend`][crate::traits::NasBackend], is about one machine
//! this box was told about; this one is about a name in a URL. It is here rather than inside the app
//! that needed it because the thing an app cannot do for itself is the *transport*: a request is a
//! blocking call into a socket and a TLS handshake, and on this board that work has to happen off the
//! frame loop, on a task with a stack of its own (`260929-03`). An app above this trait asks for a
//! document and reads an answer; it never sees a socket.
//!
//! # Start, then poll
//!
//! [`HttpBackend::start`] never blocks and reads nothing: it says "fetch this when you can" and
//! returns. What came of it is [`HttpBackend::state`], which is cheap and safe to call once a frame,
//! and the body is [`HttpBackend::take_result`], which is called on the frame the state says
//! [`HttpState::Done`][crate::types::HttpState::Done] and not on any other. This is the same shape
//! the radio's scan has, and for the same reason — a request is a second or two of the network's
//! time, and a page that waited for one would be a page that stopped drawing.
//!
//! One request at a time: [`HttpBackend::start`] while one is in flight is
//! [`HalError::Busy`] rather than a second attempt quietly overtaking the first.
//!
//! # HTTPS needs a clock, and a board may not have one
//!
//! Every certificate in the world carries a validity window, and it is checked against the *system*
//! clock. A board whose clock has never been set is a board whose clock reads 1970, which is before
//! every certificate ever issued — so the handshake fails, and it fails in a way that looks like a
//! network fault. [`HttpFault::ClockNotSet`][crate::types::HttpFault::ClockNotSet] is that case said
//! out loud, so a page can tell the person to check the time rather than the Wi-Fi. See
//! [`SystemBackend::clock`][crate::traits::SystemBackend::clock], which is where the time comes from.

use crate::error::HalError;
use crate::types::{HttpProgress, HttpResponse, HttpState, Sink};

/// Fetching documents over HTTP and HTTPS.
pub trait HttpBackend: Send + Sync {
    /// Asks for `url`, and puts the answer in the backend's own buffer.
    ///
    /// Returns at once; the answer arrives when it arrives. This is the whole of a *document* — a
    /// forecast, a manifest, a page of JSON — and the bound that goes with it is
    /// [`HttpResponse::MAX_BODY_BYTES`][crate::types::HttpResponse::MAX_BODY_BYTES], with an answer
    /// over it reported as [`HttpFault::TooLarge`][crate::types::HttpFault::TooLarge]. Anything that
    /// is megabytes — a file, an image — goes to [`HttpBackend::start_to`] instead.
    fn start(&mut self, url: &str) -> Result<(), HalError> {
        self.start_to(url, Sink::Memory)
    }

    /// Asks for `url`, with the answer going to `sink`.
    ///
    /// The general form of [`HttpBackend::start`], which is this with
    /// [`Sink::Memory`][crate::types::Sink::Memory]. The sink is worth its own parameter because the
    /// answers are not the same kind of thing: the buffer is bounded and a file is not, and only the
    /// caller knows which it asked for.
    ///
    /// The URL *is* the request: this is a `GET`, and the schemes it accepts are `http` and `https`.
    /// A request with a body or a method of its own is not needed by anything on this board yet, and
    /// a parameter carrying both would be a parameter every implementation has to look at to find
    /// out it is empty.
    ///
    /// A [`Sink::File`][crate::types::Sink::File] or [`Sink::Ota`][crate::types::Sink::Ota] answer
    /// is *not* in the [`HttpResponse`] that [`HttpBackend::take_result`] hands over — that one's body
    /// is empty — but where the sink put it: a file whose length is the answer's length, or a slot
    /// the box is now set to start from. Remembering the sink between the two calls is the caller's
    /// job, and it is the only thing that says where to look.
    ///
    /// [`HalError::Busy`] while a request is already in flight, and [`HalError::InvalidArg`] for
    /// something that is not an URL this backend can fetch, or a sink it cannot use. Neither is a
    /// fault of the *network* — what the last request did is [`HttpBackend::state`].
    fn start_to(&mut self, url: &str, sink: Sink) -> Result<(), HalError>;

    /// How the request is going, as a snapshot.
    ///
    /// Cheap, and safe to call every frame. [`HttpState::Idle`][crate::types::HttpState::Idle]
    /// before anything has been asked for and again once an answer has been taken.
    fn state(&self) -> HttpState;

    /// Takes the answer of a request that finished, and clears it.
    ///
    /// The one-shot half of [`HttpState::Done`][crate::types::HttpState::Done]: the state goes on
    /// saying `Done` until this is called, which is what makes "is there an answer" a question with
    /// an answer until somebody reads it. Taking it leaves the backend [`HttpState::Idle`] as if
    /// nothing had ever been asked for.
    ///
    /// [`HalError::Busy`] when there is nothing to take — the request is still running, or failed, or
    /// its answer has already been read. A fault that is *not* here: a request that did not come back
    /// says so through [`HttpBackend::state`], and has no result to hand over.
    fn take_result(&mut self) -> Result<HttpResponse, HalError>;

    /// How far a request that is going somewhere has got, or `None` when there is none to read.
    ///
    /// Cheap, like [`HttpBackend::state`], and asked once a frame by the page that is waiting: a
    /// minute of downloading is a minute somebody is watching, and the only thing that says the box
    /// has not hung is a number that moves. `None` before such a request has been made, and again
    /// once its answer has been taken or cancelled — including the frames a *buffer* request is
    /// running, whose progress is real but is not a bar anybody draws.
    ///
    /// The default is `None` rather than a made-up zero, because a backend that cannot say has
    /// nothing honest to say: an implementation with no counters left over from the last request
    /// reports nothing, and a page falls back on the size its manifest gave it.
    fn progress(&self) -> Option<HttpProgress> {
        None
    }

    /// Abandons the request in flight. Nothing to abandon is `Ok(())`.
    ///
    /// The backend is [`HttpState::Idle`][crate::types::HttpState::Idle] afterwards, whether there
    /// was something to drop or not, which is the state a page leaves behind when it goes away
    /// mid-request.
    fn cancel(&mut self) -> Result<(), HalError>;

    /// Advances a simulated request's state. Native, event-driven backends can ignore this.
    ///
    /// The same knob the radio has, and for the same reason: the device's client is woken by the
    /// network, and a desktop's has no network to be woken by — see `sim::http`.
    fn tick(&mut self) {}
}
