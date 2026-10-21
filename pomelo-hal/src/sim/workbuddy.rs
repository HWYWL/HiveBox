//! The simulated WorkBuddy account: a token, a streak, and a round trip that takes three ticks.
//!
//! What it exists for is the panel: a page about somebody's check-in cannot be built on a desktop
//! without an account, and the three screens that matter — signed in already, not signed in yet, and
//! no token at all — are the ones a page gets wrong when nothing can be shown.
//!
//! The *timing* is the other half, and it is the half that is easy to forget: a request that
//! answered in the frame it was asked for would let a page be written that never draws a spinner,
//! and the spinner is most of what a person sees on the device.
//!
//! The balance is deliberately unknown here as well as on the board: see
//! [`WorkBuddyReport::credit_remaining`][crate::types::WorkBuddyReport::credit_remaining].

use crate::error::HalError;
use crate::traits::WorkBuddyBackend;
use crate::types::{
    WorkBuddyAccount, WorkBuddyAction, WorkBuddyAnswer, WorkBuddyFault, WorkBuddyReport,
    WorkBuddyState,
};

/// How many ticks an invented request takes before its answer is ready.
///
/// Three, the same as [`sim::cloud`][crate::sim::cloud]'s, and for the same reason: a page polls
/// once a frame, so this is three frames of spinner on a desktop and about the time a TLS handshake
/// and a POST take on the board.
const LATENCY: u32 = 3;

/// A WorkBuddy account that is not there.
pub struct SimWorkBuddy {
    /// What a page is allowed to see of the token.
    account: WorkBuddyAccount,
    /// What a status request would answer.
    report: WorkBuddyReport,
    /// The request in flight, if there is one.
    in_flight: Option<Request>,
    /// The answer waiting to be taken, if there is one.
    answer: Option<WorkBuddyAnswer>,
    /// How the last request went, which is what a page polls.
    state: WorkBuddyState,
    /// A fault every request ends in, when a demo asked for one.
    fault: Option<WorkBuddyFault>,
}

/// The request in flight: what was asked, and how long is left of the round trip.
struct Request {
    action: WorkBuddyAction,
    ticks_left: u32,
}

impl Default for SimWorkBuddy {
    fn default() -> Self {
        Self::new()
    }
}

impl SimWorkBuddy {
    /// An account that has checked in today, twelve days running.
    pub fn new() -> Self {
        Self {
            account: WorkBuddyAccount {
                masked: String::from("…a1b2"),
                host: String::from("www.workbuddy.cn"),
            },
            report: WorkBuddyReport {
                credit_remaining: Some(2425.4),
                credit_total: Some(3000.0),
                usage_kinds: Some(4),
                usage_calls: Some(87),
                live_packages: Some(3),
                soonest_expiry: String::from("10-22"),
                soonest_credit: Some(2200.0),
                today_requests: Some(40),
                today_credit: Some(97.4),
                today_model: String::from("deepseek-v4.1-flash"),
                checked_in_today: true,
                streak_days: 12,
                daily_credit: 10,
                activity_credits: 120,
                activity: String::from("签到有礼"),
                access_days_left: Some(27),
                refresh_days_left: Some(29),
                credited: None,
                already_checked_in: false,
            },
            in_flight: None,
            answer: None,
            state: WorkBuddyState::Idle,
            fault: None,
        }
    }

    /// The same account before today's check-in: the one screen the happy path never reaches.
    pub fn not_checked_in(mut self) -> Self {
        self.report.checked_in_today = false;
        self.report.streak_days = 11;
        self
    }

    /// The same board with no token saved.
    ///
    /// Not a failure: it is where a board is before anybody pastes one in, and what the page has to
    /// say is where it goes.
    pub fn without_token(mut self) -> Self {
        self.account = WorkBuddyAccount::default();
        self
    }

    /// The same account with a token the board cannot read a lifetime out of.
    pub fn without_lifetime(mut self) -> Self {
        self.report.access_days_left = None;
        self.report.refresh_days_left = None;
        self
    }

    /// The same account on a bad day: every request ends in `fault`.
    pub fn failing(mut self, fault: WorkBuddyFault) -> Self {
        self.fault = Some(fault);
        self
    }

    /// The report a status request would answer, for a demo that wants to draw one without asking.
    pub fn report(&self) -> &WorkBuddyReport {
        &self.report
    }

    /// Turns a token into the mask a page sees: the last four characters and nothing else.
    fn mask(token: &str) -> String {
        let keep = token.char_indices().rev().take(4).last().map_or(token.len(), |it| it.0);

        format!("…{}", &token[keep..])
    }
}

impl WorkBuddyBackend for SimWorkBuddy {
    fn account(&self) -> WorkBuddyAccount {
        self.account.clone()
    }

    fn remember(
        &mut self,
        token: &str,
        _refresh: Option<&str>,
        host: Option<&str>,
    ) -> Result<(), HalError> {
        if token.is_empty() || token.contains(['"', '\\', ' ']) {
            return Err(HalError::InvalidArg);
        }

        if self.in_flight.is_some() {
            return Err(HalError::Busy);
        }

        self.account = WorkBuddyAccount {
            masked: Self::mask(token),
            host: host
                .filter(|host| !host.is_empty())
                .unwrap_or("www.workbuddy.cn")
                .to_string(),
        };

        Ok(())
    }

    fn forget(&mut self) -> Result<(), HalError> {
        if self.in_flight.is_some() {
            return Err(HalError::Busy);
        }

        self.account = WorkBuddyAccount::default();

        Ok(())
    }

    fn start(&mut self, action: WorkBuddyAction) -> Result<(), HalError> {
        if self.in_flight.is_some() || self.answer.is_some() {
            return Err(HalError::Busy);
        }

        /* Answered at once rather than after a round trip: nothing left the board, and a page that
         * waited three frames to be told it has no token would be a page with a spinner in front of
         * a sentence it could have drawn immediately. */
        if !self.account.configured() {
            self.answer = Some(WorkBuddyAnswer::Refused {
                fault: WorkBuddyFault::NoToken,
                message: String::new(),
            });
            self.state = WorkBuddyState::Failed(WorkBuddyFault::NoToken);

            return Ok(());
        }

        if let Some(fault) = self.fault {
            self.answer = Some(WorkBuddyAnswer::Refused {
                fault,
                message: String::new(),
            });
            self.state = WorkBuddyState::Failed(fault);

            return Ok(());
        }

        self.in_flight = Some(Request {
            action,
            ticks_left: LATENCY,
        });
        self.state = WorkBuddyState::Running;

        Ok(())
    }

    fn state(&self) -> WorkBuddyState {
        self.state
    }

    fn take_answer(&mut self) -> Result<WorkBuddyAnswer, HalError> {
        let answer = self.answer.take().ok_or(HalError::Busy)?;

        self.state = WorkBuddyState::Idle;

        Ok(answer)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        self.in_flight = None;
        self.answer = None;
        self.state = WorkBuddyState::Idle;

        Ok(())
    }

    fn tick(&mut self) {
        let Some(request) = self.in_flight.as_mut() else {
            return;
        };

        request.ticks_left = request.ticks_left.saturating_sub(1);

        if request.ticks_left > 0 {
            return;
        }

        let action = request.action;
        self.in_flight = None;

        self.answer = Some(match action {
            WorkBuddyAction::Status => WorkBuddyAnswer::Report(self.report.clone()),
            WorkBuddyAction::Checkin => {
                /* A check-in that has already happened today is not a failure: the account says
                 * so, and the page draws the same thing either way. */
                let already = self.report.checked_in_today;

                if !already {
                    self.report.checked_in_today = true;
                    self.report.streak_days += 1;
                    self.report.activity_credits += self.report.daily_credit;
                }

                let mut report = self.report.clone();
                report.credited = Some(if already { 0 } else { self.report.daily_credit });
                report.already_checked_in = already;

                WorkBuddyAnswer::Report(report)
            }
        });

        self.state = WorkBuddyState::Done;
    }
}

#[cfg(test)]
mod tests {
    use super::SimWorkBuddy;
    use crate::traits::WorkBuddyBackend;
    use crate::types::{WorkBuddyAction, WorkBuddyAnswer, WorkBuddyFault, WorkBuddyState};

    /// Asks, and turns the ticks until the answer is there.
    fn settle(workbuddy: &mut SimWorkBuddy, action: WorkBuddyAction) -> WorkBuddyAnswer {
        workbuddy.start(action).unwrap();

        for _ in 0..super::LATENCY {
            workbuddy.tick();
        }

        workbuddy.take_answer().unwrap()
    }

    #[test]
    fn a_status_round_trip_takes_the_latency_and_then_answers() {
        let mut workbuddy = SimWorkBuddy::new();

        workbuddy.start(WorkBuddyAction::Status).unwrap();

        assert_eq!(workbuddy.state(), WorkBuddyState::Running);
        assert_eq!(
            workbuddy.take_answer(),
            Err(crate::error::HalError::Busy),
            "nothing to take while the round trip is out"
        );

        for _ in 0..super::LATENCY {
            workbuddy.tick();
        }

        assert_eq!(workbuddy.state(), WorkBuddyState::Done);

        let WorkBuddyAnswer::Report(report) = workbuddy.take_answer().unwrap() else {
            panic!("a good day is a report");
        };

        assert!(report.checked_in_today);
        assert_eq!(report.streak_days, 12);
        assert_eq!(workbuddy.state(), WorkBuddyState::Idle);
    }

    #[test]
    fn a_board_with_no_token_says_so_at_once_rather_than_after_a_round_trip() {
        let mut workbuddy = SimWorkBuddy::new().without_token();

        workbuddy.start(WorkBuddyAction::Status).unwrap();

        assert_eq!(
            workbuddy.state(),
            WorkBuddyState::Failed(WorkBuddyFault::NoToken),
            "nothing left the board, so there is nothing to wait for"
        );

        assert_eq!(
            workbuddy.take_answer(),
            Ok(WorkBuddyAnswer::Refused {
                fault: WorkBuddyFault::NoToken,
                message: String::new(),
            })
        );
    }

    #[test]
    fn checking_in_twice_is_not_an_error_the_second_time() {
        let mut workbuddy = SimWorkBuddy::new().not_checked_in();

        let WorkBuddyAnswer::Report(first) = settle(&mut workbuddy, WorkBuddyAction::Checkin) else {
            panic!("a check-in that works is a report");
        };

        assert_eq!(first.credited, Some(10));
        assert!(!first.already_checked_in);
        assert!(first.checked_in_today);

        let WorkBuddyAnswer::Report(second) = settle(&mut workbuddy, WorkBuddyAction::Checkin) else {
            panic!("a check-in that has already happened is still a report");
        };

        assert!(second.already_checked_in);
        assert_eq!(second.credited, Some(0));
        assert_eq!(
            second.streak_days, 12,
            "the second one did not count again"
        );
    }

    #[test]
    fn a_token_with_no_readable_lifetime_draws_a_dash() {
        let mut workbuddy = SimWorkBuddy::new().without_lifetime();

        let WorkBuddyAnswer::Report(report) = settle(&mut workbuddy, WorkBuddyAction::Status) else {
            panic!("a good day is a report");
        };

        assert_eq!(report.access_days_left, None);
    }

    #[test]
    fn a_saved_token_is_shown_as_its_last_four_characters_and_no_more() {
        let mut workbuddy = SimWorkBuddy::new().without_token();

        workbuddy
            .remember("eyJhbGciOiJIUzI1NiJ9.payload.signature9x8y", None, Some("www.example.cn"))
            .unwrap();

        let account = workbuddy.account();

        assert!(account.configured());
        assert_eq!(account.masked, "…9x8y");
        assert_eq!(account.host, "www.example.cn");
    }

    #[test]
    fn a_second_request_while_one_is_out_is_busy() {
        let mut workbuddy = SimWorkBuddy::new();

        workbuddy.start(WorkBuddyAction::Status).unwrap();

        assert_eq!(workbuddy.start(WorkBuddyAction::Checkin), Err(crate::error::HalError::Busy));
    }
}
