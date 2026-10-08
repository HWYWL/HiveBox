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
//! # Eight pictures of a battery
//!
//! `battery_android_0` through `_6`, then `_full`: seven steps of about fourteen percent each, and a
//! full battery as its own picture. The icon is the whole reading — the same as the signal, and for
//! the same reason: a percentage printed beside a picture of the percentage would be one reading
//! drawn twice, and eight steps are what a 20 px glyph can say.
//!
//! # The row is inset, and the band is not
//!
//! The panel's corners are round ([`style::STATUS_INSET`]): the bar's *black* reaches both edges —
//! it is the top of the display — while what is drawn on it stops short of the corners.

use iced::widget::{container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Padding};
use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::ThemeMode;

use crate::style;
use crate::Message;

/// The bar as an element: `clock` and background app icons on the left, `wifi` bars and `battery` percent on the right.
///
/// The whole bar is one row: the clock, background app icons, space, then the signal and the charge.
/// Nothing here is interactive — the readings are the platform's to push, and the bar has no message
/// of its own — so this is a plain `Element` and not one mapped from a message.
/// The bar as an element: `clock` and background app icons on the left, `wifi` bars and `battery` percent on the right.
///
/// The whole bar is one row: the clock, background app icons, space, then the signal and the charge.
/// Nothing here is interactive — the readings are the platform's to push, and the bar has no message
/// of its own — so this is a plain `Element` and not one mapped from a message.
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

    let battery_icon_widget = |glyph: Icon| {
        text(glyph.glyph())
            .size(style::STATUS_BATTERY_ICON)
            .font(icons::font())
            .color(status_fg)
            .line_height(1.0)
    };

    let battery_group = row![
        text(format!("{battery}%"))
            .size(style::STATUS_PERCENT_FONT)
            .color(status_fg),
        battery_icon_widget(battery_icon(battery, charging)),
    ]
    .align_y(Alignment::Center)
    .spacing(style::STATUS_BATTERY_GAP);

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

/// The picture for `percent` of charge and `charging` state.
///
/// When `charging` is true, displays [`Icon::BATTERY_ANDROID_FRAME_BOLT`].
/// Otherwise, divided into 7 shares across 100% (each share is 100 / 7 ≈ 14.28%):
/// - Icon 0 (`BATTERY_ANDROID_0`): 0.5 share (0%..=7%)
/// - Icons 1~6 (`BATTERY_ANDROID_1..6`): 1 share each (8%..=21%, 22%..=35%, 36%..=49%, 50%..=64%, 65%..=78%, 79%..=92%)
/// - Icon 7 (`BATTERY_ANDROID_FULL`): 0.5 share (93%..=100%)
pub fn battery_icon(percent: u8, charging: bool) -> Icon {
    if charging {
        return Icon::BATTERY_ANDROID_FRAME_BOLT;
    }
    let index = match percent {
        0..=7 => 0,
        8..=21 => 1,
        22..=35 => 2,
        36..=49 => 3,
        50..=64 => 4,
        65..=78 => 5,
        79..=92 => 6,
        _ => 7,
    };
    BATTERY[index]
}

/// The eight pictures the charge climbs through, in order: seven of a battery filling up, then full.
const BATTERY: [Icon; 8] = [
    Icon::BATTERY_ANDROID_0,
    Icon::BATTERY_ANDROID_1,
    Icon::BATTERY_ANDROID_2,
    Icon::BATTERY_ANDROID_3,
    Icon::BATTERY_ANDROID_4,
    Icon::BATTERY_ANDROID_5,
    Icon::BATTERY_ANDROID_6,
    Icon::BATTERY_ANDROID_FULL,
];

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

    /// When charging is active, the bolt icon is always shown regardless of percentage.
    #[test]
    fn the_charging_bolt_icon_is_shown_when_charging() {
        assert_eq!(battery_icon(0, true), Icon::BATTERY_ANDROID_FRAME_BOLT);
        assert_eq!(battery_icon(50, true), Icon::BATTERY_ANDROID_FRAME_BOLT);
        assert_eq!(battery_icon(100, true), Icon::BATTERY_ANDROID_FRAME_BOLT);
    }

    /// The charge has eight pictures: 0 and FULL take 0.5 share each, and 1~6 take 1 share each (1 share = 100/7%).
    #[test]
    fn the_battery_icon_climbs_eight_steps_and_the_last_one_is_full() {
        for (percent, icon) in [
            (0, Icon::BATTERY_ANDROID_0),
            (7, Icon::BATTERY_ANDROID_0),
            (8, Icon::BATTERY_ANDROID_1),
            (21, Icon::BATTERY_ANDROID_1),
            (22, Icon::BATTERY_ANDROID_2),
            (35, Icon::BATTERY_ANDROID_2),
            (36, Icon::BATTERY_ANDROID_3),
            (49, Icon::BATTERY_ANDROID_3),
            (50, Icon::BATTERY_ANDROID_4),
            (64, Icon::BATTERY_ANDROID_4),
            (65, Icon::BATTERY_ANDROID_5),
            (78, Icon::BATTERY_ANDROID_5),
            (79, Icon::BATTERY_ANDROID_6),
            (92, Icon::BATTERY_ANDROID_6),
            (93, Icon::BATTERY_ANDROID_FULL),
            (100, Icon::BATTERY_ANDROID_FULL),
            (255, Icon::BATTERY_ANDROID_FULL),
        ] {
            assert_eq!(battery_icon(percent, false), icon, "{percent}%");
        }
    }

    /// A fuller battery is never drawn as a lower picture.
    ///
    /// The whole hundred percent, in order, which is what says the arithmetic in [`battery_icon`]
    /// does not fold back on itself somewhere between the boundaries above.
    ///
    /// The comparison is where the picture sits in [`BATTERY`], not the `Icon`s' own ordering: an
    /// `Icon` orders by its code point, and the code points of a battery filling up run *backwards*
    /// (`_0` is U+F30D and `_full` is U+F304), so `>` on the two would say the climb descends.
    #[test]
    fn a_fuller_battery_is_never_a_lower_picture() {
        let step = |percent| {
            let icon = battery_icon(percent, false);

            BATTERY
                .iter()
                .position(|candidate| *candidate == icon)
                .expect("every picture of a charge is one of the eight")
        };

        let mut last = step(0);

        for percent in 0..=100u8 {
            assert!(
                step(percent) >= last,
                "{percent}% draws lower than {}%",
                percent - 1
            );

            last = step(percent);
        }
    }

    #[test]
    fn status_view_builds_for_both_themes() {
        let bg_icons = [Icon::COUNTER_0, Icon::TERMINAL];
        let _dark = view("12:00", 80, false, 3, &bg_icons, ThemeMode::Dark);
        let _light = view("12:00", 80, true, 3, &bg_icons, ThemeMode::Light);
    }
}
