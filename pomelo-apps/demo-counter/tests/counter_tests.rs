//! The counter's own tests: what a tap does, through the widget tree.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. `iced_test`'s simulator
//! builds the *real* widget tree, finds the button by the label it is drawn with, presses it, and
//! hands back the message that produced. What the counter costs the board — frames, damage, a
//! finger on the panel — is asserted in `firmware/panel-tests`, where the panel is.

use iced::Size;
use iced_test::Simulator;

use demo_counter::{Counter, Message};

/// The panel the layout is designed for. The app fills whatever it is given; a test has to pick a
/// size, and this is the one the design was drawn at.
const PANEL: f32 = 480.0;

/// The label on the button in Chinese and English.
const BUTTON_LABEL_ZH: &str = "点击 +1";
const BUTTON_LABEL_EN: &str = "TAP +1";

fn ui(counter: &Counter) -> Simulator<'_, Message> {
    Simulator::with_size(
        iced::Settings {
            // By *generic* family: the tester's default is "Fira Sans", which only exists when
            // iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no room for.
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(PANEL, PANEL),
        counter.view(),
    )
}

#[test]
fn the_button_counts() {
    let mut counter = Counter::new();

    counter.update(Message::Tap);
    assert_eq!(counter.count(), 1);

    counter.update(Message::Tap);
    assert_eq!(counter.count(), 2);
}

#[test]
fn a_press_on_the_button_counts() -> Result<(), iced_test::Error> {
    let mut counter = Counter::new();

    let mut ui = ui(&counter);
    let _ = ui.click(BUTTON_LABEL_ZH)?;

    for message in ui.into_messages() {
        counter.update(message);
    }

    assert_eq!(counter.count(), 1, "the press has to reach the button");
    Ok(())
}

/// The number keeps the layout still as it grows.
///
/// Two digits wide is a formatting decision the layout depends on, and the way it fails is invisible
/// in a still frame: the number shifts by half a digit the moment it becomes two digits, and a press
/// aimed at the button a moment earlier lands where it used to be.
#[test]
fn a_press_still_reaches_the_button_when_the_number_grows() {
    let mut counter = Counter::new();

    for _ in 0..10 {
        counter.update(Message::Tap);
    }

    assert_eq!(counter.count(), 10);

    let mut ui = ui(&counter);
    let _ = ui.click(BUTTON_LABEL_ZH).expect("the button is still there");

    for message in ui.into_messages() {
        counter.update(message);
    }

    assert_eq!(counter.count(), 11);
}

#[test]
fn the_theme_can_be_switched() {
    use pomelo_widgets::preferences::ThemeMode;

    let mut counter = Counter::new();
    assert_eq!(counter.theme_mode(), ThemeMode::Dark);

    counter.set_theme_mode(ThemeMode::Light);
    assert_eq!(counter.theme_mode(), ThemeMode::Light);

    let dark_theme = {
        let mut c = Counter::new();
        c.set_theme_mode(ThemeMode::Dark);
        c.theme()
    };
    let light_theme = counter.theme();
    assert_ne!(dark_theme.palette().background, light_theme.palette().background);
}

#[test]
fn the_language_can_be_switched() -> Result<(), iced_test::Error> {
    use demo_counter::Language;

    let mut counter = Counter::new();
    assert_eq!(counter.language(), Language::Chinese);

    // Chinese defaults
    {
        let mut ui_zh = ui(&counter);
        assert!(ui_zh.find(BUTTON_LABEL_ZH).is_ok());
        assert!(ui_zh.find("触摸计数器").is_ok());
        assert!(ui_zh.find("由 Iced 框架驱动").is_ok());
        let _ = ui_zh.click(BUTTON_LABEL_ZH)?;
        for message in ui_zh.into_messages() {
            counter.update(message);
        }
    }
    assert_eq!(counter.count(), 1);

    // Switch to English
    counter.set_language(Language::English);
    assert_eq!(counter.language(), Language::English);

    {
        let mut ui_en = ui(&counter);
        assert!(ui_en.find(BUTTON_LABEL_EN).is_ok());
        assert!(ui_en.find("TOUCH COUNTER").is_ok());
        assert!(ui_en.find("Powered by Iced").is_ok());
        assert!(ui_en.find(BUTTON_LABEL_ZH).is_err());
        assert!(ui_en.find("由 Iced 提供支持").is_err());
        let _ = ui_en.click(BUTTON_LABEL_EN)?;
        for message in ui_en.into_messages() {
            counter.update(message);
        }
    }
    assert_eq!(counter.count(), 2);

    Ok(())
}
