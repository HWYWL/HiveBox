//! Layout helpers shared by every page: `page`, `title`, `body`, `card_surface`,
//! `separator`, `detail_rows`, `detail_card`, `switch_row`, `usage_bar`, `toggle`,
//! `action_row`, `notice_card`, `row_style`, `toggle_style`.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, container, rule, scrollable, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Element, Length, Padding, Renderer, Shadow, Theme};

use crate::style;
use crate::Message;
use pomelo_material_symbols::Icon;
use pomelo_widgets::ThemeMode;

/// The element type every builder in this crate speaks.
pub(crate) type UI<'a> = Element<'a, Message>;

/// The id the page body's `scrollable` carries.
///
/// One id for every page: only one page is ever in the tree, so there is nothing to tell apart, and
/// a single name is what lets a change of section carry one [`Task`](iced::Task) that sends the new
/// page to the top. [`crate::to_top`] is where that is built.
pub(crate) const BODY: &str = "settings-body";

// =============================================================================
// Shared layout primitives
// =============================================================================

/// A page: one `scrollable` over the head and the body together.
///
/// Every page has the same shape — a heading, then blocks — so there is one function and not two.
/// The head is an element rather than a string because the two kinds of page head differently: a
/// list's is its own first line ([`title`]), a sub-page's is a card
/// ([`crate::pages::card::Header`]).
///
/// The head scrolls because a heading that stays put while its content moves is a bar, and a bar is
/// a thing that covers content. On a 480 px panel there is nothing to cover it with.
pub(crate) fn page<'a>(head: impl Into<UI<'a>>, body: impl Into<UI<'a>>) -> UI<'a> {
    scrollable(Column::with_children(vec![head.into(), body.into()]).width(Length::Fill))
        // Named, so a change of section can send it back to the top: a new page is the same node
        // in the tree, and its offset would otherwise survive. See [`BODY`].
        .id(BODY)
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The heading of a list: the list's own first line, not a bar over it.
///
/// Left-aligned, on the page's background rather than in a band of its own. It carries both gaps
/// around itself — `TITLE_TOP_GAP` above and `TITLE_GAP` below — because the body under it states
/// neither: how far a heading is from the panel's edge, and from what it heads, is the heading's
/// business.
pub(crate) fn title<'a>(title: &'static str, theme: ThemeMode) -> UI<'a> {
    container(
        text(title)
            .size(style::TITLE_FONT)
            .color(style::label_for(theme)),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: style::TITLE_TOP_GAP,
        bottom: style::TITLE_GAP,
        left: style::PAGE_MARGIN,
        right: style::PAGE_MARGIN,
    })
    .into()
}

/// A page body: the cards, `CARD_GAP` apart, with `BOTTOM_GAP` under the last, and the page's
/// margins on the sides.
///
/// Nothing at the top: the gap *above* a body belongs to whatever precedes it — a title on the list
/// ([`title`]), a head card on a sub-page ([`crate::pages::card::Header`]) — and those two are not
/// the same distance.
pub(crate) fn body<'a>(parts: Vec<UI<'a>>) -> Column<'a, Message, Theme, Renderer> {
    Column::with_children(parts)
        .spacing(style::CARD_GAP)
        .padding(Padding {
            top: 0.0,
            bottom: style::BOTTOM_GAP,
            left: style::PAGE_MARGIN,
            right: style::PAGE_MARGIN,
        })
        .width(Length::Fill)
}

/// A rounded surface, as wide as the page it sits in.
///
/// The surface and not the card: [`crate::pages::card::Card`] is what sits on it, and it is named
/// for what it holds rather than for the box.
pub(crate) fn card_surface<'a>(content: impl Into<UI<'a>>, theme: ThemeMode) -> UI<'a> {
    container(content)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(style::card_for(theme).into()),
            border: Border {
                color: style::card_border_for(theme),
                width: style::CARD_BORDER_WIDTH,
                radius: style::CARD_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The hairline between two rows of a card: iced's own `rule`, inset to the row's text.
///
/// The insets are the row's own measurements — [`style::ROW_TEXT_INSET`] is where the label starts,
/// [`style::ROW_PADDING_H`] is where the chevron ends — so the line is the same layout the row is,
/// rather than a number someone has to keep in step with it. `FillMode` is why this needs no
/// wrapper: iced's `rule` insets itself.
pub(crate) fn separator<'a>(theme: ThemeMode) -> UI<'a> {
    rule::horizontal(style::ROW_SEPARATOR)
        .style(move |_theme| rule::Style {
            color: style::separator_for(theme),
            radius: 0.0.into(),
            fill_mode: rule::FillMode::AsymmetricPadding(
                style::ROW_TEXT_INSET as u16,
                style::ROW_PADDING_H as u16,
            ),
            snap: false,
        })
        .into()
}

/// The rows of a key/value table: one line each, with hairlines between them.
pub(crate) fn detail_rows<'a>(
    rows: Vec<(&'static str, String)>,
    theme: ThemeMode,
) -> Vec<UI<'a>> {
    let last = rows.len().saturating_sub(1);
    let mut children: Vec<UI<'a>> = Vec::new();

    for (index, (key, value)) in rows.into_iter().enumerate() {
        let line = Row::with_children(vec![
            text(key)
                .size(style::DETAIL_FONT)
                .color(style::detail_key_for(theme))
                .into(),
            Space::new().width(Length::Fill).into(),
            text(value)
                .size(style::DETAIL_FONT)
                .color(style::label_for(theme))
                .into(),
        ])
        .width(Length::Fill)
        .align_y(Alignment::Center);

        children.push(
            container(line)
                .width(Length::Fill)
                .padding([style::DETAIL_PADDING_V, style::DETAIL_PADDING_H])
                .into(),
        );

        if index < last {
            children.push(separator(theme));
        }
    }

    children
}

/// A key/value table in a card.
pub(crate) fn detail_card<'a>(rows: Vec<(&'static str, String)>, theme: ThemeMode) -> UI<'a> {
    card_surface(
        Column::with_children(detail_rows(rows, theme)).width(Length::Fill),
        theme,
    )
}

/// A card whose row is a label and a switch.
///
/// A switch row's vertical padding is `ROW_PADDING_V`, the tile's: the two rows are the same row
/// with a different thing at its end, and giving them two numbers is how they came to differ.
pub(crate) fn switch_row<'a>(label: &'static str, control: UI<'a>, theme: ThemeMode) -> UI<'a> {
    let line = Row::with_children(vec![
        text(label)
            .size(style::LABEL_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new().width(Length::Fill).into(),
        control,
    ])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    card_surface(
        container(line).padding([style::ROW_PADDING_V, style::SWITCH_PADDING_H]),
        theme,
    )
}

/// The two halves of a usage bar, as the flex shares they are drawn with.
///
/// Both are at least one, and that is the whole reason this is a function rather than two lines in
/// [`usage_bar`]: this layout engine reads a share of zero as *no* share — `Length::fill_factor` is
/// zero for `FillPortion(0)`, so the child is laid out as a non-fluid one and handed every pixel
/// there is — and a bar whose fill is zero would then be drawn full. An empty bar and a full one
/// are not a rounding error apart; they are opposite answers, and a 0.0% card drew the wrong one.
///
/// One part in ten thousand is what a single share costs: the two halves divide the track between
/// them, so either can be as small as 0.01% — narrower than the track is tall, which is to say
/// invisible, which is what "none of it" is supposed to look like.
fn shares(percent: f32) -> (u16, u16) {
    /// The whole bar. A hundredth of a percent is the finest it is drawn to.
    const WHOLE: u16 = 10_000;

    let filled = ((percent.clamp(0.0, 100.0) * 100.0).round() as u16).clamp(1, WHOLE);

    (filled, (WHOLE - filled).max(1))
}

/// A card with a labelled progress bar.
///
/// `percent` — not pixels: the two halves of the bar are flex shares, so the same number
/// describes the design's 440pt card and any other width.
pub(crate) fn usage_bar<'a>(
    title: &'static str,
    value: String,
    value_color: Color,
    percent: f32,
    bar_color: Color,
    theme: ThemeMode,
) -> UI<'a> {
    let (filled_share, remaining_share) = shares(percent);

    let filled = container(Space::new().width(Length::Fill))
        .width(Length::FillPortion(filled_share))
        .height(Length::Fixed(style::BAR_HEIGHT))
        .style(move |_theme| container::Style {
            background: Some(bar_color.into()),
            border: Border {
                radius: style::BAR_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    let remaining = container(Space::new().width(Length::Fill))
        .width(Length::FillPortion(remaining_share))
        .height(Length::Fixed(style::BAR_HEIGHT));

    let track = container(
        Row::with_children(vec![filled.into(), remaining.into()])
            .width(Length::Fill)
            .height(Length::Fixed(style::BAR_HEIGHT)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(style::BAR_HEIGHT))
    .style(move |_theme| container::Style {
        background: Some(style::bar_track_for(theme).into()),
        border: Border {
            radius: style::BAR_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    });

    let heading = Row::with_children(vec![
        text(title)
            .size(style::DETAIL_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new().width(Length::Fill).into(),
        text(value)
            .size(style::DETAIL_FONT)
            .color(value_color)
            .into(),
    ])
    .width(Length::Fill);

    card_surface(
        container(
            Column::with_children(vec![
                heading.into(),
                Space::new().height(Length::Fixed(style::BAR_GAP)).into(),
                track.into(),
            ])
            .width(Length::Fill),
        )
        .padding(style::USAGE_PADDING),
        theme,
    )
}

/// An iOS switch: a 52x28 track with a 24px knob, drawn from containers.
///
/// The whole track is the button. The knob is positioned by a fill on the side it is moving
/// away from, which is what puts it 2px from the edge it rests against without hard-coding the
/// track's own width into the knob's position.
pub(crate) fn toggle<'a>(on: bool, message: Message, theme: ThemeMode) -> UI<'a> {
    let knob = container(Space::new())
        .width(Length::Fixed(style::KNOB))
        .height(Length::Fixed(style::KNOB))
        .style(|_theme| container::Style {
            background: Some(Color::WHITE.into()),
            border: Border {
                radius: style::KNOB_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    let mut parts: Vec<UI<'a>> = Vec::new();

    if on {
        parts.push(Space::new().width(Length::Fill).into());
        parts.push(knob.into());
        parts.push(
            Space::new()
                .width(Length::Fixed(style::TOGGLE_INSET))
                .into(),
        );
    } else {
        parts.push(
            Space::new()
                .width(Length::Fixed(style::TOGGLE_INSET))
                .into(),
        );
        parts.push(knob.into());
        parts.push(Space::new().width(Length::Fill).into());
    }

    button(
        Row::with_children(parts)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::Center),
    )
    .width(Length::Fixed(style::TOGGLE_W))
    .height(Length::Fixed(style::TOGGLE_H))
    .padding(0)
    .style(move |_theme, status| toggle_style(on, status, theme))
    .on_press(message)
    .into()
}

/// A row that offers an action — or, with `message` `None`, a row that names a state instead.
///
/// It is a row and not a card, so that it can be the last line of a card that already exists
/// ([`wifi::connection_card`]) as well as a card of its own.
pub(crate) fn action_row<'a>(
    label: &'static str,
    message: Option<Message>,
    theme: ThemeMode,
) -> UI<'a> {
    let color = match message {
        Some(_) => style::accent(),
        None => style::notice_for(theme),
    };

    let line = Row::with_children(vec![
        text(label).size(style::DETAIL_FONT).color(color).into(),
        Space::new().width(Length::Fill).into(),
        text(">")
            .size(style::CHEVRON_FONT)
            .color(style::chevron_for(theme))
            .into(),
    ])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let contents = container(line)
        .width(Length::Fill)
        .padding([style::DETAIL_PADDING_V, style::DETAIL_PADDING_H]);

    match message {
        Some(message) => button(contents)
            .width(Length::Fill)
            .padding(0)
            .style(move |_theme, status| row_style(theme, status))
            .on_press(message)
            .into(),
        None => contents.into(),
    }
}

/// A bare text button: a label in the accent colour, with no card and no row around it.
///
/// For an action that is not a setting — asking the radio to scan again — where a card would give it
/// the weight of the rows that *are* settings. It keeps a row's vertical padding regardless: the
/// height is what a finger needs, and a bare word is a small thing to hit.
///
/// With `message` `None` it is a label instead, which is how the same line says "Scanning…" while
/// the scan is in flight — the same trick [`action_row`] uses.
pub(crate) fn text_button<'a>(
    label: &'static str,
    message: Option<Message>,
    theme: ThemeMode,
) -> UI<'a> {
    let color = match message {
        Some(_) => style::accent(),
        None => style::notice_for(theme),
    };

    let label = text(label).size(style::LABEL_FONT).color(color);

    match message {
        Some(message) => button(label)
            .padding([style::ROW_PADDING_V, 0.0])
            .style(move |_theme, status| row_style(theme, status))
            .on_press(message)
            .into(),
        None => container(label)
            .padding([style::ROW_PADDING_V, 0.0])
            .into(),
    }
}

/// A full-width action that reads as one of the cards: [`text_button`]'s opposite number.
///
/// Both are actions that are not settings, and the two differ in what they are worth on screen. A
/// bare word is right when the action belongs to something that is *above* it — "scan again" under a
/// list, which a card would give the weight of the rows it is not one of. A card's own width is right
/// when the action is the page's whole last line and there is nothing above it to belong to: the row
/// ends where the cards end instead of trailing off half-way, and it is the width of a card because
/// it is the height of a finger.
///
/// Carries the card surface, so the press wash and the border are the ones the rows above it use.
pub(crate) fn card_button<'a>(label: &'static str, message: Message, theme: ThemeMode) -> UI<'a> {
    button(
        container(text(label).size(style::LABEL_FONT).color(style::accent()))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding([style::ROW_PADDING_V, style::DETAIL_PADDING_H]),
    )
    .width(Length::Fill)
    .padding(0)
    .style(move |_theme, status| {
        let fill = if is_pressed(status) {
            style::muted_for(theme)
        } else {
            style::card_for(theme)
        };

        button::Style {
            background: Some(fill.into()),
            text_color: style::accent(),
            border: Border {
                color: style::card_border_for(theme),
                width: style::CARD_BORDER_WIDTH,
                radius: style::CARD_RADIUS.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(message)
    .into()
}

/// A round-ended button carrying a glyph.
///
/// Three of them in this app — the back arrow on a page, and the cross and the tick at the head of
/// the password sheet — and one shape for all three, so they are the same size wherever they are
/// met. A capsule and not a circle: a circle's target is its own diameter and nothing else, and the
/// same height carried out sideways is the same button with more of it to hit.
///
/// `message` may be `None`, which draws the button as unavailable rather than hiding it: the tick is
/// `None` for as long as the radio is working.
pub(crate) fn capsule_button<'a>(
    icon: Icon,
    message: Option<Message>,
    look: fn(ThemeMode, button::Status) -> button::Style,
    theme: ThemeMode,
) -> UI<'a> {
    button(
        container(
            text(icon.glyph())
                .font(pomelo_material_symbols::font())
                .size(style::NAV_GLYPH),
        )
        .center_x(Length::Fixed(style::NAV_BUTTON_W))
        .center_y(Length::Fixed(style::NAV_BUTTON_H)),
    )
    .padding(0)
    .style(move |_theme, status| look(theme, status))
    .on_press_maybe(message)
    .into()
}

/// A card whose single row names a state instead of offering an action.
pub(crate) fn notice_card<'a>(label: &'static str, theme: ThemeMode) -> UI<'a> {
    card_surface(
        container(
            text(label)
                .size(style::DETAIL_FONT)
                .color(style::notice_for(theme)),
        )
        .width(Length::Fill)
        .padding([style::DETAIL_PADDING_V, style::DETAIL_PADDING_H]),
        theme,
    )
}

// =============================================================================
// Styles
// =============================================================================

/// The wash a press leaves on a surface that has no colour of its own.
///
/// Light and dark take opposite films: white over a black card, black over a white one. Either way
/// the card is a little closer to the colour of the finger's shadow, which is all a press has to say.
fn press_wash(theme: ThemeMode) -> Color {
    match theme {
        ThemeMode::Dark => Color::from_rgba(1.0, 1.0, 1.0, 0.11),
        ThemeMode::Light => Color::from_rgba(0.0, 0.0, 0.0, 0.08),
    }
}

/// The shape of a capsule, given its fill and its ink.
///
/// Half the height is the radius, which is what makes the two ends semicircles whatever the width
/// is. The ink is set here rather than on the text because iced hands a button's `text_color` down
/// to whatever is drawn inside it — see `Button::draw` — so a capsule's colour is one decision and
/// not two that can disagree.
fn capsule(fill: Color, ink: Color) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: Border {
            radius: (style::NAV_BUTTON_H / 2.0).into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Whether a status is a finger on the button.
fn is_pressed(status: button::Status) -> bool {
    matches!(status, button::Status::Pressed | button::Status::Hovered)
}

/// Back, on the page: the card's own colour.
///
/// The card's and not a bar's, because there is no bar — the button is a shape of its own on the
/// page, and the fill is what makes it read as one.
pub(crate) fn nav_style(theme: ThemeMode, status: button::Status) -> button::Style {
    let fill = if is_pressed(status) {
        style::muted_for(theme)
    } else {
        style::card_for(theme)
    };

    capsule(fill, style::label_for(theme))
}

/// The cross, at the head of the password sheet: the grey of a switch that is off. It is the answer
/// that is not the point of the screen, and it should not look like it is.
pub(crate) fn dismiss_style(theme: ThemeMode, status: button::Status) -> button::Style {
    let fill = if is_pressed(status) {
        style::muted_for(theme)
    } else {
        style::track_off_for(theme)
    };

    capsule(fill, style::label_for(theme))
}

/// The tick: the accent colour, and the only filled thing on the sheet.
///
/// Disabled is grey, and not a dimmer accent: the distance between "press me" and "not yet" should
/// not be a shade the eye has to measure.
pub(crate) fn confirm_style(theme: ThemeMode, status: button::Status) -> button::Style {
    let fill = if matches!(status, button::Status::Disabled) {
        style::muted_for(theme)
    } else if is_pressed(status) {
        style::accent_pressed()
    } else {
        style::accent()
    };

    capsule(fill, Color::WHITE)
}

/// A main-list row: nothing at rest, a wash when the finger is on it.
pub(crate) fn row_style(theme: ThemeMode, status: button::Status) -> button::Style {
    let wash = if matches!(status, button::Status::Pressed) {
        Some(press_wash(theme).into())
    } else {
        None
    };

    button::Style {
        background: wash,
        text_color: style::label_for(theme),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A switch's track: green when on, grey when off.
pub(crate) fn toggle_style(
    on: bool,
    status: button::Status,
    theme: ThemeMode,
) -> button::Style {
    let fill = match (on, status) {
        (true, button::Status::Pressed) => style::green_pressed(),
        (true, _) => style::green(),
        (false, button::Status::Pressed) => style::track_off_pressed_for(theme),
        (false, _) => style::track_off_for(theme),
    };

    button::Style {
        background: Some(fill.into()),
        text_color: Color::WHITE,
        border: Border {
            radius: style::TOGGLE_RADIUS.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar's two halves, and the reason `shares` exists at all: a share of zero is not an empty
    /// box in this layout engine, so neither half may ever be handed one.
    #[test]
    fn a_bar_always_divides_its_track_in_two() {
        assert_eq!(shares(0.0), (1, 9_999));
        assert_eq!(shares(50.0), (5_000, 5_000));
        assert_eq!(shares(100.0), (10_000, 1));
        assert_eq!(shares(21.7), (2_170, 7_830));
        assert_eq!(shares(0.3), (30, 9_970));

        // A reading from a backend that misbehaved is drawn inside the track rather than outside it,
        // and it is still two shares.
        assert_eq!(shares(-5.0), (1, 9_999));
        assert_eq!(shares(120.0), (10_000, 1));
        assert_eq!(shares(f32::NAN), (1, 9_999));
    }

    /// Nothing on this page may be drawn with a share of zero — see the test above for why.
    #[test]
    fn neither_half_of_a_bar_is_ever_zero() {
        for percent in [0.0, 0.001, 0.01, 33.3, 66.6, 99.9, 100.0] {
            let (filled, remaining) = shares(percent);
            assert!(filled > 0 && remaining > 0, "{percent}% gave {filled}/{remaining}");
        }
    }
}
