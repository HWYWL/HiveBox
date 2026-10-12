//! Turning the clock the board keeps into the numbers a dial is drawn from.
//!
//! # Why the arithmetic is here and not in a date library
//!
//! This is a calendar, and a calendar is the one thing every language has a library for — none of
//! which are in this dependency graph, and none of which this page needs. What it needs is three
//! numbers a dial is drawn from (an hour, a minute, a second), one date line, and one weekday name; a
//! crate that brought time zones, locales and a parser for dates a person types would be a great deal
//! of code for the part of it that is `(seconds / 3600) % 24`.
//!
//! So the conversion is the standard civil-from-days algorithm, written out, and it is tested against
//! dates that are known: the epoch was a Thursday, 2024 had a 29th of February, and this project's own
//! release notes are dated.

use std::time::{SystemTime, UNIX_EPOCH};

use pomelo_widgets::preferences::Language;

/// The offset from UTC this page draws in.
///
/// Eight hours, which is not this page's decision: the launcher's status bar draws the same time the
/// same way, and it does it by adding this number to the epoch. A *setting* is the right home for it —
/// the board's RTC keeps UTC and somebody in another zone would have to change this line to move the
/// dial — and until there is one, the two clocks on this device agree with each other, which is the
/// property that would otherwise be broken by fixing only one of them.
pub const UTC_OFFSET_SECS: i64 = 8 * 3600;

/// A wall-clock reading: what the dial, the date line and the alarm list all read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Monday is 0 and Sunday is 6, which is the order calendars are printed in rather than the order
    /// `%w` prints them in.
    pub weekday: u32,
}

impl Civil {
    /// The reading a `SystemTime` is, in [`UTC_OFFSET_SECS`].
    pub fn from_system_time(time: SystemTime) -> Self {
        // A time before the epoch is not a time this board can be at — the firmware hands out `None`
        // for a clock that was never set, and that is a different answer from 1970 — but a subtraction
        // that cannot fail is worth more than one that panics on the boards that never get here.
        let seconds = time
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs() as i64)
            .unwrap_or(0);

        Self::from_unix(seconds)
    }

    /// The reading `seconds` after the epoch is, in [`UTC_OFFSET_SECS`].
    ///
    /// The offset is added to the *instant* rather than to the fields, which is what makes this a
    /// conversion and not a correction: a reading shifted by eight hours and then taken apart
    /// belongs to the day it lands on, so 23:30 UTC on a Friday is half past seven on a **Saturday**
    /// morning here, weekday and date both.
    pub fn from_unix(seconds: i64) -> Self {
        let local = seconds + UTC_OFFSET_SECS;

        // Floor division, because the epoch is not the beginning of the calendar: a negative instant
        // — which the RTC can hold while it is being set — must land on the day before it, not on the
        // day of a remainder truncated towards zero.
        let days = local.div_euclid(86_400);
        let of_day = local.rem_euclid(86_400);

        let (year, month, day) = civil_from_days(days);

        Self {
            year,
            month,
            day,
            hour: (of_day / 3600) as u32,
            minute: ((of_day / 60) % 60) as u32,
            second: (of_day % 60) as u32,
            // The epoch was a Thursday, which is 3 in a week that starts on Monday.
            weekday: (days + 3).rem_euclid(7) as u32,
        }
    }

    /// How far round the hour hand has gone, in `0.0..24.0`, counting minutes.
    pub fn hour_angle(&self) -> f32 {
        self.hour as f32 + self.minute as f32 / 60.0
    }

    /// How far round the minute hand has gone, in `0.0..60.0`, counting seconds.
    pub fn minute_angle(&self) -> f32 {
        self.minute as f32 + self.second as f32 / 60.0
    }

    /// The seconds, as a hand that sweeps rather than ticks.
    pub fn second_angle(&self) -> f32 {
        self.second as f32
    }

    /// `08:05`.
    pub fn hh_mm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// `08:05:09`.
    pub fn hh_mm_ss(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// `2026-10-12`.
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// The weekday's name, in the language the system is set to.
    pub fn weekday_name(&self, language: Language) -> &'static str {
        let english = [
            "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun",
        ];
        let chinese = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

        let names = match language {
            Language::Chinese => chinese,
            Language::English => english,
        };

        names[(self.weekday as usize).min(6)]
    }
}

/// The year, month and day a count of days from the epoch is.
///
/// Howard Hinnant's `civil_from_days`, which is the algorithm every standard library's date type is
/// built on: an era of 400 years, the day within it, and a five-month cycle within that, in integer
/// arithmetic that is exact for every date this board will ever hold.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to the 1st of March, so that the leap day is the *last* day of a year and the
    // month-length table below needs no special case for February.
    let shifted = days + 719_468;

    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);

    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;

    let year = year_of_era + era * 400;

    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;

    let day = (day_of_year - (153 * march_month + 2) / 5 + 1) as u32;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    } as u32;

    // January and February belong to the year after the one the March-based count says.
    let year = if month <= 2 { year + 1 } else { year };

    (year, month, day)
}

/// A countdown as `MM:SS`, or `H:MM:SS` once it is over an hour.
///
/// The hour appears only when there is one to show: a half-hour break drawn as `00:30:00` is three
/// leading glyphs that say nothing, and a page that draws them is a page whose big digits moved for
/// no reason.
pub fn countdown(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    let seconds = seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// A span of time in words: how long this board has been up, in the language it is set to.
pub fn duration_text(seconds: u64, language: Language) -> String {
    let days = seconds / 86_400;
    let hours = (seconds / 3600) % 24;
    let minutes = (seconds / 60) % 60;

    match language {
        Language::Chinese => {
            if days > 0 {
                format!("{days} 天 {hours} 小时")
            } else if hours > 0 {
                format!("{hours} 小时 {minutes} 分")
            } else {
                format!("{minutes} 分")
            }
        }
        Language::English => {
            if days > 0 {
                format!("{days}d {hours}h")
            } else if hours > 0 {
                format!("{hours}h {minutes}m")
            } else {
                format!("{minutes}m")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The epoch, which is the one instant every implementation of this arithmetic is checked against.
    #[test]
    fn the_epoch_is_a_thursday_morning_here() {
        // 1970-01-01T00:00:00Z is 08:00 on the 1st in an offset of +8.
        let civil = Civil::from_unix(0);

        assert_eq!(civil.year, 1970);
        assert_eq!(civil.month, 1);
        assert_eq!(civil.day, 1);
        assert_eq!(civil.hour, 8);
        assert_eq!(civil.minute, 0);
        assert_eq!(civil.second, 0);
        assert_eq!(civil.weekday, 3, "Thursday, in a week that starts on Monday");
        assert_eq!(civil.weekday_name(Language::English), "Thu");
        assert_eq!(civil.weekday_name(Language::Chinese), "周四");
    }

    /// The offset lands on the *day*, which is the case a naive `hour + 8` gets wrong.
    #[test]
    fn a_late_utc_evening_is_the_next_morning_here() {
        // Half past eleven on a Friday night in UTC, which is half past seven on Saturday morning
        // here — the date and the weekday both moved with the hour.
        let utc = civil_seconds(2026, 10, 9, 23, 30, 0);
        let civil = Civil::from_unix(utc);

        assert_eq!(civil.hour, 7);
        assert_eq!(civil.minute, 30);
        assert_eq!(civil.date(), "2026-10-10");
        assert_eq!(civil.weekday_name(Language::English), "Sat");
    }

    /// A leap day, because February is the month the March-shifted arithmetic exists for.
    #[test]
    fn the_twenty_ninth_of_february_is_a_day() {
        let civil = Civil::from_unix(local_seconds(2024, 2, 29, 20, 0, 0));

        assert_eq!((civil.year, civil.month, civil.day), (2024, 2, 29));
        assert_eq!(civil.hour, 20);
        assert_eq!(civil.weekday_name(Language::English), "Thu");
    }

    /// A day that is neither the epoch nor a leap day, and whose weekday is known independently of
    /// this code: the 12th of October 2026 is a Monday.
    #[test]
    fn an_ordinary_monday_is_a_monday() {
        let civil = Civil::from_unix(local_seconds(2026, 10, 12, 9, 30, 0));

        assert_eq!(civil.date(), "2026-10-12");
        assert_eq!(civil.hh_mm(), "09:30");
        assert_eq!(civil.hh_mm_ss(), "09:30:00");
        assert_eq!(civil.weekday_name(Language::Chinese), "周一");
    }

    /// The last day of a year, and the first of the next: the boundary the shift to March is there to
    /// get right.
    #[test]
    fn the_year_turns_where_it_should() {
        let last = Civil::from_unix(local_seconds(2026, 12, 31, 23, 59, 59));
        let first = Civil::from_unix(local_seconds(2027, 1, 1, 0, 0, 0));

        assert_eq!(last.date(), "2026-12-31");
        assert_eq!(first.date(), "2027-01-01");
        assert_eq!(last.second, 59);
        assert_eq!(first.second, 0);
    }

    /// The instant before the epoch, which a `seconds / 86400` truncated towards zero would put on the
    /// wrong day: an hour before the epoch is still 1969 in UTC, and eight in the morning of the 1st
    /// of January here.
    #[test]
    fn the_instant_before_the_epoch_is_the_morning_of_the_first() {
        let civil = Civil::from_unix(-1);

        assert_eq!((civil.year, civil.month, civil.day), (1970, 1, 1));
        assert_eq!(civil.hh_mm_ss(), "07:59:59");
    }

    /// The hands of the dial: the hour hand at half past four has to be between the four and the five,
    /// which is the whole of why the angles are fractions and not indices.
    #[test]
    fn the_hands_sit_between_the_hours() {
        let civil = Civil::from_unix(local_seconds(2026, 10, 12, 4, 30, 0));

        assert_eq!(civil.hour_angle(), 4.5);
        assert_eq!(civil.minute_angle(), 30.0);
        assert_eq!(civil.second_angle(), 0.0);
    }

    /// A countdown shows its hours only when there are any.
    #[test]
    fn a_countdown_is_minutes_until_it_is_hours() {
        assert_eq!(countdown(0), "00:00");
        assert_eq!(countdown(59), "00:59");
        assert_eq!(countdown(1_500), "25:00");
        assert_eq!(countdown(3_600), "1:00:00");
        assert_eq!(countdown(3_661), "1:01:01");
    }

    /// The uptime line, in both languages, at the three scales it is read at.
    #[test]
    fn an_uptime_is_read_in_the_units_it_has() {
        assert_eq!(duration_text(90, Language::Chinese), "1 分");
        assert_eq!(duration_text(3_600 + 120, Language::Chinese), "1 小时 2 分");
        assert_eq!(
            duration_text(86_400 * 2 + 3 * 3600, Language::Chinese),
            "2 天 3 小时"
        );
        assert_eq!(duration_text(90, Language::English), "1m");
        assert_eq!(duration_text(3_720, Language::English), "1h 2m");
        assert_eq!(duration_text(86_400 * 2 + 3 * 3600, Language::English), "2d 3h");
    }

    /// The instant at which the wall clock here reads this date and time.
    ///
    /// The offset is taken *off* the reading rather than added to it, so that a test can say
    /// "2026-12-31 at 23:59:59" — the thing a person sees on the dial — and get the instant that
    /// produces it. The tests above are about what the dial reads, and writing them in the same terms
    /// is what keeps them from being arithmetic about arithmetic.
    fn local_seconds(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        civil_seconds(year, month, day, hour, minute, second) - UTC_OFFSET_SECS
    }

    /// The seconds a wall-clock reading is, in UTC, for the tests above to be written in dates.
    ///
    /// Not a leap-second-aware conversion and not trying to be: it is the same three-field arithmetic
    /// that is being tested, run backwards from dates a person reads, and it is only ever called from
    /// this module's own tests.
    fn civil_seconds(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        days_from_civil(year, month, day) * 86_400
            + hour as i64 * 3600
            + minute as i64 * 60
            + second as i64
    }

    /// Hinnant's `days_from_civil`, for the tests that want to write a date rather than a timestamp.
    fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let march_month = if month > 2 { month - 3 } else { month + 9 } as i64;
        let day_of_year = (153 * march_month + 2) / 5 + day as i64 - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

        era * 146_097 + day_of_era - 719_468
    }
}
