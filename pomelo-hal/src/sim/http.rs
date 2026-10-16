//! Simulated HTTP client: a request that takes a few ticks and answers with what it was handed.
//!
//! There is no socket here and deliberately so. The point of the simulator is that the test suite is
//! off the network and so is a demo: a page whose desktop build fetched from a real weather service
//! would be a page that failed to build a screenshot on a train, and whose tests passed or failed
//! depending on whether the service was up. What the device's client has to get right is the
//! *trait* — start, poll, collect — and that is exactly what this one gets right, with the answer
//! supplied by whoever built it instead of by the internet.
//!
//! A [`Sink::File`] request is the one place this reaches past its own buffers: the answer is written
//! to the path with `std::fs`, because a desktop *has* a filesystem and the job is then the same job
//! rather than an impression of it. A path that will not open fails as [`HttpFault::Storage`], the way
//! a card that is not in the slot does.

use crate::error::HalError;
use crate::traits::HttpBackend;
use crate::types::{HttpFault, HttpResponse, HttpState, Sink};

/// How long a simulated request takes to come back, in ticks.
///
/// Three, the same as a simulated radio scan, and for the same reason: long enough that a page which
/// never handled "still running" is visibly broken on a desktop rather than only on the device.
const ANSWER_TICKS: u8 = 3;

/// A desktop HTTP client.
pub struct SimHttp {
    state: HttpState,
    answer_ticks_left: u8,
    /// The fault every request ends in, or `None` for one that answers normally.
    fault: Option<HttpFault>,
    /// The answers this client gives, one per request, and which of them the next request gets.
    ///
    /// A script rather than a single body because a page can ask two services in turn — the weather page
    /// asks a locator where it is and then asks the forecast service about there — and one canned body
    /// cannot say two different things. It *repeats*, which is what makes a page's second refresh
    /// answerable by the same script as its first, and a script of one answer behaves exactly as the
    /// canned body every other page on this board wants it to.
    answers: Vec<Answer>,
    next: usize,
    /// The answer waiting to be collected, in the one state that has one.
    result: Option<HttpResponse>,
    /// The last URL asked for, kept so that a test can check the request a page *built* — the query
    /// string, the days of forecast — and not only what it did with the answer.
    last_url: Option<String>,
    /// The last sink a request was given, for the same reason as [`SimHttp::last_url`]: a page that
    /// downloads a glyph table is a page whose *choice of file* is half of what it has to get right,
    /// and a test that could only see the body would pass on a page that wrote to the wrong path.
    last_sink: Option<Sink>,
}

/// One answer in a [`SimHttp`] script: what a request is answered with, status and body together.
///
/// Built from what a caller has to hand rather than named at the call site: a `&str` or `String` is a
/// body answered `200`, and a `(status, body)` pair is the answers that are not successes —
/// [`SimHttp::then`] takes either.
#[derive(Clone)]
pub struct Answer {
    status: u16,
    body: String,
}

impl From<&str> for Answer {
    /// A body, answered `200`.
    fn from(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_owned(),
        }
    }
}

impl From<String> for Answer {
    /// A body, answered `200`.
    fn from(body: String) -> Self {
        Self { status: 200, body }
    }
}

impl<T: Into<String>> From<(u16, T)> for Answer {
    /// A status and a body together, for the answers that are not successes.
    fn from((status, body): (u16, T)) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

impl SimHttp {
    /// A client that answers `200` with an empty body.
    pub fn new() -> Self {
        Self {
            state: HttpState::Idle,
            answer_ticks_left: 0,
            fault: None,
            answers: vec![Answer {
                status: 200,
                body: String::new(),
            }],
            next: 0,
            result: None,
            last_url: None,
            last_sink: None,
        }
    }

    /// A client that answers `200` with `body`.
    ///
    /// This is the desktop demo's seam into a page that fetches: the page's `main.rs` hands over a
    /// fixture — the document *that* service would have sent — and the page above this trait cannot
    /// tell the difference between the simulator and the network. See
    /// [`Board::with_http`][crate::Board::with_http].
    pub fn answering(body: impl Into<String>) -> Self {
        Self {
            answers: vec![Answer {
                status: 200,
                body: body.into(),
            }],
            ..Self::new()
        }
    }

    /// The same client, with one more answer after the one it already has.
    ///
    /// The answer is a body, which is answered `200`, or a `(status, body)` pair for the scripts whose
    /// answers differ in more than their body — a locator that answers and a server that refuses. Which
    /// request gets which answer is the order they were given in.
    pub fn then(mut self, answer: impl Into<Answer>) -> Self {
        self.answers.push(answer.into());

        self
    }

    /// A client that never answers, and says why.
    ///
    /// The faults that are about the board rather than the server — [`HttpFault::NoNetwork`],
    /// [`HttpFault::ClockNotSet`] — are the interesting ones here: a page has to draw them, and they
    /// are what a demo with the Wi-Fi off should look like.
    pub fn failing(fault: HttpFault) -> Self {
        Self {
            fault: Some(fault),
            ..Self::new()
        }
    }

    /// A client with nowhere to reach: the board is off the network.
    pub fn offline() -> Self {
        Self::failing(HttpFault::NoNetwork)
    }

    /// The same client, but its answers carry this status code.
    ///
    /// For the answers that are not successes: a `404` page, a `500` — a body that arrived with a
    /// status saying it means something else, which is a case a page has to get right and cannot
    /// rehearse against a real server.
    pub fn with_status(mut self, status: u16) -> Self {
        for answer in &mut self.answers {
            answer.status = status;
        }

        self
    }

    /// The last URL asked for.
    pub fn last_url(&self) -> Option<&str> {
        self.last_url.as_deref()
    }

    /// The last sink a request was asked to put its answer in.
    pub fn last_sink(&self) -> Option<&Sink> {
        self.last_sink.as_ref()
    }
}

impl Default for SimHttp {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether this is a URL the simulator will take.
///
/// The same check the device's client makes before it hands anything to the network stack: a scheme
/// it speaks and a host to speak it to. It is not a URL *parser* — nothing here needs the path split
/// off the query, and a parser is a thing with opinions about percent-encoding that a request only
/// has to pass through unchanged.
fn fetchable(url: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .filter_map(|scheme| url.strip_prefix(scheme))
        .any(|host| !host.is_empty() && !host.starts_with('/'))
}

impl HttpBackend for SimHttp {
    fn start_to(&mut self, url: &str, sink: Sink) -> Result<(), HalError> {
        if matches!(self.state, HttpState::Running | HttpState::Done) {
            // Refused rather than queued: a request in flight, or an answer nobody collected, is a
            // page that lost track of one — and quietly dropping it would hide that.
            //
            // `Failed` is not in that list, and it is the case that matters: a request that did not
            // come back is not in flight, so a page showing a fault has to be able to try again. The
            // device's client clears its own busy flag the same way when a request ends badly (see
            // `board_http.c`'s `finish`), so this is the simulator catching up rather than an
            // exception made for it.
            return Err(HalError::Busy);
        }
        if !fetchable(url) {
            return Err(HalError::InvalidArg);
        }
        if matches!(&sink, Sink::File(path) if path.is_empty()) {
            // A file sink with no file in it. Refused here rather than turned into a fault later,
            // because it is a page that built a path wrong and not a card that is not there — the
            // same line the device's client draws.
            return Err(HalError::InvalidArg);
        }

        self.last_url = Some(url.to_string());
        self.last_sink = Some(sink);
        self.answer_ticks_left = ANSWER_TICKS;
        self.state = HttpState::Running;
        Ok(())
    }

    fn state(&self) -> HttpState {
        self.state
    }

    fn take_result(&mut self) -> Result<HttpResponse, HalError> {
        let response = self.result.take().ok_or(HalError::Busy)?;
        self.state = HttpState::Idle;
        Ok(response)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        self.state = HttpState::Idle;
        self.answer_ticks_left = 0;
        self.result = None;
        Ok(())
    }

    fn tick(&mut self) {
        if self.state != HttpState::Running {
            return;
        }

        self.answer_ticks_left = self.answer_ticks_left.saturating_sub(1);
        if self.answer_ticks_left > 0 {
            return;
        }

        self.state = match self.fault {
            Some(fault) => HttpState::Failed(fault),
            None => self.settle(),
        };
    }
}

impl SimHttp {
    /// Hands over the script's next answer: into the buffer, or into the file a sink named.
    fn settle(&mut self) -> HttpState {
        // One answer per request, taken from the script and then stepped over: the answer is cloned out
        // before the index moves so that the body outlives the borrow of the script it came from.
        let answer = self.answers[self.next % self.answers.len()].clone();
        self.next += 1;
        let body = answer.body.into_bytes();

        let Some(path) = self.file_sink() else {
            self.result = Some(HttpResponse {
                status: answer.status,
                body,
            });
            return HttpState::Done;
        };

        // The answer lands in the file and not in the buffer, exactly as the device's client leaves it:
        // a page that downloaded a glyph table has the file and no copy in memory, which is the point of
        // asking for one. A write that fails is the card's fault and not the network's.
        match std::fs::write(&path, &body) {
            Ok(()) => {
                self.result = Some(HttpResponse {
                    status: answer.status,
                    body: Vec::new(),
                });
                HttpState::Done
            }
            Err(_) => {
                self.result = None;
                HttpState::Failed(HttpFault::Storage)
            }
        }
    }

    /// The path a file sink asked for, if the last request was given one.
    ///
    /// Cloned out rather than borrowed so that the caller is free to write to `self` afterwards.
    fn file_sink(&self) -> Option<String> {
        match &self.last_sink {
            Some(Sink::File(path)) => Some(path.clone()),
            _ => None,
        }
    }
}
