//! The answer document the C side writes for a WorkBuddy request, read back.
//!
//! `board_workbuddy.c` renders ten short lines and no JSON:
//!
//! ```text
//! ok=1
//! checked_in=1
//! streak=3
//! daily=10
//! activity_total=42
//! activity=签到有礼
//! access_days=27
//! refresh_days=29
//! credit_remaining=2425.4
//! credit_total=3000.0
//! usage_kinds=4
//! usage_calls=87
//! packages=3
//! expire=10-22
//! expire_credit=2200.0
//! today_requests=40
//! today_credit=97.4
//! today_model=deepseek-v4.1-flash
//! ```
//!
//! and a failure as `error=workbuddy_auth`, with `message=` when the account wrote a sentence.
//! Line-based rather than JSON because both ends are ours: the writer never has to escape an
//! activity's name, and the reader is [`str::split_once`] ten times instead of a parser.
//!
//! `credit_remaining` and `credit_total` come from a second call (`get-user-resource-summary`) and
//! `usage_kinds`/`usage_calls` from a third (`get-user-resource`); when either call does not answer
//! its fields are *empty* rather than zero. An empty field is a dash on the panel, a zero would be a
//! number somebody believes — see
//! [`WorkBuddyReport::credit_remaining`][crate::types::WorkBuddyReport::credit_remaining].
//!
//! The one asymmetry is `access_days`/`refresh_days`: `-1` means *unknown* — a token that is not a
//! JWT, or a clock that has never been set — and not "expired", which is `0`.

use crate::types::{WorkBuddyAnswer, WorkBuddyFault, WorkBuddyReport};

/// Reads the document `board_workbuddy.c` renders.
///
/// Total: an empty or unreadable document is a report with nothing in it rather than an error. What
/// a page gets to draw then is a panel with dashes, which is the truth about an answer with no
/// fields in it, and better than a page that disappears.
pub fn parse(document: &str) -> WorkBuddyAnswer {
    let mut report = WorkBuddyReport::default();
    let mut error: Option<WorkBuddyFault> = None;
    let mut message = String::new();

    for line in document.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "error" => error = Some(WorkBuddyFault::from_code(value)),
            "message" => {
                message.clear();
                message.push_str(value);
            }
            "checked_in" => report.checked_in_today = value == "1",
            "streak" => report.streak_days = number(value).unwrap_or(0),
            "daily" => report.daily_credit = number(value).unwrap_or(0),
            "activity_total" => report.activity_credits = number(value).unwrap_or(0),
            "activity" => {
                report.activity.clear();
                report.activity.push_str(value);
            }
            "access_days" => report.access_days_left = days(value),
            "refresh_days" => report.refresh_days_left = days(value),
            "credit_remaining" => report.credit_remaining = decimal(value),
            "credit_total" => report.credit_total = decimal(value),
            "usage_kinds" => report.usage_kinds = number(value),
            "packages" => report.live_packages = number(value),
            "expire" => {
                report.soonest_expiry.clear();
                report.soonest_expiry.push_str(value.trim());
            }
            "expire_credit" => report.soonest_credit = decimal(value),
            "today_requests" => report.today_requests = number(value),
            "today_credit" => report.today_credit = decimal(value),
            "today_model" => {
                report.today_model.clear();
                report.today_model.push_str(value.trim());
            }
            "usage_calls" => report.usage_calls = number(value),
            "credited" => report.credited = number(value),
            "already" => report.already_checked_in = value == "1",
            _ => {}
        }
    }

    match error {
        Some(fault) => WorkBuddyAnswer::Refused { fault, message },
        None => WorkBuddyAnswer::Report(report),
    }
}

/// A count, or `None` when the field is empty or is not one.
fn number(value: &str) -> Option<u32> {
    let value = value.trim();

    if value.is_empty() {
        return None;
    }

    value.parse().ok()
}

/// A credit, which is a decimal on this API — `2425.4` and not `2425` — or `None` when the field is
/// empty or is not a number.
fn decimal(value: &str) -> Option<f32> {
    let value = value.trim();

    if value.is_empty() {
        return None;
    }

    value.parse().ok()
}

/// Days left, where `-1` is *unknown* and not *expired*.
fn days(value: &str) -> Option<i64> {
    let value = value.trim();

    if value.is_empty() {
        return None;
    }

    match value.parse::<i64>() {
        Ok(days) if days >= 0 => Some(days),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::types::{WorkBuddyAnswer, WorkBuddyFault};

    /// The document the C side writes on a good day, field for field.
    const ANSWER: &str = "ok=1\n\
                          checked_in=1\n\
                          streak=3\n\
                          daily=10\n\
                          activity_total=42\n\
                          activity=签到有礼\n\
                          access_days=27\n\
                          refresh_days=29\n\
                          credit_remaining=\n\
                          credit_total=\n";

    #[test]
    fn a_good_answer_carries_the_activity_the_streak_and_the_lifetimes() {
        let WorkBuddyAnswer::Report(report) = parse(ANSWER) else {
            panic!("a document with no error in it is a report");
        };

        assert!(report.checked_in_today);
        assert_eq!(report.streak_days, 3);
        assert_eq!(report.daily_credit, 10);
        assert_eq!(report.activity_credits, 42);
        assert_eq!(report.activity, "签到有礼");
        assert_eq!(report.access_days_left, Some(27));
        assert_eq!(report.refresh_days_left, Some(29));
    }

    #[test]
    fn a_balance_keeps_its_tenths_and_a_missing_one_is_a_dash() {
        let WorkBuddyAnswer::Report(report) =
            parse("credit_remaining=2425.4\ncredit_total=3000.0\nusage_kinds=4\nusage_calls=87\n")
        else {
            panic!("no error in this document");
        };

        assert_eq!(report.credit_remaining, Some(2425.4));
        assert_eq!(report.credit_total, Some(3000.0));
        assert_eq!(report.usage_kinds, Some(4));
        assert_eq!(report.usage_calls, Some(87));
    }

    #[test]
    fn an_empty_balance_is_unknown_and_not_zero() {
        let WorkBuddyAnswer::Report(report) = parse(ANSWER) else {
            panic!("a document with no error in it is a report");
        };

        assert_eq!(
            report.credit_remaining, None,
            "a call that did not answer is a dash, and a zero would be a number somebody believes"
        );
        assert_eq!(report.credit_total, None);
    }

    #[test]
    fn a_refusal_keeps_the_accounts_own_sentence() {
        let answer = parse("error=workbuddy_api\nmessage=积分不足\n");

        assert_eq!(
            answer,
            WorkBuddyAnswer::Refused {
                fault: WorkBuddyFault::Api,
                message: String::from("积分不足"),
            }
        );
    }

    #[test]
    fn an_expired_token_is_a_fault_without_a_sentence() {
        assert_eq!(
            parse("error=workbuddy_auth\n"),
            WorkBuddyAnswer::Refused {
                fault: WorkBuddyFault::Auth,
                message: String::new(),
            }
        );
    }

    #[test]
    fn an_unknown_word_is_a_failure_rather_than_a_panic() {
        assert_eq!(
            parse("error=workbuddy_something_new\n"),
            WorkBuddyAnswer::Refused {
                fault: WorkBuddyFault::Failed,
                message: String::new(),
            }
        );
    }

    #[test]
    fn no_document_is_a_report_with_nothing_in_it() {
        let WorkBuddyAnswer::Report(report) = parse("") else {
            panic!("an empty document is not a refusal");
        };

        assert!(!report.checked_in_today);
        assert_eq!(report.access_days_left, None);
    }

    #[test]
    fn an_unknown_tokens_clock_is_a_dash_and_an_expired_one_is_a_zero() {
        let WorkBuddyAnswer::Report(report) = parse("access_days=-1\nrefresh_days=0\n") else {
            panic!("no error in this document");
        };

        assert_eq!(
            report.access_days_left, None,
            "-1 is the board saying it could not read the token, not the token being gone"
        );
        assert_eq!(report.refresh_days_left, Some(0));
    }

    #[test]
    fn a_checkin_says_what_it_gave_and_whether_it_was_already_done() {
        let WorkBuddyAnswer::Report(report) = parse("ok=1\ncredited=10\nalready=0\n") else {
            panic!("no error in this document");
        };

        assert_eq!(report.credited, Some(10));
        assert!(!report.already_checked_in);
    }
}
