//! The handful of shapes every page on this box is made of.
//!
//! Functions, not widgets: each one draws into the [`Ui`] it is handed and returns nothing. A press
//! is decided by the application (`context.tapped(rect)`), because only the application knows what
//! the press means — this module draws the rectangle, not the behaviour behind it.

use crate::theme;
use crate::ui::{Color, Rect, Ui};

/// The corner radius of a card.
pub const CARD_RADIUS: f32 = 18.0;

/// The corner radius of a button.
pub const BUTTON_RADIUS: f32 = 14.0;

/// A card: a filled rounded rectangle with nothing in it.
pub fn card(ui: &mut Ui, rect: Rect, fill: Color) {
    ui.rounded_rect(rect, CARD_RADIUS, fill);
}

/// A card with a one-pixel outline, for the one card on a page that is selected.
pub fn card_outlined(ui: &mut Ui, rect: Rect, fill: Color, outline: Color) {
    card(ui, rect, fill);
    ui.rounded_stroke(rect, CARD_RADIUS, 1.0, outline);
}

/// A button with a centred label.
///
/// `pressed` is the state the caller got from the input, not a state this function keeps: a button
/// is pressed during the frame the finger is down, and drawing happens after the event that said so.
pub fn button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    size: f32,
    fill: Color,
    label_color: Color,
    pressed: bool,
) {
    let fill = if pressed {
        theme::shade(fill, 40)
    } else {
        fill
    };

    ui.rounded_rect(rect, BUTTON_RADIUS, fill);
    ui.text_centered(rect, label, size, label_color);
}

/// A button with a glyph instead of a label.
pub fn icon_button(
    ui: &mut Ui,
    rect: Rect,
    glyph: char,
    size: f32,
    fill: Color,
    glyph_color: Color,
    pressed: bool,
) {
    let fill = if pressed {
        theme::shade(fill, 40)
    } else {
        fill
    };

    ui.rounded_rect(rect, BUTTON_RADIUS, fill);

    let (cx, cy) = rect.center();
    ui.icon(cx - size / 2.0, cy - size / 2.0, size, glyph_color, glyph);
}

/// A title, with the unit or the second line under it in muted text.
pub fn heading(ui: &mut Ui, rect: Rect, title: &str, subtitle: &str, size: f32) {
    ui.text(rect.x, rect.y, title, size, theme::TEXT);

    if !subtitle.is_empty() {
        ui.text(
            rect.x,
            rect.y + size + 6.0,
            subtitle,
            (size * 0.55).max(12.0),
            theme::MUTED,
        );
    }
}

/// A label and a value on one row: the shape the settings pages are made of.
pub fn key_value(ui: &mut Ui, rect: Rect, key: &str, value: &str, size: f32) {
    ui.text(rect.x, rect.y, key, size, theme::MUTED);
    ui.text_in(rect, value, size, theme::TEXT, app_abi::Align::Right);
}

/// A progress bar: the track, then the part of it that is done.
pub fn progress(ui: &mut Ui, rect: Rect, fraction: f32, color: Color) {
    let fraction = fraction.clamp(0.0, 1.0);

    ui.rounded_rect(rect, rect.height / 2.0, theme::SURFACE_HIGH);

    if fraction > 0.0 {
        let filled = Rect::new(
            rect.x,
            rect.y,
            (rect.width * fraction).max(rect.height),
            rect.height,
        );

        ui.rounded_rect(filled, rect.height / 2.0, color);
    }
}

/// A hairline across a card.
pub fn divider(ui: &mut Ui, rect: Rect) {
    ui.rect(Rect::new(rect.x, rect.y, rect.width, 1.0), theme::DIVIDER);
}

/// A row in a list: title, a muted second line, and the press state of whatever is behind it.
pub fn list_row(ui: &mut Ui, rect: Rect, title: &str, subtitle: &str, pressed: bool) {
    ui.rounded_rect(
        rect,
        BUTTON_RADIUS,
        if pressed {
            theme::SURFACE_HIGH
        } else {
            theme::SURFACE
        },
    );

    ui.text(rect.x + 16.0, rect.y + 12.0, title, 24.0, theme::TEXT);

    if !subtitle.is_empty() {
        ui.text(rect.x + 16.0, rect.y + 42.0, subtitle, 16.0, theme::MUTED);
    }
}

/// A value shown large, with its caption under it: the reading a tile or a card leads with.
pub fn reading(ui: &mut Ui, rect: Rect, value: &str, caption: &str, size: f32) {
    ui.text(rect.x, rect.y, value, size, theme::TEXT);
    ui.text(rect.x, rect.y + size + 4.0, caption, 16.0, theme::MUTED);
}

/// Whether a press should be drawn on a button, from the input state and the button's rectangle.
pub fn pressed(input: &crate::input::Input, rect: Rect) -> bool {
    input.pressing(rect)
}
