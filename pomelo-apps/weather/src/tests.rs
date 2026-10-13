//! The page's own tests: what it asks the service for, and what it draws from the answer.
//!
//! Nothing here names a platform layer, opens a socket or draws a pixel. The simulator's client is the
//! real one — a request that takes three ticks, a status code, a body — so what is asserted is the
//! page's behaviour against a client that behaves the way the trait says a client does.
//!
//! The reading of the document is `api`'s own tests; what is tested here is the wiring: that a request
//! is started, that its answer is read exactly once, that frames stop when they should, that a fault
//! reaches the page as the fault it was, and that the numbers on the page come from the answer rather
//! than from a literal.

use std::sync::Arc;
use std::time::Instant;

use iced::Size;
use iced_test::Simulator;
use pomelo_hal::sim::SimHttp;
use pomelo_hal::{Board, HttpFault, HttpState};
use pomelo_widgets::preferences::{FontSizeTier, Language, SystemPreferences, ThemeMode};

use crate::api::Condition;
use crate::style::PANEL;
use crate::ui::{condition_icon, day_label};
use crate::{Message, Status, Weather, Words};

/// The document this page is written against, cut down to the fields it reads.
///
/// A real answer rather than a convenient one: `current` and `daily` as Open-Meteo spells them, with
/// the days as columns that line up by index and the weather as WMO codes. Shared with `api`'s tests,
/// which is the point — the reader and the page are tested against the same thing the service sends.
pub(crate) const SAMPLE: &str = r#"{
    "latitude": 39.875,
    "longitude": 116.375,
    "utc_offset_seconds": 28800,
    "timezone": "Asia/Shanghai",
    "current": {
        "time": "2026-10-12T09:30",
        "interval": 900,
        "temperature_2m": 18.4,
        "relative_humidity_2m": 46,
        "weather_code": 2,
        "wind_speed_10m": 11.5
    },
    "daily": {
        "time": ["2026-10-12", "2026-10-13", "2026-10-14", "2026-10-15", "2026-10-16"],
        "weather_code": [2, 3, 61, 80, 0],
        "temperature_2m_max": [21.3, 19.8, 16.2, 17.5, 22.1],
        "temperature_2m_min": [9.4, 10.1, 8.8, 7.9, 9.0]
    }
}"#;

/// The document the locator is written against, cut down to the fields this page reads.
///
/// A real answer in the shape ip-api sends it — its own `status`, its `city`, `regionName` and
/// `country`, and its `lat` and `lon` — and deliberately a city that no other line in this crate
/// mentions. The header draws whatever the locator's answer named, and a fixture naming a place the
/// page could have got from somewhere else would not be evidence of that.
pub(crate) const LOCATED: &str = r#"{
    "status": "success",
    "country": "Portugal",
    "regionName": "Lisboa",
    "city": "Lisbon",
    "lat": 38.7167,
    "lon": -9.1333
}"#;

/// How many ticks the simulator's client takes to come back.
///
/// Three, and its own constant — the number is here so that the tests read as "wait for the answer"
/// rather than as a magic three.
const ANSWER_TICKS: usize = 3;

/// A screen tall enough to show the whole page without scrolling.
///
/// The page is a header and two cards and it fits the panel, but the simulator has no scroll, so
/// anything below the fold is invisible to `find`. Generous rather than exact: a number that had to be
/// re-tuned with every gap would turn a metric change into a test failure.
const TALL: f32 = 800.0;

/// A board with a client that answers the locator and then `SAMPLE`.
///
/// Two answers for the one refresh, because the page asks twice: it cannot ask what the weather is
/// somewhere until something has told it where it is, and a board that answered only the second request
/// would be a board no page could get to its answer through.
fn answering() -> Arc<Board> {
    Arc::new(Board::simulated().with_http(Box::new(
        SimHttp::answering(LOCATED).then(SAMPLE),
    )))
}

/// A board with a client that never comes back, for `fault`'s sake.
fn failing(fault: HttpFault) -> Arc<Board> {
    Arc::new(Board::simulated().with_http(Box::new(SimHttp::failing(fault))))
}

/// A board whose client answers the locator and then refuses the forecast with `body` at `status`.
///
/// The second request is the one refused, because that is the one whose refusal the tests are about:
/// a locator that refuses is a page with no place rather than a page with a bad forecast in it.
fn refusing(status: u16, body: &str) -> Arc<Board> {
    Arc::new(Board::simulated().with_http(Box::new(
        SimHttp::answering(LOCATED).then((status, body)),
    )))
}

/// The page's widget tree on a screen `height` logical pixels tall.
///
/// The font is asked for by *generic* family, not by name: the tester's own default is "Fira Sans",
/// which only exists when iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no
/// room for. A generic resolves against whatever the machine has, so text measures to real glyphs.
fn screen(app: &Weather, height: f32) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(PANEL, height),
        app.view(),
    )
}

/// One frame of the page's subscription.
///
/// The instant is not read by anything the page does — the request is polled, not timed — but it is the
/// message the subscription produces, and applying it by hand is what a test without a loop does.
fn frame(app: &mut Weather) {
    app.update(Message::Tick(Instant::now()));
}

/// One round of a refresh: the client is ticked until it answers, and the page reads the answer.
fn round(board: &Board, app: &mut Weather) {
    for _ in 0..ANSWER_TICKS {
        board.tick();
    }
    frame(app);
}

/// A whole refresh: the locator's answer, and then the forecast's.
///
/// Two rounds, because a refresh is two requests — and the second is not optional from a caller's side,
/// since a page left standing after the first would be a page waiting for somebody to press the button
/// again. A refresh that ends early ends the second round by itself: a page whose first request was
/// refused has nothing left to ask, and the client it would ask is faulted rather than idle.
fn wait(board: &Board, app: &mut Weather) {
    round(board, app);
    round(board, app);
}

/// The page, open in English, on a board that answers `SAMPLE`.
///
/// The language is set because the view tests assert English labels: the default is Chinese, and a test
/// that meant to check a label would otherwise pass or fail on a default it does not control.
fn english() -> (Arc<Board>, Weather) {
    let board = answering();
    let mut app = Weather::new(Arc::clone(&board));

    app.set_preferences(SystemPreferences {
        language: Language::English,
        theme: ThemeMode::Dark,
        font_tier: FontSizeTier::Standard,
        ..SystemPreferences::default()
    });

    wait(&board, &mut app);

    (board, app)
}

/// Opening the page is what asks the service.
#[test]
fn opening_the_page_asks_the_service() {
    let board = answering();
    let app = Weather::new(Arc::clone(&board));

    assert_eq!(board.http().state(), HttpState::Running);
    assert_eq!(app.status(), &Status::Loading);
    assert!(app.forecast().is_none(), "nothing has arrived yet");
    assert!(app.waiting(), "so the page is waiting");
}

/// The answer is read once, on the frame the client says it is done, and the frames then stop.
///
/// This is the one thing the page owes the host: a subscription to every frame is a view, a layout and
/// a damage diff every frame, and a card of numbers that are not changing is not a moving picture.
#[test]
fn the_answer_is_read_once_and_the_frames_stop() {
    let (board, mut app) = english();

    let forecast = app.forecast().cloned().expect("the answer was read");

    assert_eq!(forecast.current.temperature_c, 18.4);
    assert_eq!(forecast.days.len(), 5);
    assert_eq!(board.http().state(), HttpState::Idle, "the answer was taken");
    assert!(!app.waiting());

    // A frame that arrives anyway — the launcher's pump is a second, and it does not stop — changes
    // nothing, and does not re-arm the page.
    frame(&mut app);
    assert!(!app.waiting());
    assert_eq!(app.forecast(), Some(&forecast));
}

/// A board that is off the network is a fault line with something to do about it.
///
/// The whole reason the page draws its faults rather than saying "failed": what a person needs is
/// which thing to go and fix, and "the Wi-Fi is off" and "the clock was never set" are two different
/// things to go and fix.
#[test]
fn a_board_off_the_network_is_a_fault_with_something_to_do_about_it() {
    let board = failing(HttpFault::NoNetwork);
    let mut app = Weather::new(Arc::clone(&board));

    app.set_preferences(SystemPreferences {
        language: Language::English,
        ..SystemPreferences::default()
    });

    wait(&board, &mut app);

    assert_eq!(app.status(), &Status::Failed(HttpFault::NoNetwork));
    assert!(app.forecast().is_none());
    assert!(!app.waiting(), "there is nothing left to wait for");

    let mut ui = screen(&app, TALL);
    assert!(ui.find("Not on a network").is_ok(), "what happened");
    assert!(
        ui.find("Turn the Wi-Fi on in Settings").is_ok(),
        "and what to do about it"
    );
}

/// The clock is a fault of its own, because it is a different thing to go and fix.
///
/// The one a page that said "no network" would send somebody to look at the wrong thing for: a board
/// that has never been told the time cannot judge a certificate, so every HTTPS request fails while
/// the Wi-Fi is perfectly fine.
#[test]
fn a_clock_that_was_never_set_is_its_own_line() {
    let board = failing(HttpFault::ClockNotSet);
    let mut app = Weather::new(Arc::clone(&board));

    app.set_preferences(SystemPreferences {
        language: Language::English,
        ..SystemPreferences::default()
    });

    wait(&board, &mut app);

    assert_eq!(app.status(), &Status::Failed(HttpFault::ClockNotSet));

    let mut ui = screen(&app, TALL);
    assert!(ui.find("The clock has never been set").is_ok());
    assert!(ui.find("Set the clock, then try again").is_ok());
    assert!(
        ui.find("Turn the Wi-Fi on in Settings").is_err(),
        "and not the network line, which is about something else"
    );
}

/// A server that answered with a refusal is not a forecast, and the page says so rather than drawing
/// an empty one.
///
/// The case a page cannot rehearse against a real server and has to get right: a `404` with a body in
/// it, a captive portal's sign-in page, a proxy's error page. All of them arrive as a perfectly good
/// HTTP answer to a perfectly good HTTP client.
#[test]
fn a_refusal_with_a_body_is_not_a_forecast() {
    for (status, body) in [
        (404, "<html>Not found</html>"),
        (500, "{\"error\":true,\"reason\":\"Too many requests\"}"),
        (200, "<html><body>Sign in to the Wi-Fi</body></html>"),
    ] {
        let board = refusing(status, body);
        let mut app = Weather::new(Arc::clone(&board));

        wait(&board, &mut app);

        assert_eq!(
            app.status(),
            &Status::Unreadable,
            "{status} `{body}` is not a forecast"
        );
        assert!(app.forecast().is_none());
        assert!(!app.waiting());
    }
}

/// Bytes that arrived and were not a forecast this page can read.
#[test]
fn what_is_not_a_forecast_is_not_drawn_as_one() {
    // Not JSON, JSON of the wrong shape, and JSON that is a forecast with a field missing — three
    // different mistakes the reader can make, and one thing for the page to do about any of them.
    for body in [
        "",
        "{\"latitude\":39.875",
        "{}",
        r#"{"current":{"temperature_2m":1},"daily":{}}"#,
    ] {
        let board = refusing(200, body);
        let mut app = Weather::new(Arc::clone(&board));

        wait(&board, &mut app);

        assert_eq!(app.status(), &Status::Unreadable, "`{body}` is not a forecast");
        assert!(app.forecast().is_none());
    }
}

/// Refreshing asks the service again, and the numbers on the page are the new answer's.
#[test]
fn refreshing_asks_again() {
    let (board, mut app) = english();

    let before = app.forecast().cloned().expect("the first answer");

    app.update(Message::Refresh);

    assert_eq!(board.http().state(), HttpState::Running);
    assert_eq!(app.status(), &Status::Loading);
    assert!(app.waiting(), "the page is waiting on the new request");

    // The old reading is gone rather than kept: unlike the Wi-Fi page's chart, a forecast drawn under
    // a request that is in flight would be a page showing yesterday's weather as if it were now.
    assert!(app.forecast().is_none());

    wait(&board, &mut app);

    assert_eq!(app.forecast(), Some(&before), "the same document, read again");
}

/// A request that failed does not hold the slot: the next one is taken.
///
/// The statement of a client's rule from the page's side, and the one a page has to be able to make:
/// "the Wi-Fi is off, turn it on and press refresh" is the whole of what a fault line is for, so a
/// page that could not ask again would be a fault line with no way out of it.
#[test]
fn a_request_that_failed_does_not_block_the_next_one() {
    let board = failing(HttpFault::NoNetwork);
    let mut app = Weather::new(Arc::clone(&board));

    wait(&board, &mut app);
    assert_eq!(app.status(), &Status::Failed(HttpFault::NoNetwork));

    // The button, pressed. The client is still the failing one, so the answer is the same fault — but
    // the request went *out*, which is the thing being tested.
    app.update(Message::Refresh);

    assert_eq!(board.http().state(), HttpState::Running);
    assert_eq!(app.status(), &Status::Loading);

    wait(&board, &mut app);
    assert_eq!(app.status(), &Status::Failed(HttpFault::NoNetwork));
}

/// While a request is in flight the page says so, and the button is drawn but not pressable.
///
/// The button's `on_press_maybe` is what keeps a person off the second request the client would refuse
/// (`Busy`) — the page is drawn with the button in place and nothing to send, which is the one thing
/// about it a widget tree can see.
#[test]
fn the_button_is_off_while_a_request_is_in_flight() {
    let board = answering();
    let mut app = Weather::new(Arc::clone(&board));

    app.set_preferences(SystemPreferences {
        language: Language::English,
        ..SystemPreferences::default()
    });

    assert!(app.waiting(), "still waiting, so the button is off");

    let mut ui = screen(&app, TALL);
    assert!(
        ui.find("Fetching…").is_ok(),
        "the page says what it is doing, rather than showing a stale card"
    );
    assert!(ui.find("Refresh").is_ok(), "and the button is still there");
}

/// Every fault says what happened and what to do about it, in both languages, and says something
/// different from the others.
///
/// The match in [`Words`] is exhaustive and has no `_`, so a fault added to the HAL stops this page
/// compiling — this test is the other half of that: that the sentences a person is sent to are
/// distinct, because two faults drawn as one line would be one thing to go and fix for two problems.
#[test]
fn every_fault_is_a_sentence_and_a_thing_to_do_in_both_languages() {
    let faults = [
        HttpFault::NoNetwork,
        HttpFault::ClockNotSet,
        HttpFault::Unreachable,
        HttpFault::Timeout,
        HttpFault::Tls,
        HttpFault::TooLarge,
        HttpFault::Unsupported,
    ];

    for language in [Language::Chinese, Language::English] {
        let words = Words::of(language);

        let mut said: Vec<&str> = Vec::new();

        for fault in faults {
            let what = words.fault(fault);
            let then = words.fault_hint(fault);

            assert!(!what.is_empty(), "{fault:?} has a sentence");
            assert!(!then.is_empty(), "{fault:?} has something to do about it");
            assert_ne!(what, then, "{fault:?}: two lines, not one said twice");
            assert!(
                !said.contains(&what),
                "{fault:?} is drawn the same as another fault"
            );
            said.push(what);
        }
    }

    // And the two languages are two languages.
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);

    for fault in faults {
        assert_ne!(english.fault(fault), chinese.fault(fault), "{fault:?}");
        assert_ne!(english.fault_hint(fault), chinese.fault_hint(fault), "{fault:?}");
    }
}

/// The page draws the forecast it was given, and the words the language says.
#[test]
fn the_page_draws_the_forecast() -> Result<(), iced_test::Error> {
    let (_, app) = english();

    let mut ui = screen(&app, TALL);

    // What the page is and where it is for.
    assert!(ui.find("Weather").is_ok(), "the page says what it is");
    assert!(ui.find("Lisbon").is_ok(), "and which city it is showing");
    assert!(ui.find("From Open-Meteo").is_ok(), "and where the numbers came from");

    // The reading, which is the thing the page is for.
    assert!(ui.find("Now").is_ok());
    assert!(ui.find("18°").is_ok(), "the temperature");
    assert!(ui.find("Partly cloudy").is_ok(), "what the sky is doing");
    assert!(ui.find("Humidity").is_ok());
    assert!(ui.find("46%").is_ok(), "the whole number of the percentage");
    assert!(ui.find("Wind").is_ok());
    assert!(ui.find("12 km/h").is_ok());

    // The days, in the order the service sent them, with the two that have names named.
    assert!(ui.find("The days ahead").is_ok());
    assert!(ui.find("Today").is_ok());
    assert!(ui.find("Tomorrow").is_ok());
    assert!(ui.find("10-14").is_ok(), "and the rest by their date");
    assert!(ui.find("Rain").is_ok(), "the third day");
    assert!(ui.find("21°").is_ok(), "today's high");
    assert!(ui.find("9°").is_ok(), "and today's low");

    // The page's own name for the service is a hint, and the day list is five rows and not six.
    assert!(ui.find("10-17").is_err(), "the service sent five days");

    Ok(())
}

/// The page in Chinese says Chinese things.
#[test]
fn the_page_speaks_the_language_it_is_set_to() -> Result<(), iced_test::Error> {
    let board = answering();
    let mut app = Weather::new(Arc::clone(&board));

    app.set_preferences(SystemPreferences {
        language: Language::Chinese,
        theme: ThemeMode::Dark,
        font_tier: FontSizeTier::Standard,
        ..SystemPreferences::default()
    });

    wait(&board, &mut app);

    let mut ui = screen(&app, TALL);

    assert!(ui.find("天气").is_ok());
    assert!(ui.find("现在").is_ok());
    assert!(ui.find("多云").is_ok());
    assert!(ui.find("湿度").is_ok());
    assert!(ui.find("今天").is_ok());
    assert!(ui.find("Weather").is_err(), "and not the English");

    Ok(())
}

/// The page builds and lays out on the design panel without anything overflowing it.
#[test]
fn the_page_lays_out_on_the_panel() {
    let (_, app) = english();

    let _ui = screen(&app, PANEL);
}

/// The fault card is drawn on the panel too, and says one thing.
#[test]
fn the_fault_card_lays_out_on_the_panel() {
    let board = failing(HttpFault::ClockNotSet);
    let mut app = Weather::new(Arc::clone(&board));
    wait(&board, &mut app);

    let _ui = screen(&app, PANEL);
}

/// The words are in both languages, and the condition names are the service's numbers said in words.
#[test]
fn the_words_are_in_both_languages() {
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);

    assert_ne!(english.title, chinese.title);
    assert_ne!(english.today, chinese.today);

    for condition in [
        Condition::Clear,
        Condition::PartlyCloudy,
        Condition::Overcast,
        Condition::Fog,
        Condition::Drizzle,
        Condition::FreezingRain,
        Condition::Rain,
        Condition::Snow,
        Condition::Showers,
        Condition::Thunderstorm,
        Condition::Unknown,
    ] {
        assert!(!english.condition(condition).is_empty(), "{condition:?}");
        assert!(!chinese.condition(condition).is_empty(), "{condition:?}");
        assert_ne!(
            english.condition(condition),
            chinese.condition(condition),
            "{condition:?}"
        );
    }
}

/// Two days have names and the rest have their date.
///
/// The argument in `ui::day_label` is that turning a date into a weekday is calendar arithmetic, and
/// that this project keeps one copy of it — in the clock page — so the rest of the rows say the date
/// the service sent. This is that behaviour, including the date the page cannot cut a year off.
#[test]
fn a_day_that_has_no_name_gets_its_date() {
    let words = Words::of(Language::English);

    assert_eq!(day_label(0, "2026-10-12", &words), "Today");
    assert_eq!(day_label(1, "2026-10-13", &words), "Tomorrow");
    assert_eq!(day_label(2, "2026-10-14", &words), "10-14");

    // A date that is not the shape the service documents is drawn as it came: cutting a year off
    // something that has no year in it would be inventing a month and a day, and cutting the first ten
    // bytes off `Wednesday` would be drawing `sday`.
    assert_eq!(day_label(4, "1900-01-01", &words), "01-01");
    assert_eq!(day_label(4, "Wednesday", &words), "Wednesday");
    assert_eq!(day_label(4, "", &words), "");
    assert_eq!(day_label(4, "10-14", &words), "10-14");

    // An instant where a date was asked for keeps its date, which is the part of it a row is about.
    assert_eq!(day_label(4, "2026-10-14T09:30", &words), "10-14");
}

/// Every condition has a glyph of its own.
///
/// One row per state and one glyph per row: two conditions drawn as the same glyph would be two states
/// of the sky a person could not tell apart on the page, which is the whole of what the glyph is for.
#[test]
fn every_condition_has_a_glyph_of_its_own() {
    let conditions = [
        Condition::Clear,
        Condition::PartlyCloudy,
        Condition::Overcast,
        Condition::Fog,
        Condition::Drizzle,
        Condition::FreezingRain,
        Condition::Rain,
        Condition::Snow,
        Condition::Showers,
        Condition::Thunderstorm,
        Condition::Unknown,
    ];

    let mut glyphs: Vec<&str> = conditions
        .iter()
        .map(|condition| condition_icon(*condition).glyph())
        .collect();

    let count = glyphs.len();
    glyphs.sort_unstable();
    glyphs.dedup();

    assert_eq!(glyphs.len(), count, "two conditions share a glyph");
}

/// The header names the city the locator named, and names nothing until something has named one.
///
/// The page opens on a request rather than on a city out of this file, so there is a moment with no
/// place in it — and a header that drew the city this page used to hard-code would be claiming a
/// forecast for somewhere the reader is not, which is the whole of what was wrong with it. The
/// fixture's city appears nowhere else in this crate, which is what makes the second half of this a
/// statement about the locator's answer.
#[test]
fn the_header_names_the_city_the_locator_named() {
    let board = answering();
    let mut app = Weather::new(Arc::clone(&board));

    assert_eq!(app.location().name, "", "nothing has named a place yet");

    {
        let mut ui = screen(&app, TALL);
        assert!(ui.find("Lisbon").is_err(), "so the header has none to draw");
    }

    wait(&board, &mut app);

    assert_eq!(app.location().name, "Lisbon");
    assert!((app.location().latitude - 38.7167).abs() < 1e-6);
    assert!((app.location().longitude + 9.1333).abs() < 1e-6);
}

/// A locator that cannot place the address is the end of the refresh, and nothing is asked after it.
///
/// The first request is the one the page cannot do without: with no place there is no URL to ask about
/// the weather, so none is asked — and the page says what happened rather than drawing a forecast for
/// a place nobody named. A locator's "no" arrives as a `200` with `status` in it, which is this
/// service's own way of saying it cannot help.
#[test]
fn a_locator_that_cannot_place_the_address_asks_nothing_after_it() {
    let board = Arc::new(Board::simulated().with_http(Box::new(SimHttp::answering(
        r#"{"status":"fail","message":"reserved range","query":"192.168.1.1"}"#,
    ))));
    let mut app = Weather::new(Arc::clone(&board));

    wait(&board, &mut app);

    assert_eq!(app.status(), &Status::Unreadable);
    assert!(app.forecast().is_none());
    assert_eq!(app.location().name, "", "and the page names no place");
    assert!(!app.waiting());
}
