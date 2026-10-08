//! The iOS on-screen keyboard: four rows of keys on the band at the bottom of the screen.
//!
//! The terminal types commands with it, the settings app types a Wi-Fi password with it, and
//! neither owns it: the key table is the layout of the keyboard the original had (see [`keys`]) and
//! the palette is its `theme.rs`. What each app keeps for itself is the state a keyboard writes
//! into — a [`String`] and a cursor — and its own message type.
//!
//! # It is flex: it fills the box it is given, in both axes
//!
//! Nothing is passed in and nothing is measured: a key is a share of its row, a row is a quarter of
//! the band, and the band is 100% of whatever box the caller puts it in. A caller that wants the
//! band to be 200 px tall says so on the layout — `container(band(..)).height(Length::Fixed(200.0))`
//! — because the height of a keyboard is a decision about the page, not about the keyboard.
//!
//! That is also why there is no size to hand it: a widget in iced cannot ask how big it will be (the
//! layout happens after `view`), so a keyboard that computed its keys from a number would have to be
//! *told* one — and would then draw the keyboard of whichever screen that number came from.
//!
//! # How an app uses it
//!
//! ```ignore
//! use pomelo_widgets::touch_keyboard::{self, KeyAction, KeyboardMode};
//!
//! // in `view`, at the bottom of the page, as tall as the design says:
//! container(touch_keyboard::band(self.mode, Message::Key))
//!     .height(Length::Fixed(216.0))
//! ```
//!
//! [`band`] hands back a [`KeyAction`] through the closure: `Char`, `Backspace`, `Space`, `Enter`
//! and `SwitchMode`. The keyboard does not decide what any of them mean — `SwitchMode` is the only
//! one the app has to act on *for* the keyboard, by storing the mode it names.
//!
//! # Why it is shared
//!
//! There were two copies of this geometry before (the terminal's `keys.rs` and the original
//! keyboard crate), and a password keyboard that is two pixels off the terminal's is the kind of
//! difference nobody reports and everybody notices. One table, drawn by both — which is what
//! [`pomelo_widgets`](crate) exists for: every widget two apps draw is a module of it.

pub mod keys;
pub mod style;

pub use keys::{Cell, Key, KeyAction, KeyKind, KeyLabel, KeyboardMode};

use iced::widget::{button, container, text, Column, Row, Space};
use iced::{Border, Element, Length, Padding, Shadow};

/// The gap between two keys, in pixels: absolute, like every inset here.
///
/// A gap is a measurement of the design and not of the screen, so it does not follow the band. It is
/// carried as padding *inside* each cell (half at each end) rather than as a `Row`'s spacing, which
/// is what keeps the rows a coincidence of the design from dividing the width differently: with
/// spacing, the row with nine keys instead of ten would give its keys more of the width.
pub const GAP: f32 = 6.0;
/// The band's own padding: to the top and bottom, and to the left and right.
pub const PAD_V: f32 = 8.0;
pub const PAD_H: f32 = 6.0;
/// The gap between two rows.
pub const ROW_GAP: f32 = 8.0;

use crate::preferences::ThemeMode;

/// The band is this share of the screen's height, clamped to [`BAND_MIN`]..[`BAND_MAX`].
///
/// From the original terminal: the keyboard takes about half the panel, and on a taller screen it
/// stops growing — a keyboard that eats a whole desktop window is not a keyboard.
///
/// This is a *policy for a caller that has a screen height*, and not something the keyboard uses:
/// the terminal asks for this height and puts it on the layout itself.
pub const BAND_FRACTION: f32 = 0.48;
/// The shortest a band may be: four rows of keys have to fit.
pub const BAND_MIN: f32 = 180.0;
/// The tallest a band may be.
pub const BAND_MAX: f32 = 240.0;

/// The band's height for a screen `screen_height` tall — a caller's policy, not the keyboard's.
pub fn band_height(screen_height: f32) -> f32 {
    (screen_height * BAND_FRACTION).clamp(BAND_MIN, BAND_MAX)
}

/// The keys of `mode`, filling the box they are given, with no background.
///
/// Both axes are flex: the four rows are equal shares of the height, and a key is its share of its
/// row ([`keys::Key::portion`]). Gap, padding and type are absolute — fluid boxes, absolute type.
pub fn key_rows<'a, M>(mode: KeyboardMode, on_press: impl Fn(KeyAction) -> M + 'a) -> Element<'a, M>
where
    M: Clone + 'a,
{
    key_rows_with_theme(mode, on_press, ThemeMode::Dark)
}

/// The keys of `mode` with `theme`, filling the box they are given, with no background.
pub fn key_rows_with_theme<'a, M>(
    mode: KeyboardMode,
    on_press: impl Fn(KeyAction) -> M + 'a,
    theme: ThemeMode,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let rows = keys::rows(mode).into_iter().map(move |row| {
        let cells = row.into_iter().map(|cell| match cell {
            Cell::Key(key) => key_cell_with_theme(key, &on_press, theme),
            Cell::HalfKey => Space::new()
                .width(Length::FillPortion(keys::HALF_KEY_PORTION))
                .into(),
        });

        Row::with_children(cells).height(Length::Fill).into()
    });

    Column::with_children(rows)
        .spacing(ROW_GAP)
        .height(Length::Fill)
        .into()
}

/// The whole band: the keys on the keyboard's own background, with its padding.
///
/// This is what an app hangs at the bottom of its screen, or inside the card of a password prompt.
/// It fills the box it is given in both axes — the height is the caller's to state.
pub fn band<'a, M>(mode: KeyboardMode, on_press: impl Fn(KeyAction) -> M + 'a) -> Element<'a, M>
where
    M: Clone + 'a,
{
    band_with_theme(mode, on_press, ThemeMode::Dark)
}

/// The whole band for `theme`: the keys on the keyboard's own background, with its padding.
pub fn band_with_theme<'a, M>(
    mode: KeyboardMode,
    on_press: impl Fn(KeyAction) -> M + 'a,
    theme: ThemeMode,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    container(key_rows_with_theme(mode, on_press, theme))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: PAD_V,
            right: PAD_H,
            bottom: PAD_V,
            left: PAD_H,
        })
        .style(move |_theme| container::Style {
            background: Some(style::background_for(theme).into()),
            ..container::Style::default()
        })
        .into()
}

/// One key's cell for `theme`: the key itself, with half the gap at each end.
fn key_cell_with_theme<'a, M>(
    key: Key,
    on_press: &impl Fn(KeyAction) -> M,
    theme: ThemeMode,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    container(key_button_with_theme(key, on_press, theme))
        .width(Length::FillPortion(key.portion))
        .height(Length::Fill)
        .padding(Padding {
            top: 0.0,
            right: GAP / 2.0,
            bottom: 0.0,
            left: GAP / 2.0,
        })
        .into()
}

/// One key for `theme`: a button whose role's palette paints it, lighter while the finger is on it.
fn key_button_with_theme<'a, M>(
    key: Key,
    on_press: &impl Fn(KeyAction) -> M,
    theme: ThemeMode,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let palette = style::palette_for(key.kind, theme);

    let content: Element<'a, M> = match key.label {
        KeyLabel::Text(label) => text(label).size(key.font).into(),
        KeyLabel::Icon(icon) => text(icon.glyph())
            .size(key.font)
            .font(pomelo_material_symbols::font())
            .into(),
    };

    button(
        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(0)
    .style(move |_theme, status| button::Style {
        background: Some(
            match status {
                button::Status::Pressed => palette.pressed,
                _ => palette.fill,
            }
            .into(),
        ),
        text_color: palette.text,
        border: Border {
            radius: style::KEY_RADIUS.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    })
    .on_press(on_press(key.action))
    .into()
}
