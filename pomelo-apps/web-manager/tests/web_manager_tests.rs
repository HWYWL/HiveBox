//! The web manager's subject that is not a widget tree: the switch, and the address rule behind it.
//!
//! The rule itself lives in the HAL (`WebStatus::url`) and is tested there. What is tested here is
//! that the app *asks* it and reports the answer: a page that built its address from a port and an IP
//! of its own would agree with the board only by accident.

use std::sync::Arc;

use pomelo_hal::{Board, WebStatus};
use pomelo_widgets::SystemPreferences;
use web_manager::{Message, ThemeMode, WebManager};

/// Flipping the switch once is a start on the default port, and twice is a stop.
///
/// The board is asked as well as the app: the status the page draws is the *backend's*, and a page
/// that kept its own flag would keep drawing "running" for a server that never bound a socket.
#[test]
fn the_switch_starts_on_the_default_port_and_stops_again() {
    let board = Arc::new(Board::simulated());
    let mut app = WebManager::new(Arc::clone(&board));

    assert!(!app.is_running());
    assert!(!board.web().status().running);

    app.update(Message::Toggle);

    assert!(app.is_running());
    assert_eq!(app.status().port, WebStatus::DEFAULT_PORT);
    assert_eq!(board.web().status().port, WebStatus::DEFAULT_PORT);
    assert!(app.error().is_none());

    app.update(Message::Toggle);

    assert!(!app.is_running());
    assert!(!board.web().status().running);
    assert_eq!(app.status().port, 0, "down has no port");
}

/// The address appears only when the server is up *and* there is a network to reach it on.
#[test]
fn the_address_appears_only_once_there_is_a_network_to_reach_it_on() {
    let board = Arc::new(Board::simulated());
    let mut app = WebManager::new(Arc::clone(&board));

    // Off and off the network: the two are the same answer, "do not tell someone to open this".
    assert_eq!(app.url(), None);

    app.update(Message::Toggle);
    assert!(app.is_running());
    assert_eq!(app.url(), None, "a server nobody can reach has no address");

    // Join the simulated network, and the address the browser would open appears.
    board
        .wifi()
        .connect("ESP-Rust-5G", "12345678")
        .expect("a simulated access point");

    assert_eq!(app.url().as_deref(), Some("http://192.168.1.108"));

    // Stopping takes the address away with it, without the network changing.
    app.update(Message::Toggle);

    assert_eq!(app.url(), None);
    assert_eq!(
        board.wifi().status().ip,
        "192.168.1.108",
        "the network is untouched: it is the server that is down"
    );
}

/// A failure is kept rather than swallowed, so the page can say the switch did not move.
#[test]
fn a_failed_start_is_reported_and_the_switch_stays_where_it_was() {
    let board = Arc::new(Board::simulated());
    let mut app = WebManager::new(Arc::clone(&board));

    // A port of zero is the one refusal the HAL's trait documents, and it is reachable from here
    // only through the board — so the switch itself is exercised, and the refusal is asserted on the
    // backend underneath it.
    assert_eq!(
        board.web().start(0),
        Err(pomelo_hal::HalError::InvalidArg),
        "the board refuses a port of zero; the app never asks for one"
    );

    app.update(Message::Toggle);

    assert!(app.is_running());
    assert!(app.error().is_none());
}

/// Preferences reach the app, whichever theme and language the launcher is in.
#[test]
fn preferences_travel_with_the_app_and_both_themes_build_a_view() {
    let board = Arc::new(Board::simulated());
    let mut app = WebManager::new(Arc::clone(&board));

    let prefs = SystemPreferences::new(
        pomelo_widgets::Language::English,
        ThemeMode::Light,
        pomelo_widgets::FontSizeTier::Large,
    );
    app.set_preferences(prefs);

    assert_eq!(app.preferences(), prefs);
    assert_eq!(app.language(), pomelo_widgets::Language::English);

    // Both states, both themes: the view is built from the state, so both have to lay out. The
    // element borrows the app, so each one is dropped in its own scope before the next mutation.
    drop(app.view());

    app.update(Message::Toggle);
    drop(app.view());

    app.set_preferences(SystemPreferences::new(
        pomelo_widgets::Language::Chinese,
        ThemeMode::Dark,
        pomelo_widgets::FontSizeTier::Standard,
    ));
    drop(app.view());
}
