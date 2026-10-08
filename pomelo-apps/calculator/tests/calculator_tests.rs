//! The calculator's own tests: what it computes, and what a press does.
//!
//! Nothing here names a platform layer, reads a panel or draws a pixel. `iced_test`'s simulator
//! builds the *real* widget tree, finds a key by the label it is drawn with, presses it, and hands
//! back the messages that press produced — the same path a finger takes, minus the touchscreen and
//! the panel. The tests that do need this board's panel are in `panel_tests.rs`, because they are
//! assertions about the platform rather than about the calculator.

use iced::Size;
use iced_test::Simulator;

use calculator::{eval_op, Calculator, Key, LAYOUT};

/// The panel the layout is designed for. Every dimension in the app is fluid, so it fills whatever
/// it is given — but a test has to pick a size, and this is the one the design was drawn at.
const PANEL: f32 = 480.0;

fn calculator() -> Calculator {
    Calculator::default()
}

/// The widget tree of `calculator`, laid out on a 480×480 surface.
///
/// The font is asked for by *generic* family, not by name: the tester's own default is "Fira Sans",
/// which only exists when iced's `fira-sans` feature is on — 441 KiB of glyphs this firmware has no
/// room for. A generic resolves against whatever the machine has, so text measures to real glyphs
/// and a click can find one.
fn ui(calculator: &Calculator) -> Simulator<'_, calculator::Message> {
    Simulator::with_size(
        iced::Settings {
            default_font: iced::Font::MONOSPACE,
            ..iced::Settings::default()
        },
        Size::new(PANEL, PANEL),
        calculator.view(),
    )
}

/// Presses each label in turn, through the real widget tree, applying what each press produced.
///
/// A fresh tree per press is deliberate: it is what a frame of the real thing does, and it means a
/// press cannot depend on how an earlier one left the widget tree.
fn press_all(calculator: &mut Calculator, labels: &[&str]) -> Result<(), iced_test::Error> {
    for label in labels {
        let mut ui = ui(calculator);

        let _ = ui.click(*label)?;

        for message in ui.into_messages() {
            calculator.update(message);
        }
    }

    Ok(())
}

#[test]
fn seven_plus_three_is_ten() -> Result<(), iced_test::Error> {
    let mut calculator = calculator();

    press_all(&mut calculator, &["7", "＋", "3", "="])?;

    assert_eq!(calculator.display(), "10");
    Ok(())
}

#[test]
fn nine_minus_five_is_four() -> Result<(), iced_test::Error> {
    let mut calculator = calculator();

    press_all(&mut calculator, &["9", "－", "5", "="])?;

    assert_eq!(calculator.display(), "4");
    Ok(())
}

#[test]
fn seven_times_eight_is_fifty_six() -> Result<(), iced_test::Error> {
    let mut calculator = calculator();

    press_all(&mut calculator, &["7", "×", "8", "="])?;

    assert_eq!(calculator.display(), "56");
    Ok(())
}

#[test]
fn eight_divided_by_two_is_four() -> Result<(), iced_test::Error> {
    let mut calculator = calculator();

    press_all(&mut calculator, &["8", "÷", "2", "="])?;

    assert_eq!(calculator.display(), "4");
    Ok(())
}

#[test]
fn the_expression_line_shows_what_was_typed() -> Result<(), iced_test::Error> {
    let mut calculator = calculator();

    press_all(&mut calculator, &["7", "×", "8", "="])?;

    // The characters are the ones printed on the keys, not ASCII: `eval_op` matches `×` and `÷`, and
    // the keypad sends what it draws. Getting that pair out of step is silent — `7 × 8 = 8` — which
    // is why the sweep below exists as well as this.
    assert_eq!(calculator.expression(), "7\u{200a}×\u{200a}8");
    Ok(())
}

#[test]
fn the_clear_key_starts_over() -> Result<(), iced_test::Error> {
    let mut calculator = calculator();

    press_all(&mut calculator, &["7", "×", "8", "=", "C"])?;

    assert_eq!(calculator.display(), "0");
    assert_eq!(calculator.expression(), " ");
    Ok(())
}

/// Every operator key on the keypad has to compute something.
///
/// This is the test that was missing when multiplication and division were silently broken: the
/// keypad sent ASCII (`*`, `/`) while `eval_op` matched the characters the keys are *labelled* with
/// (`×`, `÷`), so both fell through to the fallback arm and returned the second operand. `7 × 8 = 8`
/// is a wrong answer rather than an error, and the one arithmetic test used `+` — which is why it
/// survived. Sweeping the table means the next key added has to answer here.
#[test]
fn every_operator_key_computes_something() {
    for entry in LAYOUT.iter().flat_map(|row| row.iter()) {
        let Key::Operator(op) = entry.key else {
            continue;
        };

        // 6 and 4, so that "the second operand" is distinguishable from every real answer.
        let result = eval_op(6.0, op, 4.0);

        // An operator `eval_op` does not know is a `NaN`, which the display renders as `Error`:
        // visible, though still not an answer.
        assert!(
            result.is_finite(),
            "the `{}` key sends {op:?}, which `eval_op` does not know",
            entry.label
        );
        assert_ne!(
            result, 4.0,
            "the `{}` key evaluates 6 {op} 4 to the second operand",
            entry.label
        );
    }
}

/// Every key the table draws can be found and pressed.
///
/// A label is what the panel shows and what a test — and a reader — uses to reach a key, so two
/// keys sharing one would make a press land on whichever the tree finds first. `click` answering
/// `Ok` means the label matched exactly one widget.
#[test]
fn every_key_can_be_found_and_pressed() -> Result<(), iced_test::Error> {
    let calculator = calculator();

    for entry in LAYOUT.iter().flat_map(|row| row.iter()) {
        assert!(!entry.label.is_empty(), "a key is drawn with no label");

        let mut ui = ui(&calculator);
        ui.click(entry.label)?;
    }

    Ok(())
}

#[test]
fn the_theme_can_be_switched() {
    use pomelo_widgets::preferences::ThemeMode;

    let mut calc = calculator();
    assert_eq!(calc.theme_mode(), ThemeMode::Dark);

    calc.set_theme_mode(ThemeMode::Light);
    assert_eq!(calc.theme_mode(), ThemeMode::Light);

    // Verify theme produces different background
    let dark_theme = {
        let mut c = calculator();
        c.set_theme_mode(ThemeMode::Dark);
        c.theme()
    };
    let light_theme = calc.theme();
    assert_ne!(dark_theme.palette().background, light_theme.palette().background);
}
