//! The readout at the top of the main list: three gauges, two cards, and a footer.
//!
//! # What it is for, and why it is at the top
//!
//! Every card under it is a way *in*. This one is the only part of the list that is a *reading* —
//! what this machine is and how it is holding up — and it sits above the rows because that is the
//! order the questions come in: "is anything wrong with this box" before "where do I change it".
//!
//! # Where the numbers come from
//!
//! The board, read here, through the traits it implements. Three of the readings were already
//! reachable and are not read a second time in a second way: the chip's temperature is the PMIC's
//! ([`PowerBackend::chip_temperature_c`], the same number the battery page shows, and named for the
//! part that measures it), and the storage ring is the built-in volume's fill from the same
//! [`StorageBackend`] the storage page draws. The rest — the heap, the uptime, the clock, the chip
//! and the image — are [`SystemBackend`]'s.
//!
//! [`PowerBackend::chip_temperature_c`]: pomelo_hal::PowerBackend::chip_temperature_c
//! [`StorageBackend`]: pomelo_hal::StorageBackend
//! [`SystemBackend`]: pomelo_hal::SystemBackend
//!
//! None of it is pushed in the way the battery reading is. A pushed value belongs to whoever owns
//! it, and the status bar owns the battery; nothing owns these but this readout, and a readout that
//! asks the board is a readout that says what the board says. What the platform sends instead is a
//! nudge that a second has passed — [`SystemEvent::Tick`](pomelo_hal::SystemEvent::Tick) — which is
//! the one thing a readout cannot get by asking.
//!
//! # The tick, and why it is cheap
//!
//! [`SystemPanel::refresh`] re-reads the readings that move and a finger's worth of nothing else; it
//! runs once a second while this page is up and never while it is not
//! ([`crate::Settings::refresh_system`]). The identity — the chip and the image — is read once and
//! never again, because neither can change while the box is running: a different firmware is a
//! different boot, and this app does not survive one.
//!
//! # The two facts about time
//!
//! The clock is UTC out of the HAL, because UTC is what the hardware holds, and what a person reads
//! is a local time. [`LOCAL_OFFSET_SECS`] is that conversion, and it is the same one the launcher's
//! status-bar clock makes in `current_time_info` and the same one the firmware's event pump applies
//! before it rolls the minute over. Three readers of one assumption is two too many, and when the
//! HAL grows a clock with a zone on it, this is the copy to delete.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iced::widget::text::Wrapping;
use iced::widget::{container, stack, text, Column, Row, Space};
use iced::{Alignment, Border, Length};
use pomelo_hal::{Board, ChipInfo, FirmwareInfo, MemoryInfo, VolumeKind};
use pomelo_material_symbols::Icon;
use pomelo_widgets::{Language, SystemPreferences, ThemeMode};

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::common::{card_surface, detail_rows, separator, UI};
use crate::pages::ring::{self, Reading};
use crate::style::{self, IconColor};

/// How far the local clock is from UTC, in seconds.
///
/// Eight hours, because this board is built for CST — the same assumption the launcher's clock and
/// the firmware's minute rollover already make. See the module docs.
const LOCAL_OFFSET_SECS: i64 = 8 * 60 * 60;

/// Every second in one.
const SECONDS_PER_HOUR: u64 = 3600;

/// The chip and the image, together.
///
/// Together and not as two options, because they are read for one card and a card that has a model
/// and no version has nothing a reader can do with either half.
struct Identity {
    chip: ChipInfo,
    firmware: FirmwareInfo,
}

/// What the readout draws: the board's own account of itself, and the readings that move.
pub struct SystemPanel {
    /// The chip and the image. Read once: see the module docs.
    identity: Option<Identity>,
    /// The heap, as the board reports it.
    memory: Option<MemoryInfo>,
    /// How full the box's own storage is, in `0.0..=100.0`.
    storage_percent: Option<f32>,
    /// The PMIC's die temperature. The one reading here that is measured by a part doing something
    /// else — it powers the board — which is why it comes from the power backend and not from this
    /// one.
    temperature_c: Option<f32>,
    /// How long the board has been up.
    uptime: Option<Duration>,
    /// The board's clock, or `None` when it has never been set.
    clock: Option<SystemTime>,
}

impl SystemPanel {
    /// The panel, having asked the board for everything it shows.
    pub(crate) fn new(board: &Board) -> Self {
        let mut panel = Self {
            identity: None,
            memory: None,
            storage_percent: None,
            temperature_c: None,
            uptime: None,
            clock: None,
        };

        panel.read_identity(board);
        panel.refresh(board);

        panel
    }

    /// Asks again for the readings that move.
    ///
    /// Every one of them is a reading that can fail on its own — a heap the board cannot measure, a
    /// clock that has never been set, an empty slot where a volume was — and each failure leaves its
    /// own `None` rather than raising: one unanswerable number is not a reason to draw none of them.
    /// The card that draws an empty ring says so in words.
    pub(crate) fn refresh(&mut self, board: &Board) {
        {
            let system = board.system();

            self.memory = system.memory().ok();
            self.uptime = system.uptime().ok();
            self.clock = system.clock().ok().flatten();
        }

        self.temperature_c = board.power().chip_temperature_c().ok();
        self.storage_percent = internal_use(board);

        // An identity read that failed is tried again here rather than left blank for the rest of
        // the session. The two calls cannot fail on this board, so this costs one branch a second
        // and removes a state — "the card is empty and always will be" — that no reader could tell
        // apart from a bug.
        if self.identity.is_none() {
            self.read_identity(board);
        }
    }

    /// Reads the chip and the image, keeping what the last read found if this one fails.
    fn read_identity(&mut self, board: &Board) {
        let system = board.system();

        let (Ok(chip), Ok(firmware)) = (system.chip(), system.firmware()) else {
            return;
        };

        self.identity = Some(Identity { chip, firmware });
    }
}

/// How full the box's own storage is, or `None` if the board could not say.
///
/// The built-in partition and not the card. A card is optional, and a ring that changed what it
/// meant when one arrived — from "this box is this full" to "the card you just put in is this full"
/// — would be a number nobody could compare with the one they saw a minute ago. What the card holds
/// is the storage page's subject, where each volume gets its own bar and its own numbers.
///
/// [`StorageBackend::volumes`] and not a probe: this runs once a second, and the slot has no
/// card-detect line, so asking whether there is a card in it means *mounting* whatever is there.
/// The volumes that are mounted are already known.
fn internal_use(board: &Board) -> Option<f32> {
    board
        .storage()
        .volumes()
        .ok()?
        .iter()
        .find(|volume| volume.kind == VolumeKind::Internal)
        .map(|volume| volume.used_percent())
}

// =============================================================================
// The panel
// =============================================================================

/// The readout: one card, three parts.
///
/// One card and not three, because the three parts are one answer: the gauges say how it is doing,
/// the cards say what it is, and the footer says since when. A reader who has to work out which box
/// belongs to which is reading a form rather than a readout.
pub(crate) fn summary_panel<'a>(preferences: SystemPreferences, panel: &SystemPanel) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;

    let gauges = Row::with_children(vec![
        gauge(
            Icon::MEMORY,
            language.text(Key::MemoryUsed),
            counted(language, panel.memory.map(|memory| memory.used_percent())),
            share(
                panel.memory.map(|memory| memory.used_percent()),
                style::memory_bar(),
            ),
            theme,
        ),
        gauge(
            Icon::DEVICE_THERMOSTAT,
            language.text(Key::ChipTemperature),
            degrees(language, panel.temperature_c),
            // The scale a die temperature is read on: 100 °C fills the ring. Not a share of
            // anything — nothing divides a temperature — and not an invented maximum either: a
            // hundred degrees is where a chip stops being a temperature and starts being a fault.
            share(panel.temperature_c, style::temperature_ring()),
            theme,
        ),
        gauge(
            Icon::STORAGE,
            language.text(Key::StorageUsed),
            counted(language, panel.storage_percent),
            share(panel.storage_percent, style::storage_bar()),
            theme,
        ),
    ])
    .width(Length::Fill)
    .align_y(Alignment::Start);

    let cards = Row::with_children(vec![
        info_card(
            Icon::DEVELOPER_BOARD,
            IconColor::Blue,
            language.text(Key::Model),
            panel
                .identity
                .as_ref()
                .map(|identity| identity.chip.model.clone()),
            language,
            theme,
        ),
        Space::new()
            .width(Length::Fixed(style::SUMMARY_CARD_GAP))
            .into(),
        info_card(
            Icon::INFO,
            IconColor::Green,
            language.text(Key::Firmware),
            // The version and not the project's name as well: on this image the name is whatever
            // CMake was given — `firmware` — and `firmware 6de00de-dirty` is a card whose longest
            // word is the one word that says nothing. [`FirmwareInfo::name`] is there for an image
            // whose name is worth reading.
            panel
                .identity
                .as_ref()
                .map(|identity| identity.firmware.version.clone()),
            language,
            theme,
        ),
    ])
    .width(Length::Fill)
    .height(Length::Fixed(style::SUMMARY_CARD_H));

    // The footer, as the rows every other card in this app is made of: a label on the left and its
    // value on the right. Two rows rather than one line of four — "System time: 2026-10-09
    // 17:36:11" and its neighbour are longer than the panel is, in English, at this size.
    let footer = detail_rows(
        vec![
            (
                language.text(Key::SystemTime),
                local_time(panel.clock, language),
            ),
            (language.text(Key::Uptime), elapsed(panel.uptime, language)),
        ],
        theme,
    );

    let mut children: Vec<UI<'a>> = vec![
        container(gauges).padding(style::SUMMARY_PADDING).into(),
        separator(theme),
        container(cards).padding(style::SUMMARY_PADDING).into(),
        separator(theme),
    ];
    children.extend(footer);

    card_surface(Column::with_children(children).width(Length::Fill), theme)
}

/// One gauge: a ring with its glyph and value in the middle, and its name under it.
///
/// The middle is a `stack` over the canvas because a canvas here cannot draw text — see
/// [`crate::pages::ring`]. That is also what lets the glyph and the number be this app's own text,
/// at this app's font sizes, instead of a rasteriser's idea of a font.
///
/// The glyph is drawn in the ring's own colour, taken from the reading rather than passed beside it:
/// one gauge has one colour, and two arguments that have to agree are two arguments that can stop
/// agreeing.
fn gauge<'a>(
    icon: Icon,
    label: &'static str,
    value: String,
    reading: Reading,
    theme: ThemeMode,
) -> UI<'a> {
    let dial = stack![
        ring::ring(reading, style::bar_track_for(theme)),
        // The layer over the ring is given the whole of it, and centres its own content: a stack
        // sizes itself to its base layer, and a layer smaller than that would sit in a corner.
        //
        // The layer over the ring is given the whole of it and centres its content twice over: the
        // container in the middle of the canvas's box, and the two lines in the middle of their
        // column. A stack is as big as its base layer, so that box is the ring's.
        //
        // The column's own `align_x` is enough for these two because each of them is exactly as
        // wide as it is drawn — a text is measured, never stretched — so centring the two boxes
        // centres the two inks. The argument stops holding the moment a *container* is wider than
        // the text inside it: that is the label under this ring, and it needed a `center_x` of its
        // own. See below.
        container(
            Column::with_children(vec![
                text(icon.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(style::RING_GLYPH)
                    .color(reading.color)
                    .into(),
                text(value)
                    .size(style::RING_VALUE_FONT)
                    .color(style::label_for(theme))
                    .into(),
            ])
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
    ];

    Column::with_children(vec![
        container(dial)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),
        Space::new()
            .height(Length::Fixed(style::RING_LABEL_GAP))
            .into(),
        // Centred by the *container*, and that is the whole of what this line is about.
        //
        // `Text::center` sets the alignment inside a text and leaves the text as wide as itself, so
        // a text whose container is a whole cell wide — which this one has to be, so that a long
        // label wraps inside the cell rather than running into its neighbour's — sits against that
        // container's left edge. The number up in the ring was centred, because its column is only
        // as wide as it is; the word under the ring was not, because this box is wider than the
        // word. That difference is what made the three labels look askew of the rings they belong
        // to, and `center_x` is what closes it.
        container(
            text(label)
                .size(style::RING_LABEL_FONT)
                .color(style::muted_for(theme)),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into(),
    ])
    // A share of the row, and the three cells divide it equally: the gauges are the same thing three
    // times, and three of them at their own widths would be three columns of different sizes.
    .width(Length::Fill)
    .into()
}

/// One of the two cards under the gauges: a tinted tile with a glyph, a name and a value.
///
/// The tint is the entry's own colour over the card's surface — see [`style::tint_for`] — which is
/// what tells the two apart at a glance without either of them being a button. Neither is: there is
/// nothing to press here.
///
/// The name and the value are two lines, the glyph beside the first of them, and that is not
/// decoration: a version is one long word with nowhere to break, and a value sharing its line with
/// a name is a value that gets drawn outside the card it belongs to — which is what the first
/// build of this panel did, with `firmware 6de00de-dirty` hanging off the end of the green card.
/// On its own line it has the whole of it.
fn info_card<'a>(
    icon: Icon,
    tint: IconColor,
    label: &'static str,
    value: Option<String>,
    language: Language,
    theme: ThemeMode,
) -> UI<'a> {
    let heading = Row::with_children(vec![
        text(icon.glyph())
            .font(pomelo_material_symbols::font())
            .size(style::SUMMARY_CARD_GLYPH)
            .color(tint.color())
            .into(),
        Space::new()
            .width(Length::Fixed(style::BADGE_GAP))
            .into(),
        container(
            text(label)
                .size(style::SUMMARY_CARD_LABEL_FONT)
                .color(style::muted_for(theme)),
        )
        .center_y(Length::Fill)
        .into(),
    ])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let words = Column::with_children(vec![
        heading.into(),
        Space::new()
            .height(Length::Fixed(style::SUMMARY_CARD_GAP_V))
            .into(),
        text(value.unwrap_or_else(|| language.text(Key::None).to_string()))
            .size(style::SUMMARY_CARD_VALUE_FONT)
            .color(style::label_for(theme))
            // Glyph wrapping and not word wrapping: a version has no words in it, so word wrapping
            // would leave it one long piece and let it hang out of the card rather than break it.
            .wrapping(Wrapping::Glyph)
            .into(),
    ])
    .width(Length::Fill);

    container(container(words).padding(style::SUMMARY_CARD_PADDING))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(style::tint_for(tint, theme).into()),
            border: Border {
                radius: style::CARD_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

// =============================================================================
// What a ring says
// =============================================================================

/// A percentage as a ring shows it: no decimal point.
///
/// A gauge is a shape at a glance, and the fraction of a percent a ring cannot show is a digit the
/// eye skips. Where the precise number matters — the heap in bytes, the volume's own figures — the
/// page behind this one has it.
fn counted(language: Language, percent: Option<f32>) -> String {
    match percent {
        Some(percent) => format!("{percent:.0}%"),
        None => language.text(Key::None).to_string(),
    }
}

/// A temperature as a ring shows it.
///
/// Degrees the way the battery page writes them — a space and a `C`, not a `°`: this board's font is
/// a Chinese and Latin subset with no degree sign in it, which is why the battery page spells it
/// this way too. Rounded to a whole degree, for the reason [`counted`] gives.
fn degrees(language: Language, celsius: Option<f32>) -> String {
    match celsius {
        Some(celsius) => format!("{celsius:.0} C"),
        None => language.text(Key::None).to_string(),
    }
}

/// A percentage as an arc, in `color`.
///
/// No reading is an *empty* ring rather than a full one. `None` is not zero — a board that could not
/// measure its heap has not used none of it — so the ring says nothing and the words in the middle
/// say why.
fn share(percent: Option<f32>, color: iced::Color) -> Reading {
    Reading {
        fraction: percent.unwrap_or(0.0) / 100.0,
        color,
    }
}

// =============================================================================
// Time
// =============================================================================

/// The clock as a person reads it: `2026-10-09 17:36:11`, or a word when there is no clock.
fn local_time(clock: Option<SystemTime>, language: Language) -> String {
    let Some(seconds) = clock
        .and_then(|clock| clock.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs() as i64)
    else {
        return language.text(Key::None).to_string();
    };

    let local = seconds + LOCAL_OFFSET_SECS;
    // `div_euclid`/`rem_euclid` and not `/`/`%`: a time before 1970 is a negative count of seconds,
    // and truncating division would put it on the wrong day — a board whose clock came up wrong is
    // exactly the board that must not also be drawn a day out.
    let (year, month, day) = civil(local.div_euclid(86_400));
    let time = local.rem_euclid(86_400);

    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        time / SECONDS_PER_HOUR as i64,
        (time % SECONDS_PER_HOUR as i64) / 60,
        time % 60,
    )
}

/// The civil date a count of days from 1970-01-01 falls on.
///
/// Howard Hinnant's `civil_from_days` — the arithmetic every datetime library is, written out
/// because this crate has no datetime library and does not need one for a single line of a single
/// card. The year is shifted to start in March so that the leap day lands at its end, which is what
/// makes the day-of-year to month-and-day step a division rather than a table.
fn civil(days: i64) -> (i64, i64, i64) {
    /// 1970-01-01 in the shifted calendar: the epoch is 719,468 days after 0000-03-01.
    const EPOCH_SHIFT: i64 = 719_468;
    /// One era is 400 years: the cycle the leap rule repeats on.
    const ERA_DAYS: i64 = 146_097;

    let shifted = days + EPOCH_SHIFT;
    let era = shifted.div_euclid(ERA_DAYS);
    let of_era = shifted.rem_euclid(ERA_DAYS);
    let year_of_era =
        (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;

    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    // Back to a year that starts in January: March and everything after it is this year, January
    // and February belong to the one after.
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };

    let year = year_of_era + era * 400 + i64::from(month <= 2);

    (year, month, day)
}

/// How long the board has been up, in the units a person says it in.
///
/// Hours are not carried into days: a board up for two days says "48小时" and means it. A day is a
/// calendar word, and which day it is depends on when the box was switched on rather than on how
/// long it has run.
fn elapsed(uptime: Option<Duration>, language: Language) -> String {
    let Some(seconds) = uptime.map(|uptime| uptime.as_secs()) else {
        return language.text(Key::None).to_string();
    };

    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = (seconds % SECONDS_PER_HOUR) / 60;
    let seconds = seconds % 60;

    // The units are interface words, so they are translated; the numbers and their order are the
    // same in both, which is why this is a `format!` and not a table in `i18n`.
    match language {
        Language::Chinese => format!("{hours}小时{minutes}分{seconds}秒"),
        Language::English => format!("{hours}h {minutes}m {seconds}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one conversion this file does that is not addition: days into a date.
    ///
    /// The three dates worth checking are the epoch itself, a leap day, and a day in a century that
    /// is not a leap year — the rule that a `% 4` gets wrong by one every hundred years.
    #[test]
    fn a_count_of_days_is_the_date_it_falls_on() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(1), (1970, 1, 2));
        assert_eq!(civil(-1), (1969, 12, 31));

        // 2000-02-29: the leap day of a year divisible by 400.
        assert_eq!(civil(11_016), (2000, 2, 29));
        // 1900-03-01: the day after a February that a `% 4` rule would have given 29 days.
        assert_eq!(civil(-25_508), (1900, 3, 1));
        // 2026-10-09, the day this was written.
        assert_eq!(civil(20_735), (2026, 10, 9));
    }

    /// A clock is read in local time, and a board without one says so rather than drawing 1970.
    #[test]
    fn the_clock_is_local_and_its_absence_is_a_word() {
        // 2025-10-09T08:53:20Z, which is 16:53:20 eight hours east of it.
        let afternoon_utc = UNIX_EPOCH + Duration::from_secs(1_760_000_000);

        assert_eq!(
            local_time(Some(afternoon_utc), Language::English),
            "2025-10-09 16:53:20",
            "UTC+8, and seconds and all"
        );

        assert_eq!(
            local_time(None, Language::English),
            "none",
            "a board that has never been told the time is not a board at the epoch"
        );
        assert_eq!(local_time(None, Language::Chinese), "无");
    }

    /// Uptime is counted in the units a person says it in, and does not roll into days.
    #[test]
    fn an_uptime_is_counted_in_hours() {
        let long = Duration::from_secs(2 * 24 * 3_600 + 32 * 60 + 57);

        assert_eq!(elapsed(Some(long), Language::Chinese), "48小时32分57秒");
        assert_eq!(elapsed(Some(long), Language::English), "48h 32m 57s");
        assert_eq!(elapsed(Some(Duration::ZERO), Language::English), "0h 0m 0s");
        assert_eq!(elapsed(None, Language::English), "none");
    }

    /// A ring with no reading is empty, and its words say why.
    #[test]
    fn a_reading_that_is_missing_is_not_a_reading_of_zero() {
        assert_eq!(counted(Language::English, None), "none");
        assert_eq!(counted(Language::English, Some(21.7)), "22%");
        assert_eq!(degrees(Language::English, Some(31.4)), "31 C");
        assert_eq!(degrees(Language::Chinese, None), "无");
    }
}
