//! The battery page, drawn.
//!
//! Three cards and a banner, and all four read the app's own state rather than the board: [`PowerLog`]
//! has already taken the reading and written it down, so what is here is a function of a value. That is
//! what lets a test build a page out of a curve written by hand and look at what came out.
//!
//! # The curve, without a canvas
//!
//! A discharge curve is a column per sample: a full-height column with a gap on top of it for the part
//! of the charge that is gone. The gap is a `Length::FillPortion` and the charge is the rest of the
//! same hundred, so the two always add up to the column's height and the curve cannot disagree with the
//! percentage printed above it by a rounding step — which is the trick the overview's bars are made of,
//! turned on its side.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, container, row, scrollable, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow};

use pomelo_material_symbols::Icon;
use pomelo_widgets::preferences::{Language, ThemeMode};

use crate::sample::{span, Sample};
use crate::style::{self, Sizes};
use crate::{format, Message, PowerLog};

/// The page.
pub fn view(app: &PowerLog) -> Element<'_, Message> {
    let mode = app.preferences().theme;
    let sizes = Sizes::new();
    let words = Words::of(app.preferences().language);

    let mut cards: Vec<Element<'_, Message>> = Vec::new();

    if let Some(banner) = heat_banner(app, sizes, mode, &words) {
        cards.push(banner);
    }

    cards.push(charge_card(app, sizes, mode, &words));
    cards.push(curve_card(app, sizes, mode, &words));
    cards.push(log_card(app, sizes, mode, &words));

    let page = Column::with_children(vec![
        header(sizes, mode, &words),
        // No scrollbar. It would be drawn over the right edge of the cards rather than beside them,
        // and its rail is the one thing on the page without a corner: it would square off the right
        // side of every card and run the strip the last ten pixels to the edge of the panel, which
        // reads as content that does not fit. The panel has a finger and not a pointer, and a bar a
        // hair wide is something to look at rather than something to grab, as on the track list.
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

    // The page is as wide as the panel it was drawn for, and the space either side of it on a wider
    // window is the surface's own. That is what [`style::PANEL`] is for: a curve stretched across a
    // monitor is a different chart from the one the numbers beside it describe.
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

/// The title, and the one control on the page.
fn header<'a>(sizes: Sizes, mode: ThemeMode, words: &Words) -> Element<'a, Message> {
    row![
        text(words.title).size(sizes.text).color(style::ink(mode)),
        Space::new().width(Length::Fill),
        save_button(sizes, mode, words),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// The button that takes a sample there and then.
///
/// The page writes one by itself when it opens and one an hour after that, and an hour is a long time
/// to wait to find out whether the disk is being written to at all — which is the whole reason this is
/// a button and not just the clock's business.
fn save_button<'a>(sizes: Sizes, mode: ThemeMode, words: &Words) -> Element<'a, Message> {
    button(
        row![
            text(Icon::SAVE.glyph())
                .font(pomelo_material_symbols::font())
                .size(sizes.small)
                .color(style::accent()),
            text(words.save).size(sizes.small).color(style::muted(mode)),
        ]
        .spacing(style::GAP / 2.0)
        .align_y(Alignment::Center),
    )
    .padding([6.0, 12.0])
    .on_press(Message::LogNow)
    .style(move |_theme, status| {
        let background = match status {
            button::Status::Pressed | button::Status::Hovered => style::card(mode),
            _ => Color::TRANSPARENT,
        };

        button::Style {
            background: Some(background.into()),
            text_color: style::ink(mode),
            border: Border {
                radius: style::RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .into()
}

/// The banner that says the chip is warm, and the louder one that says it is hot.
///
/// It is drawn *above* the reading rather than inside a card, because it is not a measurement to be
/// read at leisure: it is the one thing on this page that is a warning, and a warning under a curve is
/// a warning somebody scrolls past.
fn heat_banner<'a>(
    app: &'a PowerLog,
    sizes: Sizes,
    mode: ThemeMode,
    words: &Words,
) -> Option<Element<'a, Message>> {
    if !app.warm() {
        return None;
    }

    let celsius = app.temperature()?;
    let (label, tint) = if app.hot() {
        (words.hot, style::bad())
    } else {
        (words.warm, style::warn())
    };

    let body = row![
        text(Icon::THERMOSTAT.glyph())
            .font(pomelo_material_symbols::font())
            .size(sizes.text)
            .color(tint),
        text(format!("{label} · {}", format::degrees(celsius)))
            .size(sizes.small)
            .color(style::ink(mode)),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center);

    Some(
        container(body)
            .width(Length::Fill)
            .padding(style::GAP)
            .style(move |_theme| container::Style {
                background: Some(tint.scale_alpha(0.18).into()),
                border: Border {
                    radius: style::RADIUS.into(),
                    width: 1.0,
                    color: tint,
                },
                ..container::Style::default()
            })
            .into(),
    )
}

/// The charge: the one number worth reading from across a desk.
fn charge_card<'a>(
    app: &'a PowerLog,
    sizes: Sizes,
    mode: ThemeMode,
    words: &Words,
) -> Element<'a, Message> {
    let Some(now) = app.latest() else {
        let body = text(words.unknown).size(sizes.small).color(style::muted(mode));

        return card(
            words.charge,
            Icon::BATTERY_ALERT,
            style::bad(),
            body.into(),
            sizes,
            mode,
        );
    };

    let tint = style::charge(now.percent as f32);

    let body = Column::with_children(vec![
        row![
            text(format::percent(now.percent as f32))
                .size(sizes.reading)
                .color(tint),
            Space::new().width(Length::Fill),
            state_chip(now.charging, sizes, mode, words),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into(),
        bar(format::fraction(now.percent as f32), tint, mode),
        facts_row(now, sizes, mode),
    ])
    .spacing(style::GAP / 2.0)
    .width(Length::Fill);

    card(words.charge, Icon::BATTERY_FULL, tint, body.into(), sizes, mode)
}

/// Whether the cell is on a charger, as a glyph and a word.
fn state_chip<'a>(
    charging: bool,
    sizes: Sizes,
    mode: ThemeMode,
    words: &Words,
) -> Element<'a, Message> {
    let (icon, label, tint) = if charging {
        (Icon::BOLT, words.charging, style::accent())
    } else {
        (Icon::BATTERY_FULL, words.on_battery, style::muted(mode))
    };

    row![
        text(icon.glyph())
            .font(pomelo_material_symbols::font())
            .size(sizes.text)
            .color(tint),
        text(label).size(sizes.small).color(tint),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .into()
}

/// The curve, and the two numbers read off it.
fn curve_card<'a>(
    app: &'a PowerLog,
    sizes: Sizes,
    mode: ThemeMode,
    words: &Words,
) -> Element<'a, Message> {
    let tint = match app.latest() {
        Some(now) => style::charge(now.percent as f32),
        None => style::muted(mode),
    };

    // The rate and the estimate come from the same slope and are shown together: the second is the
    // first carried forward, and a page that showed only the answer would be asking to be trusted
    // rather than read.
    let rate = match app.rate() {
        Some(rate) => format::rate(rate),
        None => format::missing(),
    };

    let estimate = match app.estimate() {
        Some(left) => (format::duration(left), style::good()),
        None if app.latest().is_some_and(|sample| sample.charging) => {
            (String::from(words.charging), style::accent())
        }
        None => (format::missing(), style::muted(mode)),
    };

    let body = Column::with_children(vec![
        chart(app.samples(), tint, sizes, mode),
        row![
            text(words.window).size(sizes.small).color(style::muted(mode)),
            Space::new().width(Length::Fill),
            text(format::duration(span(app.samples())))
                .size(sizes.small)
                .color(style::ink(mode)),
        ]
        .width(Length::Fill)
        .into(),
        row![
            text(words.left).size(sizes.small).color(style::muted(mode)),
            Space::new().width(Length::Fill),
            text(estimate.0).size(sizes.text).color(estimate.1),
        ]
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .into(),
        row![
            text(words.rate).size(sizes.small).color(style::muted(mode)),
            Space::new().width(Length::Fill),
            text(rate).size(sizes.small).color(style::ink(mode)),
        ]
        .width(Length::Fill)
        .into(),
    ])
    .spacing(style::GAP / 2.0)
    .width(Length::Fill);

    card(words.curve, Icon::SHOW_CHART, tint, body.into(), sizes, mode)
}

/// The discharge curve: a column per sample, as tall as the charge it was taken at.
fn chart<'a>(
    samples: &'a [Sample],
    tint: Color,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    if samples.len() < 2 {
        return container(
            text(format::missing())
                .size(sizes.small)
                .color(style::muted(mode)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(style::CHART))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme| capsule(style::track(mode), style::RADIUS))
        .into();
    }

    let columns: Vec<Element<'a, Message>> = samples
        .iter()
        .map(|sample| {
            let charge = sample.percent.min(100) as u16;

            let mut column = Column::new().width(Length::Fill).height(Length::Fill);

            if charge < 100 {
                column = column.push(Space::new().height(Length::FillPortion(100 - charge)));
            }
            if charge > 0 {
                column = column.push(
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::FillPortion(charge))
                        .style(move |_theme| capsule(tint, 1.0)),
                );
            }

            column.into()
        })
        .collect();

    container(
        Row::with_children(columns)
            .spacing(style::CHART_GAP)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(style::CHART))
    .style(move |_theme| capsule(style::track(mode), style::RADIUS))
    .into()
}

/// The hour's record: how many lines it has, where it is, and whether the last write worked.
fn log_card<'a>(
    app: &'a PowerLog,
    sizes: Sizes,
    mode: ThemeMode,
    words: &Words,
) -> Element<'a, Message> {
    let mut lines: Vec<Element<'a, Message>> = vec![
        labelled(words.lines, app.logged().to_string(), sizes, mode),
        labelled(words.every, format::duration(crate::LOG_EVERY), sizes, mode),
        labelled(words.location, file_name(app.path()), sizes, mode),
    ];

    if let Some(fault) = app.fault() {
        lines.push(fault_line(fault, sizes));
    }

    let body = Column::with_children(lines)
        .spacing(style::GAP / 2.0)
        .width(Length::Fill);

    card(words.record, Icon::HISTORY, style::accent(), body.into(), sizes, mode)
}

/// A card: a title row with a glyph, then whatever the caller built.
fn card<'a>(
    title: &'static str,
    icon: Icon,
    tint: Color,
    body: Element<'a, Message>,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    container(
        Column::with_children(vec![
            row![
                text(icon.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(sizes.text)
                    .color(tint),
                text(title).size(sizes.text).color(style::ink(mode)),
            ]
            .spacing(style::GAP / 2.0)
            .align_y(Alignment::Center)
            .into(),
            body,
        ])
        .spacing(style::GAP)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(style::GAP + 2.0)
    .style(move |_theme| container::Style {
        background: Some(style::card(mode).into()),
        border: Border {
            radius: style::RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// A name and its value, the value on the side a column of them lines up on.
fn labelled<'a>(
    label: &'static str,
    value: String,
    sizes: Sizes,
    mode: ThemeMode,
) -> Element<'a, Message> {
    row![
        text(label).size(sizes.small).color(style::muted(mode)),
        Space::new().width(Length::Fill),
        text(value).size(sizes.small).color(style::ink(mode)),
    ]
    .width(Length::Fill)
    .into()
}

/// The facts about a reading, as one line of `·`-separated numbers.
///
/// One line and not three rows, because the two are read together or not at all: a voltage with no
/// temperature is not a fact about a battery, it is a fact about a reading.
///
/// The temperature is the one of them that is *coloured* rather than merely written, because it is the
/// only one with a band: [`style::heat`] is the same scale the banner uses, so the number beside the
/// charge says the same thing the banner above it would.
fn facts_row<'a>(sample: Sample, sizes: Sizes, mode: ThemeMode) -> Element<'a, Message> {
    let mut facts: Vec<Element<'a, Message>> = Vec::new();

    if let Some(millivolts) = sample.voltage_mv {
        facts.push(
            text(format::volts(millivolts))
                .size(sizes.small)
                .color(style::muted(mode))
                .into(),
        );
    }

    if let Some(celsius) = sample.temperature_c {
        if !facts.is_empty() {
            facts.push(
                text("·")
                    .size(sizes.small)
                    .color(style::muted(mode))
                    .into(),
            );
        }

        facts.push(
            text(format::degrees(celsius))
                .size(sizes.small)
                .color(style::heat(celsius))
                .into(),
        );
    }

    if facts.is_empty() {
        return text(format::missing())
            .size(sizes.small)
            .color(style::muted(mode))
            .into();
    }

    Row::with_children(facts)
        .spacing(style::GAP / 2.0)
        .align_y(Alignment::Center)
        .into()
}

/// What went wrong with the log, in the colour the page uses for what went wrong.
fn fault_line<'a>(fault: &'a str, sizes: Sizes) -> Element<'a, Message> {
    text(fault)
        .size(sizes.small)
        .color(style::bad())
        .width(Length::Fill)
        .into()
}

/// The name of the file at the end of a path, for the log card.
fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// A level bar: a fill inside a track, both of them capsules.
fn bar<'a>(share: f32, fill: Color, mode: ThemeMode) -> Element<'a, Message> {
    let portion = (share.clamp(0.0, 1.0) * 1000.0).round() as u16;

    let mut contents = Row::new().width(Length::Fill).height(Length::Fill);

    if portion > 0 {
        contents = contents.push(
            container(Space::new())
                .width(Length::FillPortion(portion))
                .height(Length::Fill)
                .style(move |_theme| capsule(fill, style::BAR_RADIUS)),
        );
    }
    if portion < 1000 {
        contents = contents.push(Space::new().width(Length::FillPortion(1000 - portion)));
    }

    container(contents)
        .width(Length::Fill)
        .height(Length::Fixed(style::BAR))
        .style(move |_theme| capsule(style::track(mode), style::BAR_RADIUS))
        .into()
}

/// A capsule of one colour, which is the whole of this page's vocabulary for a filled shape.
fn capsule(colour: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(colour.into()),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Every word this page says, in the two languages the box is set to.
struct Words {
    title: &'static str,
    charge: &'static str,
    curve: &'static str,
    record: &'static str,
    left: &'static str,
    rate: &'static str,
    window: &'static str,
    charging: &'static str,
    on_battery: &'static str,
    lines: &'static str,
    every: &'static str,
    location: &'static str,
    hot: &'static str,
    warm: &'static str,
    save: &'static str,
    unknown: &'static str,
}

impl Words {
    /// The page's words, in one language.
    ///
    /// Both columns are written here rather than looked up, for the reason the clock's are: the page's
    /// vocabulary is short and fixed, and a table with two columns is easier to keep honest than a file
    /// per language.
    fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                title: "电池",
                charge: "电量",
                curve: "放电曲线",
                record: "日志",
                left: "预计剩余",
                rate: "放电速度",
                window: "曲线窗口",
                charging: "充电中",
                on_battery: "电池供电",
                lines: "已记录",
                every: "记录间隔",
                location: "文件",
                hot: "芯片过热",
                warm: "芯片偏热",
                save: "立即记录",
                unknown: "读不到电量",
            },
            Language::English => Self {
                title: "Battery",
                charge: "Charge",
                curve: "Discharge",
                record: "Log",
                left: "Time left",
                rate: "Drain rate",
                window: "Window",
                charging: "Charging",
                on_battery: "On battery",
                lines: "Recorded",
                every: "Every",
                location: "File",
                hot: "Chip is hot",
                warm: "Chip is warm",
                save: "Log now",
                unknown: "No reading",
            },
        }
    }
}
