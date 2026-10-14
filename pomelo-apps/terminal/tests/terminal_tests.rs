//! The terminal's own tests: what typing does, through the widget tree.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. `iced_test`'s simulator
//! builds the *real* widget tree, finds a key by the label it is drawn with, presses it, and hands
//! back the message that produced. What the terminal costs the board — frames, damage, a finger on
//! the keyboard — is asserted in `firmware/panel-tests`, where the panel is.

use std::sync::Arc;
use std::time::Duration;

use iced::time::Instant;
use iced::Size;
use iced_test::Simulator;
use pomelo_hal::{Board, NasCredentials, NasFault, NasShellStatus};
use pomelo_widgets::pomelo_material_symbols::Icon;

use terminal::{Message, Terminal, SCREEN};

const RETURN: &str = Icon::KEYBOARD_RETURN.glyph();
const DEL: &str = Icon::BACKSPACE.glyph();

/// Every key of the on-screen keyboard, by the label it is drawn with.
///
/// The labels are the keyboard's own (`keys::rows`), and they are what a person reads to aim a
/// finger: a test that clicked coordinates would be testing the layout instead of the keyboard.
fn interface(terminal: &Terminal) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            // By *generic* family: the tester's default is "Fira Sans", which only exists when
            // iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no room for.
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        terminal.size(),
        terminal.view(),
    )
}

/// Types `keys` on a freshly built tree of `terminal`'s current state, applying what came back.
fn type_keys(terminal: &mut Terminal, keys: &[&str]) -> Result<(), iced_test::Error> {
    for key in keys {
        let mut ui = interface(terminal);
        let _ = ui.click(*key)?;
        for message in ui.into_messages() {
            terminal.update(message);
        }
    }

    Ok(())
}

#[test]
fn typing_on_the_keyboard_builds_the_input_line() -> Result<(), iced_test::Error> {
    let mut terminal = Terminal::new();

    type_keys(&mut terminal, &["e", "c", "h", "o"])?;

    assert_eq!(terminal.current_input(), "echo");
    Ok(())
}

#[test]
fn shift_locks_uppercase_until_pressed_again() -> Result<(), iced_test::Error> {
    let mut terminal = Terminal::new();
    let shift = Icon::SHIFT.glyph();

    type_keys(
        &mut terminal,
        &[shift, "H", "E", "L", "L", "O", shift, "w", "o", "r", "l", "d"],
    )?;

    assert_eq!(terminal.current_input(), "HELLOworld");
    Ok(())
}

#[test]
fn return_runs_the_command_and_prints_its_output() -> Result<(), iced_test::Error> {
    let mut terminal = Terminal::new();

    type_keys(
        &mut terminal,
        &["e", "c", "h", "o", "space", "h", "i", RETURN],
    )?;

    assert_eq!(terminal.current_input(), "", "return clears the input line");
    assert!(
        terminal.history_lines().iter().any(|line| line == "hi"),
        "the command's output is in the history: {:?}",
        terminal.history_lines()
    );
    Ok(())
}

/// `pwd` prints where the shell is, which is the one command whose output the test can predict.
///
/// Not `cd`: where `cd` can go depends on what storage the platform gave the shell, and that is a
/// property of *this* machine rather than of the terminal (`shell::get_storage_root` picks between
/// `/storage`, `./storage` and `.`). A test that asserted a directory would be asserting the
/// developer's filesystem.
#[test]
fn pwd_prints_the_working_directory() -> Result<(), iced_test::Error> {
    let mut terminal = Terminal::new();
    let cwd = terminal.cwd().to_string();

    type_keys(&mut terminal, &["p", "w", "d", RETURN])?;

    assert!(
        terminal.history_lines().iter().any(|line| line == &cwd),
        "pwd has to print {cwd:?}: {:?}",
        terminal.history_lines()
    );
    Ok(())
}

/// Backspace and the mode keys are keys like any other, and the line is what they act on.
#[test]
fn del_removes_the_last_character() -> Result<(), iced_test::Error> {
    let mut terminal = Terminal::new();

    type_keys(&mut terminal, &["a", "b", "c", DEL])?;

    assert_eq!(terminal.current_input(), "ab");
    Ok(())
}

/// The app starts at the design size and believes what it is told afterwards.
///
/// The first half is what makes the app runnable on its own (a desktop window that never reports a
/// size, a board whose platform forgets); the second half is the message the subscription produces.
#[test]
fn the_screen_size_is_a_message() {
    let mut terminal = Terminal::new();

    assert_eq!(
        terminal.size(),
        Size::new(SCREEN as f32, SCREEN as f32),
        "the design size, until something says otherwise"
    );

    let was = terminal.keyboard_height();
    terminal.update(Message::Resized(Size::new(SCREEN as f32, 320.0)));

    assert_eq!(terminal.size(), Size::new(SCREEN as f32, 320.0));
    assert!(
        terminal.keyboard_height() < was,
        "a shorter screen means a shorter keyboard band: {was} then {}",
        terminal.keyboard_height()
    );
}

#[test]
fn the_theme_can_be_switched() {
    use pomelo_widgets::preferences::ThemeMode;

    let mut term = Terminal::new();
    assert_eq!(term.theme_mode(), ThemeMode::Dark);

    term.set_theme_mode(ThemeMode::Light);
    assert_eq!(term.theme_mode(), ThemeMode::Light);

    let dark_theme = {
        let mut t = Terminal::new();
        t.set_theme_mode(ThemeMode::Dark);
        t.theme()
    };
    let light_theme = term.theme();
    assert_ne!(dark_theme.palette().background, light_theme.palette().background);
}

#[test]
fn preferences_roundtrip() {
    use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};

    let mut term = Terminal::new();
    let mut prefs = SystemPreferences::default();
    prefs.theme = ThemeMode::Light;
    term.set_preferences(prefs);

    assert_eq!(term.preferences(), prefs);
    assert_eq!(term.theme_mode(), ThemeMode::Light);
}

// -----------------------------------------------------------------------------------------------
// A session on the machine at the other end of the network
// -----------------------------------------------------------------------------------------------

/// A machine this board has been told about, and a board to reach it through.
///
/// The board is the simulator's, so the session on the other end is a real one: it answers, it has a
/// directory of its own, and it keeps quiet when it has nothing to say — see `pomelo-hal`'s `sim`. What
/// the terminal does with those answers is the whole of what these tests are about.
fn board_with_a_machine() -> (Arc<Board>, NasCredentials) {
    let board = Board::simulated();
    let device = machine();
    board.nas().remember(&device).expect("the machine is taken");

    (Arc::new(board), device)
}

/// The machine as the NAS monitor would have it: an address, a port, an account.
fn machine() -> NasCredentials {
    NasCredentials {
        host: String::from("192.168.1.10"),
        port: 22,
        user: String::from("nasstat"),
        password: String::from("hunter2"),
        interval_secs: 5,
        host_key: None,
    }
}

/// Being pointed at a machine replaces the page, and the page is the machine's.
///
/// The prompt on it is the *machine's* and not this app's: a shell at the other end prints
/// `user@host:cwd$ ` before it reads, that arrives as bytes like everything else, and the app draws it
/// as it draws any other line. A test that found this app's own prompt in a session's page would have
/// found the app talking to itself.
#[test]
fn a_session_shows_the_machines_page() {
    let (board, device) = board_with_a_machine();
    let mut terminal = Terminal::new().with_board(board);
    let local = terminal.history_lines();

    terminal.connect(&device);
    assert!(terminal.session().is_some(), "the app is pointed at a machine");

    terminal.update(Message::Tick(Instant::now()));

    let page = terminal.history_lines();
    assert_ne!(page, local, "and the page is no longer this app's own");
    assert!(
        page.iter()
            .any(|line| line.starts_with("nasstat@192.168.1.10:")),
        "the machine's prompt is on it: {page:?}"
    );
}

/// The keyboard is the same keyboard, and what it types goes down the wire.
///
/// A line is sent as a line rather than a keystroke at a time, which is what makes this enough: the
/// shell at the other end does its own echoing and line editing, so what comes back is the echo *and*
/// the answer, in one order, from one place.
#[test]
fn typing_at_a_session_goes_to_the_machine() -> Result<(), iced_test::Error> {
    let (board, device) = board_with_a_machine();
    let mut terminal = Terminal::new().with_board(board);
    terminal.connect(&device);
    terminal.update(Message::Tick(Instant::now()));

    type_keys(&mut terminal, &["p", "w", "d"])?;
    assert_eq!(terminal.current_input(), "pwd", "the app holds the line");

    type_keys(&mut terminal, &[RETURN])?;
    assert_eq!(terminal.current_input(), "", "return sends it and lets it go");

    terminal.update(Message::Tick(Instant::now()));

    let page = terminal.history_lines();
    assert!(
        page.iter().any(|line| line.ends_with("/volume1")),
        "and what the machine printed is on the page: {page:?}"
    );
    Ok(())
}

/// Leaving a session gives back the shell in this app, exactly where it was left.
///
/// The two shells are two shells: a session replaces the *page* and not the app, so the transcript and
/// the half-typed line of the shell underneath are still there when somebody comes back to them.
#[test]
fn leaving_a_session_brings_back_this_apps_shell() -> Result<(), iced_test::Error> {
    let (board, device) = board_with_a_machine();
    let mut terminal = Terminal::new().with_board(board);

    type_keys(&mut terminal, &["e", "c", "h", "o"])?;
    let local = terminal.history_lines();

    terminal.connect(&device);
    terminal.update(Message::Tick(Instant::now()));
    assert!(terminal.session().is_some());

    terminal.update(Message::Leave);

    assert!(terminal.session().is_none(), "the session is let go");
    assert_eq!(
        terminal.history_lines(),
        local,
        "and the shell in this app is where it was"
    );
    assert_eq!(
        terminal.current_input(),
        "echo",
        "with the line that was half typed"
    );
    Ok(())
}

/// A machine nobody knows is a fault, the page says what happened, and it offers the ways out.
///
/// Both of them, and neither of them while the session is still coming up: there is nothing to do
/// about a login in flight but wait, and a button that cannot do anything is worse than no button.
#[test]
fn a_machine_nobody_knows_is_a_fault_with_a_way_out() -> Result<(), iced_test::Error> {
    let (board, _) = board_with_a_machine();
    let mut terminal = Terminal::new().with_board(board);
    let stranger = NasCredentials {
        host: String::from("10.0.0.9"),
        ..machine()
    };

    terminal.connect(&stranger);

    assert_eq!(
        terminal.session().expect("a session").status(),
        NasShellStatus::Lost(NasFault::Unreachable),
        "a host the board has never been told about is refused rather than opened"
    );

    let mut ui = interface(&terminal);
    assert!(
        ui.find("10.0.0.9 did not answer.").is_ok(),
        "the page says what happened, and to which machine"
    );
    assert!(ui.find("Reconnect").is_ok(), "and offers to try again");

    let _ = ui.click("Leave")?;
    for message in ui.into_messages() {
        terminal.update(message);
    }

    assert!(terminal.session().is_none(), "leave takes the session away");
    Ok(())
}

/// A desktop window that was never given a board has no machine to be pointed at, and no complaint
/// about it: the shell in this app is the terminal, and a terminal pointed at nothing is still one.
#[test]
fn without_a_board_there_is_nothing_to_connect_to() {
    let mut terminal = Terminal::new();

    terminal.connect(&machine());
    terminal.update(Message::Tick(Instant::now()));

    assert!(terminal.session().is_none());
    assert!(
        !terminal.history_lines().is_empty(),
        "the shell in this app is what is left"
    );
}

/// A cursor asks to be woken up in either shell, and a page with no cursor on it is what stops the clock.
///
/// The clock is what a page needs when something on it changes on nobody's message, and there are two
/// such things now: a cursor, which blinks wherever it is drawn — at the end of this app's own prompt
/// and at the end of a session's — and a session's output, which arrives when the machine decides to
/// send it. The one page that asks for neither is a session that has hung up: no prompt, no cursor, and
/// nothing more to say. It is decided per subscription rather than per update, so a page like that costs
/// exactly what an idle terminal costs. Asserted on what the app *built* rather than on a subscription's
/// `Debug`, which iced prints as the bare word `Subscription` and no detail of what is in it.
#[test]
fn a_cursor_is_what_asks_for_a_clock() -> Result<(), iced_test::Error> {
    let (board, device) = board_with_a_machine();
    let mut terminal = Terminal::new().with_board(board);

    assert!(
        terminal.wants_a_clock(),
        "the shell in this app has a cursor to blink"
    );

    terminal.connect(&device);
    assert!(
        terminal.wants_a_clock(),
        "and a session's page has one, and answers when it feels like it"
    );

    // `exit`: the shell at the other end hangs up, and what is left is a page to read.
    type_keys(&mut terminal, &["e", "x", "i", "t", RETURN])?;
    terminal.update(Message::Tick(Instant::now()));

    assert_eq!(
        terminal.session().expect("the session is still on the page").status(),
        NasShellStatus::Closed,
        "the machine said goodbye"
    );
    assert!(
        !terminal.wants_a_clock(),
        "no cursor and no more answers: nothing left to wake up for"
    );

    terminal.update(Message::Leave);
    assert!(
        terminal.wants_a_clock(),
        "and this app's own shell has a cursor again"
    );

    terminal.update(Message::Resized(Size::new(480.0, 320.0)));
    assert!(
        terminal.wants_a_clock(),
        "a resize is not what stops a cursor blinking"
    );
    Ok(())
}

/// The cursor blinks on the clock and not on the frames: half a second showing, half a second not,
/// however many frames a host happens to send in between.
///
/// Counted in frames it would blink at the speed of the screen — sixty a second in a desktop window,
/// once per turn of a board's loop, and however fast that loop is turning — so the ticks carry the
/// instant they happened at and the app measures against it. This is the one place a blink can be
/// watched without watching a screen: a desktop window cannot be told to hurry up, and on a board the
/// panel is the only clock that draws anything.
#[test]
fn the_cursor_blinks_on_the_clock_and_not_on_the_frames() {
    let mut terminal = Terminal::new();
    let start = Instant::now();
    let at = |millis: u64| start + Duration::from_millis(millis);

    assert!(terminal.cursor_showing(), "a line just shown has a cursor on it");

    // Sixty frames inside the first tenth of a second, which is one of them showing and no more.
    for frame in 0..60 {
        terminal.update(Message::Tick(at(frame * 2)));
    }
    assert!(terminal.cursor_showing(), "a frame is not a blink");

    terminal.update(Message::Tick(at(500)));
    assert!(!terminal.cursor_showing(), "half a second in, it goes");

    terminal.update(Message::Tick(at(700)));
    assert!(!terminal.cursor_showing(), "and the frames inside the next half do not bring it back");

    terminal.update(Message::Tick(at(1_000)));
    assert!(terminal.cursor_showing(), "its own half being up is what does");
}
