//! The terminal's own tests: what typing does, through the widget tree.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. `iced_test`'s simulator
//! builds the *real* widget tree, finds a key by the label it is drawn with, presses it, and hands
//! back the message that produced. What the terminal costs the board — frames, damage, a finger on
//! the keyboard — is asserted in `firmware/panel-tests`, where the panel is.

use iced::Size;
use iced_test::Simulator;
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
