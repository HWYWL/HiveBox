//! The chart and the map: thirteen columns of the band, and where the networks sit in them.
//!
//! Both are drawn inside one card with the same left gutter, so a network on channel 6 is a square in
//! the map directly under the bar it is part of in the chart. The gutter is where the axis is written:
//! the channel numbers once at the top, and the dBm each row stands for down the side. Two cards would
//! have been two grids, and two grids that nearly line up are worse than two that obviously do not.
//!
//! The chart counts what is *heard* ([`channel::overlap`]) and the map shows only what is *there*: a
//! square on the channel a network is on, and none on the neighbours it leaks onto. That is the pair
//! worth having — the chart says how much noise a channel is under, the map says how many networks
//! would have to be dealt with — and a map that spilled either side of every network would be the chart
//! drawn twice.

use iced::widget::{container, text, Column, Row, Space};
use iced::{Alignment, Element, Length};

use pomelo_hal::ApInfo;
use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::ThemeMode;

use crate::channel::{self, Bin, Spectrum, LEVELS};
use crate::style::{self, Sizes};
use crate::ui::{capsule, card_row, gutter};
use crate::{Message, WifiMap, Words};

/// How wide the gap between the chart's columns and the map's cells is.
const CELL_GAP: f32 = 2.0;

/// The chart, and the map under it.
pub(crate) fn spectrum_card<'a>(
    app: &WifiMap,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let spectrum = &app.spectrum;
    let recommended = spectrum.recommended();

    let mut chart: Vec<Element<'a, Message>> = vec![gutter(
        text(words.channel_axis)
            .size(sizes.tiny)
            .color(style::muted(theme)),
    )];
    chart.extend(
        spectrum
            .bins()
            .iter()
            .map(|bin| column_of(bin, bin.channel == recommended, spectrum, sizes, theme)),
    );

    let chart = Row::with_children(chart)
        .spacing(CELL_GAP)
        .height(Length::Fixed(style::CHART));

    let loudest = strongest_per_channel(app.aps());

    // The map's own caption, in the chart's gutter: the two grids share a left edge, and this is what
    // says where one ends and the other begins.
    let caption = Row::with_children(vec![
        container(
            text(words.map)
                .size(sizes.small)
                .color(style::muted(theme)),
        )
        .width(Length::Fixed(style::MAP_LABEL))
        .into(),
        text(words.map_hint)
            .size(sizes.tiny)
            .color(style::muted(theme))
            .into(),
    ])
    .spacing(CELL_GAP)
    .align_y(Alignment::Center);

    let mut map: Vec<Element<'a, Message>> = vec![caption.into()];

    for level in 0..LEVELS {
        let mut cells: Vec<Element<'a, Message>> = vec![gutter(
            text(level_label(level))
                .size(sizes.tiny)
                .color(style::muted(theme)),
        )];

        cells.extend((channel::FIRST..=channel::LAST).map(|channel| {
            let signal = loudest
                .get(usize::from(channel - channel::FIRST))
                .copied()
                .flatten()
                .filter(|rssi| channel::level_of(*rssi) == level);

            cell(signal)
        }));

        map.push(
            Row::with_children(cells)
                .spacing(CELL_GAP)
                .height(Length::Fixed(style::MAP_ROW))
                .align_y(Alignment::Center)
                .into(),
        );
    }

    Column::with_children(vec![
        card_row(Icon::BAR_CHART, words.chart, words.chart_hint, sizes, theme),
        chart.into(),
        Column::with_children(map).into(),
    ])
    .spacing(style::GAP / 2.0)
    .into()
}

/// One channel of the chart: what is on it, over an empty track it fills from the bottom.
fn column_of<'a>(
    bin: &Bin,
    recommended: bool,
    spectrum: &Spectrum,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let level = spectrum.level(bin);
    let height = (bar_area(sizes) * level).max(2.0);

    // The suggestion is drawn in the page's own colour and not in a crowding colour: "use this one" is
    // not the same claim as "this one is quiet", and a channel drawn green when it is merely the best of
    // the three that can be used would be the page making the stronger claim.
    let fill = if recommended {
        style::accent()
    } else {
        style::crowd(level)
    };

    // The column inside the track fills it, so that the flexible space above the bar takes up whatever
    // the bar does not: a shrink-to-fit column would put a short bar at the *top* of its track, which is
    // a chart that says the opposite of what it means.
    let bar = container(
        Column::new()
            .push(Space::new().height(Length::Fill))
            .push(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fixed(height))
                    .style(move |_theme| capsule(fill, style::RADIUS / 3.0)),
            )
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(bar_area(sizes)))
    .style(move |_theme| capsule(style::track(theme), style::RADIUS / 3.0));

    let count = if bin.aps > 0 {
        bin.aps.to_string()
    } else {
        String::new()
    };

    Column::with_children(vec![
        text(count)
            .size(sizes.tiny)
            .color(style::muted(theme))
            .into(),
        Space::new().height(Length::Fill).into(),
        bar.into(),
        text(bin.channel.to_string())
            .size(sizes.tiny)
            .color(if recommended {
                style::accent()
            } else {
                style::muted(theme)
            })
            .into(),
    ])
    .width(Length::FillPortion(1))
    .height(Length::Fixed(style::CHART))
    .align_x(Alignment::Center)
    .spacing(2.0)
    .into()
}

/// One cell of the map's grid: a network's square, or the room for one.
///
/// The room is drawn whether or not there is a square in it: an empty cell is what a channel nobody is
/// on looks like, and a row that stopped early would read as a shorter band rather than as an empty one.
fn cell<'a>(signal: Option<i8>) -> Element<'a, Message> {
    let inside: Element<'a, Message> = match signal {
        Some(rssi) => container(Space::new())
            .width(Length::Fixed(style::DOT))
            .height(Length::Fixed(style::DOT))
            .style(move |_theme| capsule(style::signal(rssi), style::DOT / 3.0))
            .into(),
        None => Space::new().into(),
    };

    container(inside)
        .width(Length::FillPortion(1))
        .height(Length::Fixed(style::MAP_ROW))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// The strongest network *on* each of the band's channels, or `None` where none is.
fn strongest_per_channel(aps: &[ApInfo]) -> [Option<i8>; 13] {
    let mut loudest: [Option<i8>; 13] = [None; 13];

    for ap in aps.iter().filter(|ap| channel::in_band(ap.channel)) {
        let slot = &mut loudest[usize::from(ap.channel - channel::FIRST)];

        *slot = Some(match *slot {
            Some(best) => best.max(ap.rssi),
            None => ap.rssi,
        });
    }

    loudest
}

/// The best signal a row of the map stands for, as the label down its side.
///
/// The first row is everything at -40 dBm or better and says so; the rest are ten dBm bands and are
/// named by their ceiling, which is the number a person compares a reading against.
fn level_label(level: usize) -> String {
    let ceiling = -40 - 10 * level as i32;

    if level == 0 {
        format!("≥{ceiling}")
    } else {
        ceiling.to_string()
    }
}

/// How tall a column's bar may be: the chart minus the two labels above and below it.
fn bar_area(sizes: Sizes) -> f32 {
    (style::CHART - sizes.tiny * 3.0).max(style::DOT * 2.0)
}
