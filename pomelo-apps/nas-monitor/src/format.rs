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

/// What a value the machine did not report is drawn as: a dash, because a blank cell reads as a page
/// that failed to load and a zero would be a number it never said.
///
/// A **fullwidth** hyphen-minus and not the em dash a page would normally use for this. The panel's
/// text font is a subset of Source Han Sans — `pomelo-font`'s `charset-common.txt` — and U+2014 is not
/// in it: an em dash draws as `.notdef`, a hollow box, which is *less* honest than nothing at all. A
/// fullwidth hyphen-minus is in the charset, is the same width and sits at the same height, so it is
/// the same mark on the panel and a real glyph on the desktop.
///
/// The same trap is worth knowing about for any other mark this page might want: `·` (U+00B7), `°`
/// (U+00B0) and `…` (U+2026) are in the charset, an en dash and an em dash are not.
pub const NOTHING: &str = "－";

/// How wide `text` is at `size`, by this page's own estimate: under two thirds of an em per narrow
/// character and a whole one per full-width character.
///
/// An estimate, and deliberately pessimistic — it is the one the disk table's width test has always
/// used. It lives here rather than in that test because [`fit`] has to *draw* by the same arithmetic
/// the test *checks* by: a table whose cells were cut by one rule and measured by another would pass
/// its own test and still arrive on the panel with half a model.
///
/// `°` counts as narrow, which is the reason for the clause: it is not ASCII and it is not a
/// full-width character either, and a `38°C` charged a whole em for its degree sign is a check that
/// fails on a temperature that fits.
pub fn width_of(text: &str, size: f32) -> f32 {
    text.chars().map(|c| char_width(c, size)).sum()
}

/// The width of one character in that estimate.
fn char_width(character: char, size: f32) -> f32 {
    if character.is_ascii() || character == '°' {
        size * 0.62
    } else {
        size
    }
}

/// `text` cut down to what `room` pixels can hold, with `...` where the rest went.
///
/// The one thing on this page that is *long*: a drive's identity — the kernel's name for it and the
/// model, which runs to twenty characters and is the widest string a NAS reports. It goes under the
/// disk's use, in a column five numbers have already taken their share of, so a cell that cannot fit
/// it says the beginning of it rather than pushing the five columns beside it off the panel. See the
/// disk table's arithmetic, which is what makes `room` a number rather than a hope.
///
/// By *width* and not by character count, because both a `WDC` and a `存储空间` can arrive in the same
/// column and a count that is right for one is wrong for the other.
pub fn fit(text: &str, room: f32, size: f32) -> String {
    if width_of(text, size) <= room {
        return text.to_string();
    }

    // What is left once the ellipsis has been paid for. A room too small even for that keeps the
    // ellipsis: a cell that cannot say anything should still say that it could not.
    let budget = room - width_of("...", size);

    let mut kept = String::new();
    let mut used = 0.0;

    for character in text.chars() {
        let width = char_width(character, size);

        if used + width > budget {
            break;
        }

        kept.push(character);
        used += width;
    }

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

/// How many drives the machine has, as the words for them: `3 块磁盘`, `1 disk`, `4 disks`.
///
/// The one string on this page that changes *grammar* with the language rather than units: Chinese counts
/// with a measure word and does not inflect, English pluralizes, and `1 disks` is the kind of thing a
/// reader notices before anything else on the page. Here for the reason [`uptime`] is — see its note.
pub fn disks(count: usize, language: Language) -> String {
    match language {
        Language::Chinese => format!("{count} 块磁盘"),
        Language::English => match count {
            1 => String::from("1 disk"),
            _ => format!("{count} disks"),
        },
    }
}

/// How many machines the board has been told about: `3 台服务器`, `1 server`, `4 servers`.
///
/// The same shape as [`disks`] and for the same reason — see its note — with the noun this app is
/// actually about.
pub fn machines(count: usize, language: Language) -> String {
    match language {
        Language::Chinese => format!("{count} 台服务器"),
        Language::English => match count {
            1 => String::from("1 server"),
            _ => format!("{count} servers"),
        },
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

    /// The placeholder is a character the panel's font actually has, which an em dash is not.
    ///
    /// This is the test that stops somebody tidying `－` back into the `—` it reads as: the panel's
    /// subset has no em dash, so `—` arrives as `.notdef` — the hollow box the page used to draw in the
    /// CPU card. See [`NOTHING`], which is where the reasoning is.
    #[test]
    fn the_placeholder_is_a_glyph_the_panel_can_draw() {
        assert_eq!(NOTHING, "－", "a fullwidth hyphen-minus");

        let glyph = NOTHING.chars().next().expect("a placeholder is a character");
        assert!(
            !['—', '–'].contains(&glyph),
            "an em dash is not in the subset: the panel draws it as a box"
        );
        assert_eq!(
            width_of(NOTHING, 20.0),
            20.0,
            "and being full-width, it takes the room the em dash would have"
        );
    }

    /// A cell's text is cut to the room it has — *by width* and not by character count, because a
    /// `WDC` and a `存储空` are the same number of characters and nowhere near the same width.
    #[test]
    fn text_is_cut_to_the_room_it_has() {
        // What fits is left exactly as it is.
        assert_eq!(fit("sdb", 40.0, 20.0), "sdb");
        assert_eq!(fit("sdb · WDC WD40EFRX", 1000.0, 20.0), "sdb · WDC WD40EFRX");

        // And what does not fit stops on the character that overflowed it. At 20 px a narrow
        // character is 12.4 wide, so 100 px less the 37.2 an ellipsis costs leaves 62.8 — five
        // characters, and the sixth is the one that did not fit.
        assert_eq!(fit("WDC WD40EFRX-68N32N0", 100.0, 20.0), "WDC W...");
        assert_eq!(
            fit("存储空间 1", 100.0, 20.0),
            "存储空...",
            "a full-width character is charged a whole em"
        );

        // A room not even wide enough for the ellipsis keeps the ellipsis: a cell that can say
        // nothing should still say that it could not.
        assert_eq!(fit("WDC WD40EFRX", 10.0, 20.0), "...");

        // And whatever comes out fits the room it was given, which is the promise the table's
        // arithmetic rests on.
        for room in [10.0, 37.0, 62.8, 100.0, 155.0, 480.0] {
            for text in ["sdb", "mmcblk0boot0 · WDC WD40EFRX-68N32N0", "存储空间 1", ""] {
                let cut = fit(text, room, 17.28);

                assert!(
                    width_of(&cut, 17.28) <= room.max(width_of("...", 17.28)),
                    "「{cut}」 does not fit {room} px"
                );
            }
        }
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

    /// A drive count is counted the language's way: a measure word in Chinese, singular and plural in
    /// English. The singular is the case worth a test, because it is the one a count of one gets wrong.
    #[test]
    fn a_drive_count_counts_the_languages_way() {
        assert_eq!(disks(1, Language::Chinese), "1 块磁盘");
        assert_eq!(disks(4, Language::Chinese), "4 块磁盘");

        assert_eq!(disks(1, Language::English), "1 disk");
        assert_eq!(disks(4, Language::English), "4 disks");

        assert_eq!(machines(1, Language::Chinese), "1 台服务器");
        assert_eq!(machines(3, Language::Chinese), "3 台服务器");
        assert_eq!(machines(1, Language::English), "1 server");
        assert_eq!(machines(3, Language::English), "3 servers");
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
