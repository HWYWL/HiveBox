//! Everything this page draws: the header, the advice, the list, and the card the two grids sit in.
//!
//! Three cards rather than one page of numbers, because the three answer three different questions at
//! three different speeds: the advice is *the answer* (one number, read from across a desk), the chart
//! and the map are the evidence for it (looked at — [`crate::plot`]), and the list is the detail (read,
//! a row at a time).

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, container, row, scrollable, text, Column, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow};

use pomelo_hal::{signal_bars, ApInfo, ScanState};
use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::plot;
use crate::style::{self, Sizes};
use crate::{Message, WifiMap, Words};

/// The page.
pub(crate) fn page(app: &WifiMap) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = Sizes::of(app.preferences.font_tier);
    let words = Words::of(app.preferences.language);

    // Nothing to chart: either the radio would not start, or the air is empty. Both are one line in a
    // card — an empty chart with thirteen zero-height columns would be a page that looked like a
    // failure rather than a page reporting a fact.
    let quiet = match app.state {
        ScanState::Error => Some(words.failed),
        _ if app.spectrum.networks() == 0 => Some(words.empty),
        _ => None,
    };

    let mut cards: Vec<Element<'_, Message>> = Vec::new();

    match quiet {
        Some(line) => cards.push(message(line, sizes, mode)),
        None => {
            cards.push(advice(app, &words, sizes, mode));
            cards.push(plot::spectrum_card(app, &words, sizes, mode));
            cards.push(list_card(app, &words, sizes, mode));
        }
    }

    let page = Column::with_children(vec![
        header(app, &words, sizes, mode),
        // No scrollbar: it is drawn over the right edge of the cards rather than beside them — over
        // the chart's last channel and the list's signal bars — and its rail has no corner to it, so
        // it squares off the right side of every card and runs the last ten pixels to the edge of the
        // panel, which reads as content that does not fit. The panel has a finger and not a pointer.
        scrollable(
            Column::with_children(cards)
                .spacing(style::GAP)
                .width(Length::Fill),
        )
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .width(Length::Fill)
        .into(),
    ])
    .spacing(style::GAP)
    .padding(style::MARGIN)
    .width(Length::Fill)
    .height(Length::Fill);

    // The panel is 480 px square and a desktop window is not. The page keeps its width and the space
    // either side of it is the surface's own, which is what [`style::PANEL`] is for: a chart stretched
    // across a monitor is a different chart from the one the numbers beside it describe.
    container(
        container(page)
            .max_width(style::PANEL)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(style::surface(mode).into()),
        ..container::Style::default()
    })
    .into()
}

/// The head of the page: what this is, how the scan is going, and the button to look again.
///
/// `words` is borrowed for no longer than the call and `'a` is the element's own, the way the rest of
/// these helpers are written: every string a [`Words`] holds is `'static`, so nothing on the page is
/// tied to the local the words were built into.
fn header<'a>(app: &WifiMap, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let status = match app.state {
        ScanState::Scanning => words.scanning.to_string(),
        ScanState::Error => words.failed.to_string(),
        _ => words.found(app.spectrum.networks()),
    };

    let heading = Column::with_children(vec![
        text(words.title).size(sizes.text).color(style::ink(theme)).into(),
        text(status).size(sizes.small).color(style::muted(theme)).into(),
    ])
    .spacing(2.0);

    let rescan = button(
        row![
            text(Icon::REFRESH.glyph())
                .font(icons::font())
                .size(sizes.small)
                .color(style::ink(theme)),
            text(words.rescan).size(sizes.small).color(style::ink(theme)),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    )
    .on_press(Message::Rescan)
    .padding([6.0, 10.0])
    .style(move |_theme, status| {
        let fill = if is_pressed(status) {
            style::accent()
        } else {
            style::card(theme)
        };

        button_style(fill, style::ink(theme), style::RADIUS / 2.0)
    });

    row![
        text(Icon::WIFI_FIND.glyph())
            .font(icons::font())
            .size(sizes.reading)
            .color(style::accent()),
        heading,
        Space::new().width(Length::Fill),
        rescan,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center)
    .into()
}

/// The one number: the channel to use, and the rule it came out of.
fn advice<'a>(app: &WifiMap, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let body = row![
        text(Icon::ROUTER.glyph())
            .font(icons::font())
            .size(sizes.text)
            .color(style::muted(theme)),
        text(words.advice).size(sizes.small).color(style::muted(theme)),
        text(app.spectrum.recommended().to_string())
            .size(sizes.reading)
            .color(style::accent()),
        Space::new().width(Length::Fill),
        text(words.advice_note).size(sizes.tiny).color(style::muted(theme)),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center);

    card(body.into(), theme)
}

/// Every network the scan found, loudest first, and the ones the chart has no column for under them.
fn list_card<'a>(app: &WifiMap, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = app
        .spectrum
        .loudest(app.aps())
        .into_iter()
        .map(|ap| network_row(&ap, words, sizes, theme))
        .collect();

    if !app.spectrum.others().is_empty() {
        rows.push(
            text(words.others)
                .size(sizes.small)
                .color(style::muted(theme))
                .into(),
        );

        rows.extend(
            app.spectrum
                .others()
                .iter()
                .map(|ap| network_row(ap, words, sizes, theme)),
        );
    }

    Column::with_children(vec![
        card_row(Icon::WIFI, words.list, "", sizes, theme),
        Column::with_children(rows).spacing(style::GAP / 2.0).into(),
    ])
    .spacing(style::GAP / 2.0)
    .into()
}

/// One row of the list: how loud it is, what it is called, and where it is.
///
/// `ap`'s lifetime is its own: everything the row draws from it is copied out of it, so a row can be
/// built from a network owned by the iteration rather than by the app.
fn network_row<'a>(ap: &ApInfo, words: &Words, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    let name = if ap.ssid.is_empty() {
        words.hidden.to_string()
    } else {
        ap.ssid.clone()
    };

    row![
        text(signal_icon(ap))
            .font(icons::font())
            .size(sizes.text)
            .color(style::signal(ap.rssi)),
        text(name)
            .size(sizes.text)
            .color(style::ink(theme))
            .width(Length::Fill),
        text(format!("ch {}", ap.channel))
            .size(sizes.small)
            .color(style::muted(theme)),
        text(format!("{} dBm", ap.rssi))
            .size(sizes.small)
            .color(style::muted(theme)),
        text(if ap.secure {
            Icon::LOCK.glyph()
        } else {
            Icon::LOCK_OPEN.glyph()
        })
        .font(icons::font())
        .size(sizes.small)
        .color(style::muted(theme)),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .into()
}

/// A card's heading: its icon, its title, and a hint about what the drawing is.
///
/// Both strings are `'static` — every word on the page is — which is what lets the heading be built
/// from a [`Words`] that is a local in the caller.
pub(crate) fn card_row<'a>(
    icon: Icon,
    title: &'static str,
    hint: &'static str,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    row![
        text(icon.glyph())
            .font(icons::font())
            .size(sizes.text)
            .color(style::muted(theme)),
        text(title).size(sizes.text).color(style::ink(theme)),
        Space::new().width(Length::Fill),
        text(hint).size(sizes.tiny).color(style::muted(theme)),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .into()
}

/// The map's left gutter and the chart's: one width, so the two grids share a left edge.
pub(crate) fn gutter<'a>(label: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(label)
        .width(Length::Fixed(style::MAP_LABEL))
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Center)
        .into()
}

/// A line of text in a card of its own: what the page says when there is nothing to draw.
fn message<'a>(line: &'static str, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    card(
        text(line).size(sizes.text).color(style::muted(theme)).into(),
        theme,
    )
}

/// A card: a slab, a radius and a padding, and whatever is put in it.
fn card<'a>(body: Element<'a, Message>, theme: ThemeMode) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding(style::GAP)
        .style(move |_theme| container::Style {
            background: Some(style::card(theme).into()),
            border: Border {
                radius: style::RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A rounded, filled rectangle: what a column's track and its fill are made of.
pub(crate) fn capsule(colour: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(colour.into()),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A button's capsule: its fill, and the ink its contents default to.
fn button_style(fill: Color, ink: Color, radius: f32) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Whether a finger is on the button.
///
/// `Pressed` and only `Pressed`. A touchscreen has no hover, and this platform leaves the pointer where
/// the finger lifted, so a button that has been touched stays hovered until something else moves the
/// pointer — which is a button that stays lit after it is let go.
fn is_pressed(status: button::Status) -> bool {
    matches!(status, button::Status::Pressed)
}

/// The glyph for how many bars a reading is worth, in the same steps as the status bar's.
fn signal_icon(ap: &ApInfo) -> &'static str {
    match signal_bars(ap.rssi) {
        0 => Icon::WIFI_OFF.glyph(),
        1 => Icon::WIFI_1_BAR.glyph(),
        2 => Icon::WIFI_2_BAR.glyph(),
        _ => Icon::WIFI.glyph(),
    }
}
