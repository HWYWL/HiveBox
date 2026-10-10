//! The arithmetic a page about another machine is made of, kept out of the widgets.
//!
//! Nothing here reads a board or draws anything: every function takes a number and returns a string,
//! which is what makes them testable without a panel — and what keeps the one thing that must not
//! drift (how a byte count is spelled) defined in exactly one place. The task viewer's `format` is the
//! same idea; the numbers are different because these are rates.

use std::time::Duration;

use pomelo_widgets::preferences::Language;

/// A byte count as the shortest thing that still means it: `8.4M`, `512K`, `912`.
///
/// The *bytes*, and the base is 1024 because the machine at the other end counts in it: a NAS's
/// `MemTotal` is in kibibytes and its `iostat` in blocks, so a decimal megabyte would be a number
/// nobody could reconcile with anything they could check.
pub fn bytes(bytes: u64) -> String {
    const T: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;
    const G: f64 = 1024.0 * 1024.0 * 1024.0;
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;

    let bytes = bytes as f64;

    if bytes >= T {
        format!("{}T", scaled(bytes / T))
    } else if bytes >= G {
        format!("{}G", scaled(bytes / G))
    } else if bytes >= M {
        format!("{}M", scaled(bytes / M))
    } else if bytes >= K {
        format!("{:.0}K", bytes / K)
    } else {
        format!("{bytes:.0}")
    }
}

/// A count in a unit, with the precision the size deserves: one decimal below ten and none above.
///
/// `9.5M` and `500G`, and the two are the same rule rather than two: a tenth is a fact a person can act
/// on when the number is small, and it is noise — three characters of noise, in a table's narrowest
/// columns — when the number is large. The disk table is what settles it: `500.0G` does not fit a column
/// that `500G` does.
fn scaled(value: f64) -> String {
    if value >= 10.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// The first `chars` characters of `text`, with `...` where the rest went.
///
/// The one thing on this page that is *long*: a drive's model, which runs to twenty characters and is
/// the widest string a NAS reports. It goes under the disk's use, in that column, and a cell that cannot
/// fit it says the beginning of it rather than pushing the five columns beside it off the panel.
pub fn shorten(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }

    let kept: String = text.chars().take(chars.saturating_sub(3)).collect();

    format!("{kept}...")
}

/// A rate, which is what a network and a disk are measured in: `8.4M/s`.
///
/// One decimal below ten and none above, because that is the width of the fact: `9.8M/s` and `12M/s`
/// are both the precision somebody wants, and `9.84M/s` is a number pretending to a measurement this
/// board does not make.
pub fn rate(bytes_per_sec: u64) -> String {
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;

    let per_sec = bytes_per_sec as f64;

    if per_sec >= M {
        format!("{}M/s", scaled(per_sec / M))
    } else if per_sec >= K {
        format!("{:.0}K/s", per_sec / K)
    } else {
        format!("{per_sec:.0}B/s")
    }
}

/// A temperature, as the machine reports it: `42°C`.
///
/// Whole degrees: a drive's sensor reports whole ones, and a tenth of a degree on a page about a
/// machine in another room is a precision nobody is asking for.
pub fn temperature(celsius: f32) -> String {
    format!("{celsius:.0}°C")
}

/// How long a machine has been up, in the two units that matter.
///
/// Two, and not years-months-days-hours: an uptime is read to answer "has this been up since the power
/// went off", and `17天4时` answers it while `17天4时21分` makes the reader do the same work twice.
/// Which two depends on how long it has been, so a machine up for an hour says minutes.
///
/// The units are the language's: 天/时/分 against `d`/`h`/`m`. A number is a number either way, and
/// this is the one place in this crate where the *shape* of the output changes with the language.
pub fn uptime(duration: Duration, language: Language) -> String {
    let seconds = duration.as_secs();
    let (days, hours, minutes) = (
        seconds / 86_400,
        seconds % 86_400 / 3_600,
        seconds % 3_600 / 60,
    );

    let (day, hour, minute) = match language {
        Language::Chinese => ("天", "时", "分"),
        Language::English => ("d", "h", "m"),
    };

    if days > 0 {
        format!("{days}{day}{hours}{hour}")
    } else if hours > 0 {
        format!("{hours}{hour}{minutes}{minute}")
    } else {
        format!("{minutes}{minute}")
    }
}

/// How long ago something happened, in the words a person would use: `刚刚`, `12 秒前`, `4 分钟前`.
///
/// The rounding is deliberately coarse and always *down*: a reading taken 90 seconds ago is "1 分钟前"
/// and not "2 分钟前", because the question an age answers is how much trust to place in the number
/// beside it, and rounding up would overstate the age of a reading that is nearly a minute fresher than
/// it says.
pub fn age(duration: Duration, language: Language) -> String {
    let seconds = duration.as_secs();

    let (just_now, secs, mins) = match language {
        Language::Chinese => (String::from("刚刚"), "秒前", "分钟前"),
        Language::English => (String::from("just now"), "s ago", "min ago"),
    };

    if seconds < 2 {
        just_now
    } else if seconds < 60 {
        format!("{seconds} {secs}")
    } else {
        format!("{} {mins}", seconds / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes are shortened at the unit they are at, and stay exact below a kilobyte. Terabytes are the
    /// case that matters here: a NAS's disks are the largest numbers on the page, and `4194304.0M` is
    /// what a round of this that stopped at megabytes would print for one.
    #[test]
    fn a_byte_count_is_short_and_readable() {
        assert_eq!(bytes(0), "0");
        assert_eq!(bytes(912), "912");
        assert_eq!(bytes(1024), "1K");
        assert_eq!(bytes(1536), "2K");
        assert_eq!(bytes(1024 * 1024), "1.0M");
        assert_eq!(bytes(9 * 1024 * 1024 + 512 * 1024), "9.5M");
        assert_eq!(bytes(500 * 1024 * 1024 * 1024), "500G");
        assert_eq!(bytes(4 * 1024 * 1024 * 1024 * 1024), "4.0T");
    }

    /// A model is cut to the room a table cell has, and one that fits is left alone.
    #[test]
    fn a_long_model_is_shortened_and_a_short_one_is_not() {
        assert_eq!(shorten("sdb", 14), "sdb");
        assert_eq!(shorten("WDC WD40EFRX-68N32N0", 14), "WDC WD40EFR...");
        assert_eq!(shorten("Samsung SSD 870 EVO 500GB", 14), "Samsung SSD...");
    }

    /// A rate says so, and picks its precision by size: one decimal below ten, none above.
    #[test]
    fn a_rate_says_per_second_and_not_more_precision_than_there_is() {
        assert_eq!(rate(0), "0B/s");
        assert_eq!(rate(512), "512B/s");
        assert_eq!(rate(8_192), "8K/s");
        assert_eq!(rate(9 * 1024 * 1024), "9.0M/s");
        assert_eq!(rate(12 * 1024 * 1024), "12M/s");
    }

    /// An uptime is two units, and which two depends on how long it has been.
    #[test]
    fn an_uptime_is_the_two_units_that_matter() {
        let (zh, en) = (Language::Chinese, Language::English);

        assert_eq!(uptime(Duration::from_secs(90), zh), "1分");
        assert_eq!(uptime(Duration::from_secs(3 * 3_600 + 4 * 60), zh), "3时4分");
        assert_eq!(
            uptime(Duration::from_secs(17 * 86_400 + 4 * 3_600 + 21 * 60), zh),
            "17天4时"
        );

        assert_eq!(uptime(Duration::from_secs(90), en), "1m");
        assert_eq!(uptime(Duration::from_secs(3 * 3_600 + 4 * 60), en), "3h4m");
        assert_eq!(
            uptime(Duration::from_secs(17 * 86_400 + 4 * 3_600 + 21 * 60), en),
            "17d4h"
        );
    }

    /// An age never rounds *up*, because rounding up would understate a fresh reading.
    #[test]
    fn an_age_rounds_down_so_a_fresh_reading_is_never_older_than_it_says() {
        let zh = Language::Chinese;

        assert_eq!(age(Duration::from_millis(200), zh), "刚刚");
        assert_eq!(age(Duration::from_secs(12), zh), "12 秒前");
        assert_eq!(age(Duration::from_secs(90), zh), "1 分钟前");
        assert_eq!(age(Duration::from_secs(119), zh), "1 分钟前");
    }

    /// A temperature is whole degrees, because that is what the sensor reports.
    #[test]
    fn a_temperature_is_whole_degrees() {
        assert_eq!(temperature(41.6), "42°C");
        assert_eq!(temperature(37.0), "37°C");
    }
}
