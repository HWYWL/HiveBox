//! The file manager's own tests: the two prompts that ask for a name, and the keyboard each carries.
//!
//! Nothing here reads a panel or draws a pixel. The simulator builds the *real* widget tree, finds a
//! control by the text it is drawn with, and can press it — so a test can say "the prompt for a new
//! folder has letters under it" and mean the tree, rather than a mock of the tree.
//!
//! That is what these tests are for. This panel has no physical keyboard and no focus system, so
//! `iced::widget::text_input` would never be handed focus and never raise a keyboard: the two prompts
//! that ask for a name draw the shared `pomelo-widgets` band themselves and keep the string in the
//! app, exactly as the terminal keeps its input line and the settings app keeps a Wi-Fi password.

use std::sync::Arc;

use iced::{Font, Settings, Size};
use iced_test::Simulator;
use pomelo_hal::Board;
use pomelo_widgets::preferences::Language;
use pomelo_widgets::touch_keyboard::{KeyAction, KeyboardMode};

use files::{Files, Message};

/// The panel the page is laid out for: the height the keyboard's share is measured against.
const PANEL: f32 = 480.0;

/// The app in English, on a simulated board.
///
/// The default language is not English, so a test that means to assert a label has to say which
/// language it is asserting in — otherwise it would pass or fail on a default it does not control.
fn english() -> Files {
    let mut files = Files::new(Arc::new(Board::simulated()));

    let mut preferences = files.preferences();
    preferences.language = Language::English;
    files.set_preferences(preferences);

    files
}

/// `files`' interface, on a panel the size of the one it is drawn for.
fn interface(files: &Files) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings {
            // By *generic* family: the tester's default is "Fira Sans", which exists only when iced's
            // `fira-sans` feature is on — glyphs this firmware has no room for.
            default_font: Font::MONOSPACE,
            ..Settings::default()
        },
        Size::new(PANEL, PANEL),
        files.view(),
    )
}

/// A prompt that asks for a name carries the keyboard: there is no other one on this panel.
#[test]
fn a_new_folder_prompt_has_the_keyboard() -> Result<(), iced_test::Error> {
    let mut files = english();

    files.update(Message::NewFolder);
    assert!(files.prompting(), "the prompt is up");

    let mut ui = interface(&files);

    assert!(ui.find("Cancel").is_ok(), "the question and its answers");
    assert!(ui.find("q").is_ok(), "the letters to answer it with");
    assert!(ui.find("123").is_ok(), "and the way to the numbers");

    Ok(())
}

/// Put the prompt away and nothing of it is left on the page — the keyboard belongs to the prompt,
/// and not to the page under it.
#[test]
fn cancel_takes_the_keyboard_with_it() -> Result<(), iced_test::Error> {
    let mut files = english();

    // Return on a name that is not one answers the prompt without making anything: an empty name is
    // refused by `path::valid_name`, so this test touches no filesystem.
    files.update(Message::NewFolder);
    files.update(Message::Key(KeyAction::Enter));
    assert!(!files.prompting(), "return answered the prompt");

    files.update(Message::NewFolder);
    assert!(files.prompting(), "and asking again brings it back");

    files.update(Message::Cancel);
    assert!(!files.prompting(), "cancel put it away");

    let mut ui = interface(&files);
    assert!(ui.find("Cancel").is_err(), "the question is gone");
    assert!(ui.find("q").is_err(), "and the keyboard went with it");

    Ok(())
}

/// The keys write into the name the app owns, and backspace takes the last character off it.
///
/// A `text_input` would keep the string to itself and hand back whole ones; here the name is the
/// app's, so a key is one character in and a backspace is one character out.
#[test]
fn the_keys_write_the_name() -> Result<(), iced_test::Error> {
    let mut files = english();

    files.update(Message::NewFolder);

    for character in "album".chars() {
        files.update(Message::Key(KeyAction::Char(character)));
    }

    {
        let mut ui = interface(&files);
        assert!(ui.find("album").is_ok(), "the name is drawn in the box");
    }

    files.update(Message::Key(KeyAction::Backspace));

    {
        let mut ui = interface(&files);
        assert!(ui.find("albu").is_ok(), "backspace took the last character");
        assert!(ui.find("album").is_err(), "and only that one");
    }

    // The space is a key of its own, and a name with a space in it is still a name.
    files.update(Message::Key(KeyAction::Space));

    let mut ui = interface(&files);
    assert!(ui.find("albu ").is_ok(), "space is a key like any other");

    Ok(())
}

/// The keyboard's pages are the keyboard's own: the numbers push the letters out, and the capitals
/// take their place.
#[test]
fn the_keyboards_pages_change_the_keys() -> Result<(), iced_test::Error> {
    let mut files = english();

    files.update(Message::NewFolder);
    files.update(Message::Key(KeyAction::SwitchMode(KeyboardMode::Numbers)));

    {
        let mut ui = interface(&files);
        assert!(ui.find("#+=").is_ok(), "the signs are up");
        assert!(ui.find("ABC").is_ok(), "with the way back to the letters");
        assert!(ui.find("q").is_err(), "and the letters are gone");
    }

    files.update(Message::Key(KeyAction::SwitchMode(KeyboardMode::Upper)));

    let mut ui = interface(&files);
    assert!(ui.find("Q").is_ok(), "the capitals are up");
    assert!(ui.find("q").is_err(), "and the lower case is not");

    Ok(())
}
