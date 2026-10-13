//! Everything this page draws: the header, the reading, the days ahead, and the cards a fault goes in.
//!
//! Two cards, because the forecast is two questions at two distances: what it is like *now* is the
//! number somebody reads from across a room, and the days ahead are read a row at a time. A page that
//! put them in one card would be a page whose most important number is the same size as tomorrow's.
//!
//! # The reading is one big number, and it is not a decoration
//!
//! The temperature is drawn twice the base size, in the page's own colour, with the condition's glyph
//! beside it. That is the whole of the design's opinion: a forecast is a temperature somebody is
//! deciding about, and the humidity, the wind and the days are what they look at *after*.

use iced::widget::{button, container, row, scrollable, text, Column, Space};
use iced::{Alignment, Border, Color, Element, Length, Shadow};

use pomelo_material_symbols::{self as icons, Icon};
use pomelo_widgets::preferences::ThemeMode;

use crate::api::{Condition, Day, Forecast};
use crate::style::{self, Sizes};
use crate::{Message, Status, Weather, Words};

/// The page.
pub(crate) fn page(app: &Weather) -> Element<'_, Message> {
    let mode = app.preferences.theme;
    let sizes = Sizes::of(app.preferences.font_tier);
    let words = Words::of(app.preferences.language);

    // One card, whichever of the four states this is: a page is never drawing a forecast and a fault
    // at once, because there is no such thing — the state *is* what happened to the request.
    let mut cards: Vec<Element<'_, Message>> = Vec::new();

    match &app.status {
        Status::Loading => cards.push(message(words.loading, sizes, mode)),
        Status::Failed(fault) => cards.push(trouble(
            words.fault(*fault),
            words.fault_hint(*fault),
            sizes,
            mode,
        )),
        Status::Unreadable => cards.push(trouble(
            words.unreadable,
            words.unreadable_hint,
            sizes,
            mode,
        )),
        Status::Ready(forecast) => {
            cards.push(now_card(forecast, &words, sizes, mode));

            // An answer with no days in it is still a reading, and the card for it is still worth
            // drawing: the current conditions are the part somebody opened the page for.
            if !forecast.days.is_empty() {
                cards.push(days_card(forecast, &words, sizes, mode));
            }
        }
    }

    let page = Column::with_children(vec![
        header(app, &words, sizes, mode),
        scrollable(
            Column::with_children(cards)
                .spacing(style::GAP)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .into(),
    ])
    .spacing(style::GAP)
    .padding(style::MARGIN)
    .width(Length::Fill)
    .height(Length::Fill);

    // The panel is 480 px square and a desktop window is not. The page keeps its width and the space
    // either side of it is the surface's own, which is what [`style::PANEL`] is for: a card stretched
    // across a monitor is a different card from the one the rest of this project designs against.
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

/// The head of the page: what this is, where it is for, and the button to ask again.
///
/// The subtitle is the *place* rather than the state of the request, because the state is what the
/// cards below are for: a header that said "loading" would be saying it twice, and a header that said
/// it while a card said it too is a page with two answers to one question.
fn header<'a>(
    app: &'a Weather,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let mut heading: Vec<Element<'a, Message>> = vec![
        text(words.title).size(sizes.text).color(style::ink(theme)).into(),
    ];

    // The subtitle is drawn only once something has named a place. Before the locator has answered
    // there is no place to name, and a line with the city this page used to hard-code in it would be a
    // header claiming a forecast for somewhere the reader is not — the page would rather say less than
    // say that.
    if !app.location.name.is_empty() {
        heading.push(
            text(app.location.name.as_str())
                .size(sizes.small)
                .color(style::muted(theme))
                .into(),
        );
    }

    let heading = Column::with_children(heading).spacing(2.0);

    let refresh = button(
        row![
            text(Icon::REFRESH.glyph())
                .font(icons::font())
                .size(sizes.small)
                .color(style::ink(theme)),
            text(words.refresh).size(sizes.small).color(style::ink(theme)),
        ]
        .spacing(4.0)
        .align_y(Alignment::Center),
    )
    // Nothing to press while a request is on its way: the answer that is coming is the one this
    // button would ask for. Turning it off is also what keeps the page off the `Busy` arm of
    // `Weather::refresh` — a second request is not a thing a person can start.
    .on_press_maybe((!app.waiting()).then_some(Message::Refresh))
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
        text(Icon::LOCATION_ON.glyph())
            .font(icons::font())
            .size(sizes.reading)
            .color(style::accent()),
        heading,
        Space::new().width(Length::Fill),
        refresh,
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center)
    .into()
}

/// What it is like now: the temperature, what the sky is doing, and the two readings beside them.
fn now_card<'a>(
    forecast: &Forecast,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let current = &forecast.current;

    let reading: Element<'a, Message> = Column::with_children(vec![
        text(temperature(current.temperature_c))
            .size(sizes.reading)
            .color(style::accent())
            .into(),
        text(words.condition(current.condition))
            .size(sizes.text)
            .color(style::ink(theme))
            .into(),
    ])
    .spacing(2.0)
    .into();

    let readings: Element<'a, Message> = Column::with_children(vec![
        reading_line(
            Icon::WATER_DROP,
            words.humidity,
            format!("{:.0}%", current.humidity_percent),
            sizes,
            theme,
        ),
        reading_line(
            Icon::AIR,
            words.wind,
            format!("{:.0} km/h", current.wind_kmh),
            sizes,
            theme,
        ),
    ])
    .spacing(4.0)
    .align_x(Alignment::End)
    .into();

    let body = Column::with_children(vec![
        card_row(Icon::THERMOSTAT, words.now, words.source, sizes, theme),
        row![
            text(condition_icon(current.condition).glyph())
                .font(icons::font())
                .size(sizes.glyph)
                .color(style::accent()),
            reading,
            Space::new().width(Length::Fill),
            readings,
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center)
        .into(),
    ])
    .spacing(style::GAP)
    .into();

    card(body, theme)
}

/// The days ahead, one row each, today first.
fn days_card<'a>(
    forecast: &Forecast,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = vec![card_row(
        Icon::CALENDAR_VIEW_DAY,
        words.days,
        "",
        sizes,
        theme,
    )];

    rows.extend(
        forecast
            .days
            .iter()
            .enumerate()
            .map(|(at, day)| day_row(at, day, words, sizes, theme)),
    );

    card(
        Column::with_children(rows)
            .spacing(style::GAP / 2.0)
            .width(Length::Fill)
            .into(),
        theme,
    )
}

/// One day: which day it is, what the sky is doing, and the two numbers that matter about it.
///
/// The two temperatures are in the two colours of a temperature ([`style::high`], [`style::low`])
/// rather than in the ink of the text, because down a list of five rows the *numbers* are what is
/// being compared: which day is warmer than which is a question about the right-hand column, and
/// colouring the two ends of a day's range is what makes that column readable without reading it.
fn day_row<'a>(
    at: usize,
    day: &Day,
    words: &Words,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    row![
        // The label's column is fixed so that five rows line up under one another: a row whose label
        // was as wide as its word would start its weather a little further right than its neighbours.
        container(
            text(day_label(at, &day.date, words))
                .size(sizes.text)
                .color(style::ink(theme))
        )
        .width(Length::Fixed(style::DAY_LABEL)),
        text(condition_icon(day.condition).glyph())
            .font(icons::font())
            .size(sizes.text)
            .color(style::muted(theme)),
        // And the condition's column is fixed for the same reason, doubled: the names are not the same
        // length in either language, so without it the two temperatures would wander down the card.
        container(
            text(words.condition(day.condition))
                .size(sizes.text)
                .color(style::ink(theme))
        )
        .width(Length::Fixed(style::DAY_NAME)),
        Space::new().width(Length::Fill),
        text(temperature(day.high_c))
            .size(sizes.text)
            .color(style::high()),
        text(temperature(day.low_c))
            .size(sizes.text)
            .color(style::low()),
    ]
    .spacing(style::GAP / 2.0)
    .align_y(Alignment::Center)
    .into()
}

/// One of the two readings beside the temperature.
fn reading_line<'a>(
    icon: Icon,
    label: &'static str,
    value: String,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    row![
        text(icon.glyph())
            .font(icons::font())
            .size(sizes.small)
            .color(style::muted(theme)),
        text(label).size(sizes.small).color(style::muted(theme)),
        text(value).size(sizes.small).color(style::ink(theme)),
    ]
    .spacing(4.0)
    .align_y(Alignment::Center)
    .into()
}

/// A card's heading: its icon, its title, and a hint about what the drawing is.
///
/// Both strings are `'static` — every word on the page is — which is what lets the heading be built
/// from a [`Words`] that is a local in the caller.
fn card_row<'a>(
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

/// A line of text in a card of its own: what the page says when there is nothing to draw.
///
/// The same card with nothing pressing about it — this is "waiting", which is not a problem.
fn message<'a>(line: &'static str, sizes: Sizes, theme: ThemeMode) -> Element<'a, Message> {
    card(
        text(line).size(sizes.text).color(style::muted(theme)).into(),
        theme,
    )
}

/// A fault, and what to do about it.
///
/// Two lines, and the second one is why this is not [`message`]: the page can tell the difference
/// between a box that is off the network and a clock that was never set, and a person who is told
/// *which* of the two to go and fix. A card that said only "failed" would be a card that sent them to
/// the Wi-Fi settings for a clock problem.
fn trouble<'a>(
    what: &'static str,
    then: &'static str,
    sizes: Sizes,
    theme: ThemeMode,
) -> Element<'a, Message> {
    let lines: Vec<Element<'a, Message>> = vec![
        text(what).size(sizes.text).color(style::ink(theme)).into(),
        text(then).size(sizes.small).color(style::muted(theme)).into(),
    ];

    let body = row![
        text(Icon::CLOUD_OFF.glyph())
            .font(icons::font())
            .size(sizes.text)
            .color(style::bad()),
        Column::with_children(lines).spacing(2.0),
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    card(body.into(), theme)
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

/// A temperature, as the page spells it: a whole number and a degree.
///
/// Rounded rather than shown to a decimal, because the number this page has is a model's answer for a
/// grid square a kilometre wide: `18.4°` claims a precision about a person's own street that nothing
/// behind it has.
fn temperature(celsius: f32) -> String {
    format!("{:.0}°", celsius)
}

/// Which day a row is about.
///
/// The first two have names and the rest have their date — and no weekday, deliberately. Turning a date
/// into a weekday is calendar arithmetic, and in this project that arithmetic lives in exactly one
/// place (the clock page's `time` module) with its own tests: a second copy here, to put "Wednesday" on
/// a row, would be a second thing to keep right for one word. The date the service sent is a fact the
/// page already has, and it says which day the row is about.
pub(crate) fn day_label(at: usize, date: &str, words: &Words) -> String {
    match at {
        0 => words.today.to_string(),
        1 => words.tomorrow.to_string(),
        _ => short_date(date),
    }
}

/// `2026-10-14` as `10-14`, which is the part of a date a forecast is about.
///
/// Cut only when the date *is* the shape the service documents (`YYYY-MM-DD`), because anything else
/// cut at its tenth byte is a string the page made up: `Wednesday` would come out as `sday`, and a
/// wrong label is worse than a long one. A string that is not that shape is drawn as it came — the
/// page's own bytes are a fact, and this function is not in the business of inventing dates.
fn short_date(date: &str) -> String {
    let is_a_date = date.len() >= 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-';

    // `get` rather than a slice, so that a date whose bytes are not boundaries at 5 and 10 — a
    // multi-byte character where a month should be — is drawn as it came rather than panicking.
    if is_a_date {
        if let Some(month_and_day) = date.get(5..10) {
            return month_and_day.to_string();
        }
    }

    date.to_string()
}

/// The glyph for what the sky is doing.
///
/// The three rain glyphs this icon set has, in the order of how much water each of the three rain
/// conditions usually brings — drizzle, rain, showers — because that is the whole of the difference a
/// glyph can carry: *which* of the three it is is what the name beside it says.
pub(crate) fn condition_icon(condition: Condition) -> Icon {
    match condition {
        Condition::Clear => Icon::SUNNY,
        Condition::PartlyCloudy => Icon::PARTLY_CLOUDY_DAY,
        Condition::Overcast => Icon::CLOUDY,
        Condition::Fog => Icon::FOGGY,
        Condition::Drizzle => Icon::RAINY_LIGHT,
        Condition::Rain => Icon::RAINY,
        Condition::Showers => Icon::RAINY_HEAVY,
        // Freezing rain is ice, and this set has no glyph for it: the cold warning is the closest
        // thing to what it *is*, and a snowflake would say it was snowing.
        Condition::FreezingRain => Icon::SEVERE_COLD,
        Condition::Snow => Icon::SNOWING,
        Condition::Thunderstorm => Icon::THUNDERSTORM,
        // A code from a revision of the table this page does not know. A question mark says so, and
        // says it better than the nearest weather glyph would: the nearest one would be a lie about
        // the sky, and this is a lie about nothing.
        Condition::Unknown => Icon::QUESTION_MARK,
    }
}
