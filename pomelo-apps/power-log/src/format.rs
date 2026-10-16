//! The battery's numbers, as the strings the page draws.
//!
//! Every one of these is a pure function of a number and returns a `String`, which is the whole reason
//! they sit apart from `lib`: the page's *format* is the app's own and the numbers are the board's, and
//! the format is the half that can be tested without a board, a window or a widget tree.
//!
//! # The one rule
//!
//! A unit is never dropped and never guessed at. `4.05 V` says which scale the number is on, `3h 12m`
//! says it is a span, and [`missing`] says the board could not answer — where a `0` would be a reading
//! and a lie.

use std::time::Duration;

/// The dash this page draws where the board could not answer.
///
/// Not a zero and not an empty string: a zero is a reading that happens to be nought, and a page that
/// draws one for a failed read is a page that says the battery is empty when the gauge is merely
/// broken. An empty string disappears into the row and says nothing at all.
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

/// A charge, to whole percent: `84%`.
///
/// Rounded rather than floored, so that a gauge reading 99.6 says 100 and not 99 — the number and the
/// bar drawn under it are the same reading twice, and the two disagreeing by a rounding step is what
/// makes a person stop trusting the page.
pub fn percent(share: f32) -> String {
    format!("{}%", share.round() as i64)
}

/// A cell voltage, in volts to two places: `4.05 V`.
///
/// Two places because the last digit is the one that moves: a cell on a charger sits at 4.20 and one
/// off it at 3.70, and a whole-volt reading of either is `4 V`.
pub fn volts(millivolts: u32) -> String {
    format!("{:.2} V", millivolts as f64 / 1000.0)
}

/// A temperature: `41°C`.
pub fn degrees(celsius: f32) -> String {
    format!("{}°C", celsius.round() as i64)
}

/// A span of time in the two largest units that fit it: `2d 4h`, `3h 05m`, `12m 30s`, `7s`.
///
/// Two units and no more, because the third is below the resolution of anything on this page — nobody
/// reading an estimate asks for the seconds of a two-day one. The smaller unit is zero-padded below the
/// larger across minutes and seconds (`3m 05s`) and not across hours (`2d 4h`), which is the shape a
/// clock's own readout has.
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

/// How fast the battery is going, as whole percent an hour: `4%/h`.
///
/// An hour rather than a second because that is the scale a person reads a discharge at: a cell that
/// loses 0.0011% a second is losing 4% an hour, and only the second number means anything.
pub fn rate(percent_per_second: f64) -> String {
    format!("{}%/h", (percent_per_second * 3_600.0).round() as i64)
}

/// A charge as the fraction a bar is drawn at, clamped to `0.0..=1.0`.
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

    /// Charges are whole percent, rounded, and the missing reading is not one of them.
    #[test]
    fn a_charge_is_whole_percent() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(83.6), "84%");
        assert_eq!(percent(99.6), "100%", "the bar and the number agree at the top");
        assert_eq!(missing(), MISSING);
        assert_ne!(percent(0.0), MISSING, "a flat battery is not a broken gauge");
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

    /// The rate is per hour because that is the scale it is read at, and it rounds rather than
    /// truncates so that a slow drain does not read as none at all.
    #[test]
    fn a_rate_is_whole_percent_an_hour() {
        assert_eq!(rate(0.0), "0%/h");
        assert_eq!(rate(4.0 / 3_600.0), "4%/h");
        assert_eq!(rate(0.002), "7%/h");
        assert_eq!(rate(-0.01), "-36%/h", "charging carries its own sign");
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
