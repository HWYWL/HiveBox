//! The page's own tests: what it reads from the radio, and what it draws from that.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. The simulator's radio is the
//! real one — five access points, a scan that takes three ticks — so what is asserted is the page's
//! behaviour against a radio that behaves the way the trait says a radio does.
//!
//! The arithmetic of the chart is `channel`'s own tests; what is tested here is the wiring: that a scan
//! is started, that its results are read exactly once, that frames stop when they should, and that the
//! numbers on the page come from the scan rather than from a literal.

use std::sync::Arc;
use std::time::Instant;

use iced::Size;
use iced_test::Simulator;
use pomelo_hal::{Board, ScanState};
use pomelo_widgets::preferences::{FontSizeTier, Language, SystemPreferences, ThemeMode};

use crate::channel::{self, CLEAN, FIRST, LAST};
use crate::{Message, WifiMap, Words};

/// The design panel, which is what the layout is drawn for.
const PANEL: f32 = 480.0;

/// A screen tall enough to show the whole page without scrolling.
///
/// The page is a chart, a map and a list and it is taller than the panel; the simulator has no scroll,
/// so anything below the fold is invisible to `find`. Generous rather than exact: this only has to be
/// taller than the page, and a number that had to be re-tuned with every gap would turn a metric change
/// into a test failure.
const TALL: f32 = 1400.0;

/// A board with no hardware behind it: the HAL's simulator.
fn board() -> Arc<Board> {
    Arc::new(Board::simulated())
}

/// The page's widget tree on a screen `height` logical pixels tall.
///
/// The font is asked for by *generic* family, not by name: the tester's own default is "Fira Sans",
/// which only exists when iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no room
/// for. A generic resolves against whatever the machine has, so text measures to real glyphs.
fn screen(app: &WifiMap, height: f32) -> Simulator<'_, Message> {
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
/// The instant is not read by anything the page does — the scan is polled, not timed — but it is the
/// message the subscription produces, and applying it by hand is what a test without a loop does.
fn frame(app: &mut WifiMap) {
    app.update(Message::Tick(Instant::now()));
}

/// The page, open, with a finished scan behind it.
fn scanned() -> (Arc<Board>, WifiMap) {
    let board = board();
    let mut app = WifiMap::new(Arc::clone(&board));

    for _ in 0..3 {
        board.tick();
    }
    frame(&mut app);

    (board, app)
}

/// The page, open in English, with a finished scan behind it.
///
/// The language is set because the view test asserts English labels: the default is Chinese, and a test
/// that meant to check a label would otherwise pass or fail on a default it does not control.
fn english() -> (Arc<Board>, WifiMap) {
    let board = board();
    let mut app = WifiMap::new(Arc::clone(&board));

    app.set_preferences(SystemPreferences {
        language: Language::English,
        theme: ThemeMode::Dark,
        font_tier: FontSizeTier::Standard,
        ..SystemPreferences::default()
    });

    for _ in 0..3 {
        board.tick();
    }
    frame(&mut app);

    (board, app)
}

/// Opening the page is what asks the radio — and the page says what the radio says, not a guess.
#[test]
fn opening_the_page_starts_a_scan() {
    let board = board();
    let app = WifiMap::new(Arc::clone(&board));

    assert_eq!(board.wifi().scan_state(), ScanState::Scanning);
    assert_eq!(app.state(), ScanState::Scanning);
    assert!(app.aps().is_empty(), "nothing has arrived yet");
    assert_eq!(app.spectrum().networks(), 0);
    assert!(app.waiting(), "so the page is waiting");
}

/// The scan is read once, on the frame the radio says it is done.
#[test]
fn the_scan_is_read_when_the_radio_is_done() {
    let board = board();
    let mut app = WifiMap::new(Arc::clone(&board));

    frame(&mut app);
    assert!(
        app.aps().is_empty(),
        "the simulator needs three ticks before it has anything"
    );

    for _ in 0..3 {
        board.tick();
    }
    frame(&mut app);

    assert_eq!(app.aps().len(), 5);
    assert_eq!(app.spectrum().networks(), 5);
    assert_eq!(app.state(), ScanState::Done);
    assert!(!app.waiting(), "there is nothing left to wait for");
}

/// The frames stop the moment the page has what it was waiting for.
///
/// This is the one thing the page owes the host: a subscription to every frame is a view, a layout and
/// a damage diff every frame, and a chart of a scan that finished is not a moving picture.
#[test]
fn the_frames_stop_once_the_scan_has_been_read() {
    let (board, mut app) = scanned();

    assert!(!app.waiting());

    // A frame that arrives anyway — the launcher's pump is a second, and it does not stop — changes
    // nothing, and does not re-arm the page.
    frame(&mut app);
    assert!(!app.waiting());
    assert_eq!(board.wifi().scan_state(), ScanState::Done);
}

/// The suggestion is one of the band's three clean channels.
///
/// The claim the page makes, and the only one it may make: not "this channel is empty", but "of the
/// channels that do not overlap, this is the least crowded".
#[test]
fn the_recommendation_is_one_of_the_three_clean_channels() {
    let (_, app) = scanned();

    let recommended = app.spectrum().recommended();

    assert!(
        CLEAN.contains(&recommended),
        "{recommended} is not one of 1, 6 and 11"
    );
}

/// The air here is five networks, two of which the chart has no column for.
#[test]
fn the_air_here_is_five_networks_and_two_of_them_out_of_band() {
    let (_, app) = scanned();

    assert_eq!(app.spectrum().bins().len(), 13, "the whole band, 1 to 13");
    assert_eq!(app.spectrum().others().len(), 2, "5 GHz is listed, not charted");
    assert_eq!(app.spectrum().networks(), 5);
    assert_eq!(
        app.spectrum()
            .others()
            .iter()
            .map(|ap| ap.channel)
            .collect::<Vec<_>>(),
        vec![36, 44],
        "and in the order the scan reported them"
    );

    // Nothing out of band leaked into a column.
    assert!(
        app.spectrum()
            .others()
            .iter()
            .all(|ap| !channel::in_band(ap.channel))
    );
}

/// Every network that is *in* the band is written on the page, by name.
#[test]
fn the_list_is_written_from_the_scan() {
    let (_, app) = english();

    let found = app.spectrum().loudest(app.aps());
    let names = found.iter().map(|ap| ap.ssid.as_str()).collect::<Vec<_>>();

    assert_eq!(names, vec!["Pomelo-OS", "HomeNet_2.4G", "OpenGuest"]);
}

/// Rescanning asks the radio again, and the numbers on the page are the new scan's.
#[test]
fn rescanning_asks_the_radio_again() {
    let (board, mut app) = scanned();

    app.update(Message::Rescan);

    assert_eq!(board.wifi().scan_state(), ScanState::Scanning);
    assert_eq!(app.state(), ScanState::Scanning);
    assert!(app.waiting(), "the page is waiting on the new scan");

    // The old drawing is still up while the new one is in flight: clearing it would be the page going
    // blank for a second and a half every time somebody pressed the button.
    assert_eq!(app.spectrum().networks(), 5);
}

/// A radio that is off is a line, not a dialog.
#[test]
fn a_radio_that_is_off_is_said_so_in_one_line() {
    let board = board();
    board.wifi().set_enabled(false).expect("the simulator obeys");

    let app = WifiMap::new(Arc::clone(&board));

    assert_eq!(app.state(), ScanState::Error);
    assert!(!app.waiting(), "there is nothing to wait for");
    assert!(app.aps().is_empty());
}

/// The page builds and lays out on the design panel without anything overflowing it.
#[test]
fn the_page_lays_out_on_the_panel() {
    let (_, app) = english();

    let _ui = screen(&app, PANEL);
}

/// The page draws what the scan found, and the words the language says.
///
/// On [`TALL`] rather than the panel: the page is taller than 480 and scrolls on the device, and the
/// simulator has no scroll — `find` only sees what is visible — so a search for the last row of the
/// list needs a screen that shows it. What the panel is for is the layout, which the test above is.
#[test]
fn the_page_draws_the_networks_it_found() -> Result<(), iced_test::Error> {
    let (_, app) = english();

    let mut ui = screen(&app, TALL);

    assert!(ui.find("Wi-Fi Map").is_ok(), "the page says what it is");
    assert!(ui.find("Pomelo-OS").is_ok(), "a network by name");
    assert!(ui.find("OpenGuest").is_ok(), "the open one too");
    assert!(ui.find("ch 6").is_ok(), "and where it is");
    assert!(ui.find("Use channel").is_ok(), "the advice has a heading");
    assert!(ui.find("Out of band").is_ok(), "and 5 GHz is listed under it");
    assert!(ui.find("ESP-Rust-5G").is_ok(), "by name");

    Ok(())
}

/// The words are in both languages, and the count is said the way each language says it.
#[test]
fn the_words_are_in_both_languages() {
    let english = Words::of(Language::English);
    let chinese = Words::of(Language::Chinese);

    assert_eq!(english.found(1), "1 network");
    assert_eq!(english.found(5), "5 networks");
    assert!(chinese.found(5).contains('5'));
    assert_ne!(english.title, chinese.title);
}

/// What the chart is drawn from is the band and nothing else.
#[test]
fn the_chart_covers_the_whole_band() {
    let (_, app) = scanned();

    let channels = app
        .spectrum()
        .bins()
        .iter()
        .map(|bin| bin.channel)
        .collect::<Vec<_>>();

    assert_eq!(channels.first(), Some(&FIRST));
    assert_eq!(channels.last(), Some(&LAST));

    // The 5 GHz network is the loudest thing the radio heard and the chart must not show it: a column
    // for channel 36 would be a column for a channel the chart does not have.
    let loudest_in_band = app
        .aps()
        .iter()
        .filter(|ap| channel::in_band(ap.channel))
        .map(|ap| ap.rssi)
        .max();

    assert_eq!(loudest_in_band, Some(-58));
}
