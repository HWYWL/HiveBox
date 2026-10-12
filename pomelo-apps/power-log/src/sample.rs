//! The samples the page is made of, and the file they are kept in.
//!
//! One [`Sample`] is one look at the gauge, and everything the page says about the battery is derived
//! from a list of them: the curve is their shape, the estimate is their slope, and the log on disk is
//! them written down. That is why this module is a value and a pair of pure functions rather than
//! anything that knows about a board — the arithmetic that draws a conclusion about a battery is the
//! part worth testing on its own, and a test can hand it a curve by hand.
//!
//! # The file
//!
//! A header line naming what the timestamps are counted from, then one comma-separated sample a line:
//!
//! ```text
//! # pomelo power log v1 unix
//! 1750000000,84,4050,41.5,0
//! ```
//!
//! The header is not decoration. A timestamp is only meaningful against a zero, and this board has two
//! possible ones: the wall clock, once something has told it the time, and the uptime, before that. A
//! log whose header said `unix` and a page reading it under `uptime` would draw a curve whose left half
//! was a different afternoon, so [`parse`] hands the base back and the app keeps the two apart.

use std::time::Duration;

/// The version of the log format, written into the header so that a later format can tell itself apart
/// from this one.
pub const VERSION: u32 = 1;

/// What a log's timestamps are counted from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// Seconds since the Unix epoch, from a clock that has been set.
    Unix,
    /// Seconds since the board came up, from a board that has not been told the time yet.
    Uptime,
}

impl Base {
    /// The word the header carries.
    pub fn word(self) -> &'static str {
        match self {
            Base::Unix => "unix",
            Base::Uptime => "uptime",
        }
    }

    /// The word read back, or `None` for a header this app does not know.
    pub fn of_word(word: &str) -> Option<Self> {
        match word {
            "unix" => Some(Base::Unix),
            "uptime" => Some(Base::Uptime),
            _ => None,
        }
    }
}

/// One look at the gauge, with the moment it was taken.
///
/// The two optional numbers are optional for the board's reason and not for tidiness: a backend that
/// cannot read a cell voltage says so rather than inventing one, and a sample missing it is still a
/// sample — the charge is what the curve is drawn from, and a gauge that answered is a point worth
/// keeping whatever else it could not say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// When it was taken, in seconds from the base the header names.
    pub at: u64,
    /// The charge, `0..=100`.
    pub percent: u8,
    /// The cell voltage, when the backend could read one.
    pub voltage_mv: Option<u32>,
    /// The PMIC's temperature, when the backend could read one.
    pub temperature_c: Option<f32>,
    /// Whether the cell was on a charger at that moment.
    pub charging: bool,
}

/// The header line a log of this base begins with.
pub fn header(base: Base) -> String {
    format!("# pomelo power log v{VERSION} {}\n", base.word())
}

/// One sample as its line, newline included.
///
/// A number the board could not answer is an empty field rather than a zero, which is the same rule the
/// page draws by and the reason the parser can tell "no voltage" from "no volts".
pub fn line(sample: &Sample) -> String {
    let voltage = match sample.voltage_mv {
        Some(millivolts) => millivolts.to_string(),
        None => String::new(),
    };

    let temperature = match sample.temperature_c {
        Some(celsius) => format!("{celsius:.1}"),
        None => String::new(),
    };

    format!(
        "{},{},{},{},{}\n",
        sample.at,
        sample.percent,
        voltage,
        temperature,
        u8::from(sample.charging)
    )
}

/// A line read back as a sample, or `None` for a line that is not one.
fn read(line: &str) -> Option<Sample> {
    let mut fields = line.split(',');

    let at: u64 = fields.next()?.trim().parse().ok()?;
    let percent: u8 = fields.next()?.trim().parse().ok()?;

    let voltage_mv = fields.next().and_then(|field| field.trim().parse().ok());
    let temperature_c = fields.next().and_then(|field| field.trim().parse().ok());
    let charging = matches!(fields.next().map(str::trim), Some("1") | Some("true"));

    Some(Sample {
        at,
        percent,
        voltage_mv,
        temperature_c,
        charging,
    })
}

/// A whole log read back: the base its header named, and the samples under it.
///
/// Forgiving on purpose. The file is a log on a filesystem with no journal, and its most likely
/// damage is a last line cut short by a power cut — so a line that does not parse is dropped rather
/// than failing the read, and the rest of the history is still worth having. A header that is absent
/// or in a word this version does not know gives `None`, and the samples under it are still returned:
/// what the page does with a base it does not trust is the app's decision, not the parser's.
pub fn parse(text: &str) -> (Option<Base>, Vec<Sample>) {
    let mut base = None;
    let mut samples = Vec::new();

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('#') {
            if base.is_none() {
                base = rest.split_whitespace().last().and_then(Base::of_word);
            }

            continue;
        }

        if let Some(sample) = read(line) {
            samples.push(sample);
        }
    }

    (base, samples)
}

/// How long a curve has to be before a slope is drawn through it.
///
/// Two points five seconds apart say nothing about a battery: the gauge's own last digit moves by a
/// percent either way, and a rate taken over a moment is a rate taken over the gauge's noise. Five
/// minutes is the shortest window this page will read a discharge out of, and below it the estimate
/// is [`format::missing`](crate::format::missing) rather than a number nobody should act on.
pub const MIN_SPAN: Duration = Duration::from_secs(300);

/// How long the samples cover, from the first to the last.
pub fn span(samples: &[Sample]) -> Duration {
    match (samples.first(), samples.last()) {
        (Some(first), Some(last)) => Duration::from_secs(last.at.saturating_sub(first.at)),
        _ => Duration::ZERO,
    }
}

/// How fast the charge is falling, in percent a second, or `None` when the curve is too short to say.
///
/// Only the samples that were *off* a charger are read, because a charge that climbed between two
/// points while plugged in is not a discharge with a sign on it — it is a different thing, and the
/// slope between the two would be an average of both. The rate is signed: the page draws a positive
/// one as time left and a negative or flat one as nothing to estimate from.
pub fn drain_rate(samples: &[Sample]) -> Option<f64> {
    let mut discharging = samples.iter().filter(|sample| !sample.charging);

    let first = discharging.next()?;
    let last = discharging.last()?;

    let seconds = last.at.saturating_sub(first.at);
    if Duration::from_secs(seconds) < MIN_SPAN {
        return None;
    }

    Some((first.percent as f64 - last.percent as f64) / seconds as f64)
}

/// What is left of the battery, from the shape of the curve so far.
///
/// The estimate is the plainest possible one — the charge left, over the rate it is leaving at — and it
/// is deliberately not cleverer than that. A fuel gauge is not linear and the last hour is not the
/// next one, so a curve fitted through the whole window and carried forward would be a confident
/// number about a battery that has not decided yet. This is the current slope continued, which is
/// wrong in the way anyone reading it already knows it is wrong.
///
/// The current look is passed rather than read off the end of the curve, because the two are not the
/// same instant: a point is only added to the curve once a minute, and a cell plugged in since the last
/// one is charging *now* whatever the curve says.
///
/// `None` while the cell is charging, when the charge is already empty, and when the curve is flat or
/// rising: an estimate is a statement about a discharge, and there is only one to make when there is
/// one happening.
pub fn remaining(samples: &[Sample], now: &Sample) -> Option<Duration> {
    if now.charging || now.percent == 0 {
        return None;
    }

    let rate = drain_rate(samples)?;
    if !(rate > 0.0) {
        return None;
    }

    let seconds = now.percent as f64 / rate;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }

    Some(Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample with nothing the board could not say.
    fn at(seconds: u64, percent: u8, charging: bool) -> Sample {
        Sample {
            at: seconds,
            percent,
            voltage_mv: Some(3300 + percent as u32 * 9),
            temperature_c: Some(41.5),
            charging,
        }
    }

    /// What was written is what is read, header and all, down to a number the board could not answer.
    #[test]
    fn a_log_round_trips_through_its_own_text() {
        let written = format!(
            "{}{}{}",
            header(Base::Unix),
            line(&at(1_750_000_000, 84, false)),
            line(&Sample {
                at: 1_750_003_600,
                percent: 80,
                voltage_mv: None,
                temperature_c: None,
                charging: true,
            })
        );

        let (base, samples) = parse(&written);

        assert_eq!(base, Some(Base::Unix));
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0], at(1_750_000_000, 84, false));
        assert_eq!(
            samples[1],
            Sample {
                at: 1_750_003_600,
                percent: 80,
                voltage_mv: None,
                temperature_c: None,
                charging: true,
            },
            "a field the board could not fill comes back empty rather than zero"
        );
    }

    /// A log cut short by a power cut loses one line and keeps the rest, and junk in the middle does
    /// not cost the lines on either side of it.
    #[test]
    fn a_damaged_log_keeps_the_lines_that_survived() {
        let text = "\
# pomelo power log v1 unix
1750000000,84,4050,41.5,0
1750003600
not a sample at all
1750007200,80,4020,44.0,0
";
        let (base, samples) = parse(text);

        assert_eq!(base, Some(Base::Unix));
        assert_eq!(
            samples.len(),
            2,
            "the half-written line and the junk are dropped"
        );
        assert_eq!(samples[0].percent, 84);
        assert_eq!(samples[1].percent, 80);
    }

    /// A line with two fields is *not* damage: a sample that answered about its charge and nothing
    /// else is a reading, and the parser keeps it — which is the difference between a truncated write
    /// and a backend that had no voltage to give.
    #[test]
    fn a_sample_that_answered_about_its_charge_alone_is_still_a_sample() {
        let (_, samples) = parse("1750000000,84\n");

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].percent, 84);
        assert_eq!(samples[0].voltage_mv, None);
        assert_eq!(samples[0].temperature_c, None);
        assert!(!samples[0].charging);
    }

    /// The base is a word this app either knows or does not, and a header it does not know is not a
    /// base it guesses at.
    #[test]
    fn only_the_two_bases_are_read_back() {
        assert_eq!(Base::of_word("unix"), Some(Base::Unix));
        assert_eq!(Base::of_word("uptime"), Some(Base::Uptime));
        assert_eq!(Base::of_word("seconds"), None);

        let (base, _) = parse("# pomelo power log v1 sidereal\n");
        assert_eq!(base, None, "a header from a format that is not this one");
    }

    /// An empty file is a log nobody has written yet, not a failure: a first run has no history and
    /// that is exactly what it should show.
    #[test]
    fn an_empty_log_is_an_empty_history() {
        assert_eq!(parse(""), (None, Vec::new()));
        assert_eq!(parse("\n\n"), (None, Vec::new()));
    }

    /// A slope needs two discharging samples and a window worth reading, and a charger in between is
    /// not a discharge with a sign on it.
    #[test]
    fn a_rate_needs_a_discharge_over_a_window() {
        let flat = [at(0, 84, false), at(3_600, 84, false)];
        assert_eq!(drain_rate(&flat), Some(0.0));

        let draining = [at(0, 84, false), at(3_600, 80, false)];
        assert_eq!(drain_rate(&draining), Some(4.0 / 3_600.0));

        assert_eq!(
            drain_rate(&[at(0, 84, false), at(60, 83, false)]),
            None,
            "a minute is the gauge's own noise"
        );
        assert_eq!(drain_rate(&[at(0, 84, false)]), None, "one point is not a slope");
        assert_eq!(
            drain_rate(&[at(0, 84, true), at(3_600, 80, true)]),
            None,
            "charging samples are not a discharge"
        );
    }

    /// The estimate is the charge left over the rate it is leaving at, and a charger or a flat cell
    /// leaves nothing to estimate.
    #[test]
    fn what_is_left_is_the_charge_over_its_rate() {
        let draining = [at(0, 84, false), at(3_600, 80, false)];

        // 4% an hour, 80% left: twenty hours.
        let left = remaining(&draining, &at(3_600, 80, false)).expect("a discharging curve");
        assert_eq!(left, Duration::from_secs(20 * 3_600));

        assert_eq!(
            remaining(&draining, &at(3_600, 80, true)),
            None,
            "a cell on a charger is not running out, whatever the curve was doing"
        );
        assert_eq!(
            remaining(&[at(0, 4, false), at(3_600, 0, false)], &at(3_600, 0, false)),
            None,
            "and an empty one has nothing left to count down"
        );
        assert_eq!(
            remaining(
                &[at(0, 80, false), at(3_600, 84, false)],
                &at(3_600, 84, false)
            ),
            None,
            "a charge that rose is not a discharge, whatever the charger said"
        );
        assert_eq!(
            remaining(&[at(0, 84, false), at(3_600, 80, false)], &at(3_600, 80, false)),
            Some(Duration::from_secs(20 * 3_600)),
            "and the current charge is what is counted down, not the last point"
        );
    }

    /// A span is the first sample to the last, and no samples span nothing.
    #[test]
    fn a_span_is_the_first_sample_to_the_last() {
        assert_eq!(span(&[]), Duration::ZERO);
        assert_eq!(span(&[at(10, 84, false)]), Duration::ZERO);
        assert_eq!(span(&[at(10, 84, false), at(3_610, 80, false)]), Duration::from_secs(3_600));
    }
}
