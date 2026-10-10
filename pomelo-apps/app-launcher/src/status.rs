//! The status bar: the clock on the left, the signal and the battery on the right.
//!
//! Three readings dynamically driven by [`Subscription`](iced::Subscription), and
//! two of them are drawn as **icons** out of `pomelo_material_symbols` rather than as shapes of this
//! file's own: a bar chart of four rectangles said "signal" only because the launcher drew one, while
//! [`Icon::WIFI_2_BAR`] is a picture everyone has already seen. The clock stays text, because a
//! number is what it is.
//!
//! # Four bars of signal, three pictures of it
//!
//! `pomelo_hal::signal_bars` answers on a `0..=4` scale — four thresholds ten dBm apart, which is
//! about as finely as an RSSI can honestly be divided — and Material Symbols stops at three bars. So
//! `n` bars draw the `n`-bar icon and the fourth draws the third's picture: the difference between
//! −50 and −60 dBm is not something a 20 px glyph can show, and a picture that overstated the signal
//! would be worse than one that stops at "strong". The settings app's list is where four steps are
//! drawn as four.
//!
//! # The battery is drawn, and its reading is inside it
//!
//! It is the one reading on this bar that is not a glyph, and the number is why: the charge is
//! written *inside* the shape, so the shape has to be a box with room for three digits — and a
//! battery in the icon set is a picture of a battery filling up, whose interior is the fill. So the
//! outline, the terminal on its right and the digits are drawn here, at `style`'s measurements.
//!
//! What is *not* written is a `%`. The shape says what kind of number it is, and a sign beside a
//! number that is inside a picture of a battery is the same thing said twice.
//!
//! # The row is inset, and the band is not
//!
//! The panel's corners are round ([`style::STATUS_INSET`]): the bar's *black* reaches both edges —
//! it is the top of the display — while what is drawn on it stops short of the corners.

use iced::widget::{container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Length, Padding};
use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::ThemeMode;

use crate::style;
use crate::Message;

/// The bar as an element: the clock and the background apps on the left, the signal and the charge
/// on the right.
///
/// The whole bar is one row, and nothing on it is interactive — the readings are the platform's to
/// push and the bar has no message of its own — so this is a plain `Element` and not one mapped from
/// a message.
pub fn view<'a>(
    clock: &'a str,
    battery: u8,
    charging: bool,
    wifi: u8,
    background_icons: &[Icon],
    theme_mode: ThemeMode,
) -> Element<'a, Message> {
    let (bg_color, status_fg, bg_icon_color) = if theme_mode.is_dark() {
        (
            Color::BLACK,
            Color::WHITE,
            Color::from_rgb8(156, 163, 175),
        )
    } else {
        (
            Color::WHITE,
            Color::BLACK,
            Color::from_rgb8(107, 114, 128),
        )
    };

    let icon = |glyph: Icon| {
        text(glyph.glyph())
            .size(style::STATUS_ICON)
            .font(icons::font())
            .color(status_fg)
    };

    let battery_group = battery_widget(battery, charging, status_fg);

    let mut bg_icons_row = row![].align_y(Alignment::Center).spacing(style::STATUS_BG_APP_GAP);
    for &bg_icon in background_icons {
        bg_icons_row = bg_icons_row.push(
            text(bg_icon.glyph())
                .size(style::STATUS_BG_APP_ICON)
                .font(icons::font())
                .color(bg_icon_color),
        );
    }

    container(
        row![
            text(clock).size(style::STATUS_FONT).color(status_fg),
            bg_icons_row,
            Space::new().width(Length::Fill),
            icon(wifi_icon(wifi)),
            battery_group,
        ]
        .align_y(Alignment::Center)
        .spacing(style::STATUS_GAP),
    )
    .center_y(Length::Fixed(style::STATUS_HEIGHT))
    .width(Length::Fill)
    .padding(Padding {
        left: style::STATUS_INSET,
        right: style::STATUS_INSET,
        ..Padding::ZERO
    })
    .style(move |_theme| container::Style {
        background: Some(bg_color.into()),
        ..container::Style::default()
    })
    .into()
}

/// The picture for `bars` of signal, on the HAL's own `0..=`[`style::WIFI_BARS`] scale.
///
/// The fourth bar and anything above it draw the fullest picture the set has, which is the three-bar
/// one; see this module's docs. A count above the scale is not rejected — the set is a picture of
/// "at least this strong", and a reading the platform mis-scaled should not take the bar down.
pub fn wifi_icon(bars: u8) -> Icon {
    match bars {
        0 => Icon::WIFI_OFF,
        1 => Icon::WIFI_1_BAR,
        2 => Icon::WIFI_2_BAR,
        _ => Icon::WIFI,
    }
}

/// The charge as an element: a drawn battery with the reading inside it, and a bolt while charging.
///
/// See this module's docs for why it is not a glyph. The outline and the terminal are what make the
/// shape a battery; the number is the reading; and the bolt is the charger, which the number cannot
/// say and the shape has no room to say twice.
///
/// The bolt stands *beside* the battery rather than on it, in the place the percentage used to
/// occupy. A bolt drawn over the outline would have to be drawn over the number too, and a bolt
/// hidden behind a number is a charger nobody can see.
fn battery_widget<'a>(percent: u8, charging: bool, ink: Color) -> Element<'a, Message> {
    let reading = container(
        text(charge(percent))
            .size(style::BATTERY_NUMBER_FONT)
            .color(ink),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    let body = container(reading)
        .width(Length::Fixed(style::BATTERY_W))
        .height(Length::Fixed(style::BATTERY_H))
        .style(move |_theme| container::Style {
            border: Border {
                color: ink,
                width: style::BATTERY_BORDER,
                radius: style::BATTERY_RADIUS.into(),
            },
            ..container::Style::default()
        });

    let terminal = container(Space::new())
        .width(Length::Fixed(style::BATTERY_NUB_W))
        .height(Length::Fixed(style::BATTERY_NUB_H))
        .style(move |_theme| container::Style {
            background: Some(ink.into()),
            border: Border {
                radius: style::BATTERY_NUB_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    let shape = row![body, terminal]
        .align_y(Alignment::Center)
        .spacing(style::BATTERY_NUB_GAP);

    let mut group = row![]
        .align_y(Alignment::Center)
        .spacing(style::BATTERY_BOLT_GAP);

    if charging {
        group = group.push(
            text(Icon::BOLT.glyph())
                .size(style::BATTERY_BOLT_FONT)
                .font(icons::font())
                .color(ink),
        );
    }

    group.push(shape).into()
}

/// The reading written inside the battery: the charge, and not the charge with a sign.
///
/// Its own function because of what it does *not* do: a `%` here would be the shape's meaning
/// written out beside the shape, and the number that a person reads out of a battery is the number.
fn charge(percent: u8) -> String {
    percent.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bar the HAL counts is a bar in the picture, up to the three the set has.
    ///
    /// The bounds are the point: a reading of 0 is the crossed-out icon and not an empty bar chart,
    /// and 3 and 4 are the same picture rather than one of them borrowing a glyph that does not
    /// exist.
    #[test]
    fn the_signal_icon_shows_as_many_bars_as_the_hal_counted() {
        for (bars, icon) in [
            (0, Icon::WIFI_OFF),
            (1, Icon::WIFI_1_BAR),
            (2, Icon::WIFI_2_BAR),
            (3, Icon::WIFI),
            (4, Icon::WIFI),
            (9, Icon::WIFI),
        ] {
            assert_eq!(wifi_icon(bars), icon, "{bars} bars");
        }
    }

    /// The reading inside the battery is the number, and not the number with a sign.
    ///
    /// The regression this exists for is the `%`: it used to be printed beside the shape, and a
    /// shape that already means "percent" does not need to be told so in the middle of itself.
    #[test]
    fn the_charge_is_written_without_its_sign() {
        assert_eq!(charge(0), "0");
        assert_eq!(charge(88), "88");
        assert_eq!(charge(100), "100");

        for percent in 0..=100u8 {
            let written = charge(percent);

            assert_eq!(written, percent.to_string(), "{percent} is written as itself");
            assert!(
                !written.contains('%'),
                "{written} carries a sign the shape already gives it"
            );
        }
    }

    /// The battery is drawn for a charge and for a charger, in both themes.
    ///
    /// Nothing here can say what it *looks* like — a widget tree has no pixels — so what is checked
    /// is that the shape builds at both ends of the scale and with the bolt in either state, which
    /// is where a `Length::Fill` inside a fixed box would blow up.
    #[test]
    fn the_battery_builds_at_both_ends_of_the_scale() {
        for (percent, charging) in [(0, false), (7, false), (100, false), (50, true)] {
            let _ = battery_widget(percent, charging, Color::WHITE);
            let _ = battery_widget(percent, charging, Color::BLACK);
        }
    }

    #[test]
    fn status_view_builds_for_both_themes() {
        let bg_icons = [Icon::COUNTER_0, Icon::TERMINAL];
        let _dark = view("12:00", 80, false, 3, &bg_icons, ThemeMode::Dark);
        let _light = view("12:00", 80, true, 3, &bg_icons, ThemeMode::Light);
    }
}
