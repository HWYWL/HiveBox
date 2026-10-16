//! The board's numbers, as the strings the page draws.
//!
//! Every one of these is a pure function of a number and returns a `String`, which is the whole
//! reason they are here rather than in `lib`: a dashboard is mostly *its* formatting — the readings it
//! shows are the board's and not this app's — and formatting is the part of it that can be tested
//! without a board, a window or a widget tree.
//!
//! # The one rule
//!
//! A unit is never dropped and never guessed at. `3.0 MB` says which power of two the number is in,
//! `40%` says it is a share of something, and `－` says the board could not answer — where a `0` would
//! be a reading and a lie. [`missing`] is that last one, and it is a function so that the page has one
//! place to draw it from.

use std::time::Duration;

/// The dash this page draws where the board could not answer.
///
/// Not a zero and not an empty string: a zero is a reading that happens to be nought, and a dashboard
/// that draws one for a failed read is a dashboard that says the battery is flat when the gauge is
/// merely broken. An empty string disappears into the row and says nothing at all.
///
/// The fullwidth hyphen-minus and not the em dash, because it is the dash the rest of the box writes.
/// The panel's font is cut to exactly the characters this source tree draws (`tools/scan_charset.py`),
/// so reusing a character that is already written costs nothing, while a character nobody has written
/// yet is a box on the panel until the charset is rebaked and the board reflashed.
pub const MISSING: &str = "－";

/// [`MISSING`], as an owned string, for the places that need one.
pub fn missing() -> String {
    String::from(MISSING)
}

/// The units a byte count is shown in, in the order it climbs them.
const BYTE_UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];

/// A byte count, in the largest unit that leaves it readable: `512 B`, `8.0 KB`, `18.2 GB`.
///
/// Binary units — 1024 to the step — because that is what this board's filesystems count in, and a
/// size that has been converted to powers of ten to *look* rounder is a size that no longer divides
/// into the blocks the flash actually has.
///
/// Below a kilobyte the count is exact and the unit is `B`, because a filesystem's own small numbers
/// are the ones worth reading exactly: `1.0 KB` for 1024 bytes hides whether a file is empty.
pub fn bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;

    while value >= 1024.0 && unit + 1 < BYTE_UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", BYTE_UNITS[unit])
}

/// A share, to whole percent: `40%`.
///
/// Rounded rather than floored, so that a gauge reading 99.6 says 100 and not 99 — the numbers here are
/// as coarse as the bars beside them, and one of them disagreeing with the other is what makes a
/// person stop trusting the page.
pub fn percent(share: f32) -> String {
    format!("{}%", share.round() as i64)
}

/// A cell voltage, in volts to two places: `4.10 V`.
///
/// Two places because the last digit is the one that moves: a board on a charger sits at 4.20 and one
/// off it at 3.70, and a whole-volt reading of either is `4 V`.
pub fn volts(millivolts: u32) -> String {
    format!("{:.2} V", millivolts as f64 / 1000.0)
}

/// A temperature: `41°C`.
pub fn degrees(celsius: f32) -> String {
    format!("{}°C", celsius.round() as i64)
}

/// A span of time in the two largest units that fit it: `2d 4h`, `48h 32m`, `3m 12s`, `7s`.
///
/// Two units and no more, because the third is always below the resolution of anything on this page —
/// nobody reading an uptime wants the seconds of a two-day one. The smaller unit is zero-padded below
/// the larger across minutes and seconds (`3m 05s`) and not across hours (`2d 4h`), which is the shape
/// a clock's own readout has.
pub fn duration(span: Duration) -> String {
    let seconds = span.as_secs();

    let days = seconds / 86_400;
    let hours = seconds / 3_600 % 24;
    let minutes = seconds / 60 % 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {:02}s", seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

/// A share as the fraction a bar is drawn at, clamped to `0.0..=1.0`.
///
/// Clamped because a bar is a width and a width cannot be negative or wider than its track: a backend
/// answering 105% — and a battery gauge can — would otherwise draw a bar out of the end of the card it
/// is in. The number beside it is *not* clamped, and that is deliberate: the bar is what has to stay
/// inside the card, and the reading is what has to stay truthful.
pub fn fraction(share: f32) -> f32 {
    (share / 100.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte counts climb the units and stop at the largest one that is still readable.
    #[test]
    fn a_byte_count_is_shown_in_the_unit_that_fits_it() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1023), "1023 B", "a byte short of a kilobyte");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(8 * 1024), "8.0 KB");
        assert_eq!(bytes(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(bytes(16 * 1024 * 1024 * 1024), "16.0 GB");
    }

    /// A count that would take a unit this board has none of stops at the largest unit there is,
    /// rather than inventing one: a 2048 TB answer is `2048.0 TB` and not `2.0 PB`.
    #[test]
    fn a_byte_count_stops_at_the_largest_unit_it_has() {
        assert_eq!(bytes(2048 * 1024 * 1024 * 1024 * 1024), "2048.0 TB");
    }

    /// Shares are whole percent, rounded, and the missing reading is not one of them.
    #[test]
    fn a_share_is_whole_percent() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(39.6), "40%");
        assert_eq!(percent(99.6), "100%", "the bar and the number agree at the top");
        assert_eq!(percent(100.0), "100%");
        assert_eq!(missing(), MISSING);
    }

    /// Volts keep the two places that are the ones that move, and degrees none.
    #[test]
    fn the_small_units_keep_the_places_that_move() {
        assert_eq!(volts(4200), "4.20 V");
        assert_eq!(volts(3700), "3.70 V");
        assert_eq!(volts(0), "0.00 V");
        assert_eq!(degrees(41.4), "41°C");
        assert_eq!(degrees(-3.6), "-4°C", "below zero is still a temperature");
    }

    /// A span is two units, and which two depends on how big it is.
    #[test]
    fn a_span_is_measured_in_the_two_units_that_fit_it() {
        assert_eq!(duration(Duration::from_secs(7)), "7s");
        assert_eq!(duration(Duration::from_secs(72)), "1m 12s");
        assert_eq!(duration(Duration::from_secs(3 * 3_600 + 5 * 60)), "3h 05m");
        assert_eq!(
            duration(Duration::from_secs(2 * 86_400 + 4 * 3_600)),
            "2d 4h",
            "past a day the hours stop being padded"
        );
        assert_eq!(
            duration(Duration::from_secs(86_399)),
            "23h 59m",
            "the last second before a day is still hours"
        );
    }

    /// The bar is clamped and the number is not, because only one of them has to fit in a card.
    #[test]
    fn a_bar_is_clamped_where_a_reading_is_not() {
        assert_eq!(fraction(0.0), 0.0);
        assert_eq!(fraction(40.0), 0.4);
        assert_eq!(fraction(100.0), 1.0);
        assert_eq!(fraction(105.0), 1.0, "a gauge that overreads draws a full bar");
        assert_eq!(fraction(-5.0), 0.0);
    }
}
