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

// ---------------------------------------------------------------------------------------------
// The shapes a page on this box is made of, and that a package used to have to draw by hand.
//
// They are here because the applications that became packages kept their pages when they moved,
// and every one of them was re-drawing the same five things out of `rect` and `text`: a header
// with the app's glyph in a square, a section title, a row with a glyph on the left and a number
// on the right, a slider, and an empty state. Drawn badly in ten places is what made the packages
// look like a different machine; drawn once is what puts them back.
// ---------------------------------------------------------------------------------------------

/// A page header: the application's glyph in a filled square, then its name, then what it is
/// looking at. This is the shape every page inside the box opened with, and the square is the same
/// picture the tile on the desktop is.
pub fn page_header(
    ui: &mut Ui,
    rect: Rect,
    glyph: char,
    accent: Color,
    title: &str,
    subtitle: &str,
) {
    let badge = Rect::new(rect.x, rect.y, rect.height, rect.height);

    ui.rounded_rect(badge, 14.0, accent);
    ui.icon(badge.x + 11.0, badge.y + 11.0, badge.height - 22.0, theme::TEXT, glyph);

    let x = rect.x + rect.height + 14.0;
    ui.text(x, rect.y + 2.0, title, 26.0, theme::TEXT);

    if subtitle.is_empty() {
        // One line, so it sits in the middle of the square rather than under the title.
        ui.text(x, rect.y + 16.0, "", 16.0, theme::MUTED);
    } else {
        ui.text(x, rect.y + 34.0, subtitle, 16.0, theme::MUTED);
    }
}

/// A pill button: the shape the top right corner of a page uses for "scan again".
pub fn pill(ui: &mut Ui, rect: Rect, label: &str, glyph: char, fill: Color, pressed: bool) {
    let fill = if pressed { theme::shade(fill, 40) } else { fill };

    ui.rounded_rect(rect, rect.height / 2.0, fill);

    // The glyph first, then the words: a pill with only a word in it is a button that has to be
    // read, and one with only a glyph is a button that has to be guessed.
    let (glyph_width, text_width) = (18.0, ui.measure(label, 16.0));
    let start = rect.x + (rect.width - (glyph_width + 6.0 + text_width)) / 2.0;
    let middle = rect.y + rect.height / 2.0;

    ui.icon(start, middle - 9.0, glyph_width, theme::TEXT, glyph);
    ui.text(start + glyph_width + 6.0, middle - 8.0, label, 16.0, theme::TEXT);
}

/// A section title with its own glyph: what groups the cards on a page, and where a page says
/// "these numbers are from Open-Meteo" without putting it on every row.
pub fn section(ui: &mut Ui, rect: Rect, glyph: char, color: Color, title: &str, note: &str) {
    ui.icon(rect.x, rect.y + 2.0, 18.0, color, glyph);
    ui.text(rect.x + 26.0, rect.y, title, 18.0, theme::TEXT);

    if !note.is_empty() {
        ui.text_in(rect, note, 14.0, theme::MUTED, app_abi::Align::Right);
    }
}

/// A row in a list: a glyph, a title, a second line, and a value at the right edge. The file
/// manager, the music player and the channel map are this row, one after another.
pub fn row(
    ui: &mut Ui,
    rect: Rect,
    glyph: char,
    glyph_color: Color,
    title: &str,
    subtitle: &str,
    value: &str,
    pressed: bool,
    fill: Color,
    bar: Option<Color>,
) {
    ui.rounded_rect(
        rect,
        BUTTON_RADIUS,
        if pressed { theme::shade(fill, 20) } else { fill },
    );

    if let Some(color) = bar {
        // A bar down the left edge, which is how a row says "this is the one" without having to
        // fill the whole thing with its accent — a filled row reads as a button, and the one the
        // music player is playing is not a button.
        ui.rect(
            Rect::new(rect.x, rect.y + 8.0, 4.0, rect.height - 16.0),
            color,
        );
    }

    ui.icon(rect.x + 14.0, rect.y + (rect.height - 22.0) / 2.0, 22.0, glyph_color, glyph);

    let x = rect.x + 48.0;

    if subtitle.is_empty() {
        ui.text(x, rect.y + (rect.height - 20.0) / 2.0, title, 20.0, theme::TEXT);
    } else {
        ui.text(x, rect.y + 10.0, title, 20.0, theme::TEXT);
        ui.text(x, rect.y + 34.0, subtitle, 14.0, theme::MUTED);
    }

    if !value.is_empty() {
        ui.text_in(
            Rect::new(x, rect.y, rect.right() - x - 16.0, rect.height),
            value,
            16.0,
            theme::MUTED,
            app_abi::Align::Right,
        );
    }
}

/// A filled circle the size of a full stop: what says online / offline, or charging / not, in the
/// space a word would need a line of its own for.
pub fn dot(ui: &mut Ui, center: (f32, f32), radius: f32, color: Color) {
    ui.circle(center, radius, color);
}

/// A slider: the track, the part of it that is done, and the knob a finger drags. The music
/// player's position and the volume are this, and both are read *and* written by a press.
pub fn slider(ui: &mut Ui, rect: Rect, fraction: f32, fill: Color, pressed: bool) {
    let fraction = fraction.clamp(0.0, 1.0);

    ui.rounded_rect(rect, rect.height / 2.0, theme::SURFACE_HIGH);
    ui.rounded_rect(
        Rect::new(rect.x, rect.y, (rect.width * fraction).max(rect.height), rect.height),
        rect.height / 2.0,
        fill,
    );

    let knob = if pressed { 9.0 } else { 7.0 };
    let x = rect.x + rect.height / 2.0 + (rect.width - rect.height) * fraction;

    ui.circle((x, rect.y + rect.height / 2.0), knob, theme::TEXT);
}

/// A switch: one track, one knob, and the colour of the track is the answer.
pub fn switch(ui: &mut Ui, rect: Rect, on: bool, accent: Color, pressed: bool) {
    let fill = if on { accent } else { theme::SURFACE_HIGH };
    let fill = if pressed { theme::shade(fill, 30) } else { fill };

    ui.rounded_rect(rect, rect.height / 2.0, fill);

    let radius = rect.height / 2.0 - 6.0;
    let x = if on {
        rect.right() - 6.0 - radius
    } else {
        rect.x + 6.0 + radius
    };

    ui.circle((x, rect.y + rect.height / 2.0), radius, theme::TEXT);
}

/// An empty state: a glyph over a line that says what is not there. A page with nothing in it and
/// nothing to say about it looks broken.
pub fn empty(ui: &mut Ui, rect: Rect, glyph: char, text: &str, color: Color) {
    ui.icon(rect.x, rect.y, 44.0, theme::DIVIDER, glyph);
    ui.text(rect.x, rect.y + 60.0, text, 18.0, color);
}

/// A note at the foot of a page: where the numbers came from, or what a switch really means. Its
/// glyph is what says which of the two it is.
pub fn note(ui: &mut Ui, rect: Rect, glyph: char, text: &str, color: Color) {
    ui.icon(rect.x, rect.y, 16.0, color, glyph);
    ui.text(rect.x + 24.0, rect.y, text, 14.0, color);
}

/// A reading with its glyph beside it: the temperature, the humidity and the wind are three of
/// these in a row, and the glyph is what makes three numbers on one line readable.
pub fn metric(
    ui: &mut Ui,
    rect: Rect,
    glyph: char,
    value: &str,
    caption: &str,
    size: f32,
    color: Color,
) {
    ui.icon(rect.x, rect.y + size * 0.35, 20.0, color, glyph);
    ui.text(rect.x + 28.0, rect.y, value, size, theme::TEXT);
    ui.text(rect.x + 28.0, rect.y + size + 2.0, caption, 14.0, theme::MUTED);
}
