//! A card, and the tiles in it.
//!
//! iced has no `ListTile`, so the list's row is ours to name: [`Tile`] is the unit every settings
//! list is built from — the icon and its fill, the label, what sits beside it, what sits at its
//! end, and the message the whole row sends. [`Card`] is the surface those sit on.
//!
//! # A `Card` is not a group of tiles
//!
//! It holds any content at all. A sub-page's head is a card with a large icon, a title and a
//! paragraph in it, and no tiles — that is [`Header`]. That is why the hairline belongs to
//! [`Card::tile`] rather than to a loop over tiles: a card cannot know which of its children *are*
//! tiles, but it does know whether it is empty.

use iced::widget::{button, container, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Length, Padding};

use pomelo_material_symbols::Icon;

use crate::pages::common::{capsule_button, card_surface, nav_style, row_style, separator, UI};
use crate::style;
use crate::{Message, SettingsSection};
use pomelo_widgets::ThemeMode;

/// The square at the left of a tile: a glyph and the fill it sits on.
///
/// One field and not two, because they are one thing — a tile that has no square has no fill either,
/// and a bare `color` would have to hold a value nobody reads.
struct Mark {
    icon: Icon,
    color: style::IconColor,
}

/// One row of a settings list.
///
/// Named fields rather than a tuple. iced has no `ListTile`, so the row is ours to name — and a
/// six-element tuple is not something a reader can count.
pub(crate) struct Tile<'a> {
    /// The square on the left, for a row that leads somewhere.
    mark: Option<Mark>,
    label: &'static str,
    /// What sits between the label and the end of the row, or nothing.
    ///
    /// An `Element` and not a `String`: most rows put a plain value there, and one puts the password
    /// field, which is dots *painted* rather than typed and cannot be spelled. The value rows make
    /// their text with `text(...)`, and the tests find it by that text — which is the ability they
    /// need.
    secondary: Option<UI<'a>>,
    trailing: Option<Icon>,
    /// What a press sends, or `None` for a row that is not pressable.
    message: Option<Message>,
}

impl<'a> Tile<'a> {
    /// A tile that opens `section`, ending in the chevron that says so.
    ///
    /// The icon's fill comes from the section too, so a tile cannot be given a colour the palette
    /// has not got — see [`style::IconColor`].
    pub(crate) fn section(icon: Icon, section: SettingsSection, label: &'static str) -> Self {
        Self {
            mark: Some(Mark {
                icon,
                color: style::IconColor::for_section(section),
            }),
            label,
            secondary: None,
            trailing: Some(Icon::KEYBOARD_ARROW_RIGHT),
            message: Some(Message::Open(section)),
        }
    }

    /// A tile whose message is not "open this section" — a row that switches something instead.
    pub(crate) fn action(
        icon: Icon,
        color: style::IconColor,
        label: &'static str,
        message: Message,
    ) -> Self {
        Self {
            mark: Some(Mark { icon, color }),
            label,
            secondary: None,
            trailing: Some(Icon::KEYBOARD_ARROW_RIGHT),
            message: Some(message),
        }
    }

    /// A row that is a label and what it is a label *for*.
    ///
    /// No square and no chevron: the two things that promise a page behind the row are exactly what
    /// this one does not have, and neither does it take a press. The password sheet's field is the
    /// only one of these — a row of a card that is not a destination.
    pub(crate) fn field(label: &'static str) -> Self {
        Self {
            mark: None,
            label,
            secondary: None,
            trailing: None,
            message: None,
        }
    }

    /// The same tile, with something beside its label.
    pub(crate) fn secondary(mut self, secondary: impl Into<UI<'a>>) -> Self {
        self.secondary = Some(secondary.into());
        self
    }

    /// The same tile, ending in `icon` instead of the chevron — `None` for no ending at all.
    ///
    /// A card inside a sub-page does not lead anywhere, so its tiles say why they are there with
    /// `Some(Icon::INFO)` instead of promising a page that is not behind them.
    pub(crate) fn trailing(mut self, trailing: Option<Icon>) -> Self {
        self.trailing = trailing;
        self
    }

    /// The row, as a widget.
    pub(crate) fn view(self, theme: ThemeMode) -> UI<'a> {
        let Tile {
            mark,
            label,
            secondary,
            trailing,
            message,
        } = self;

        let mut left: Vec<UI<'a>> = Vec::new();

        if let Some(mark) = mark {
            left.push(badge(mark.icon, mark.color, style::Badge::ROW));
            left.push(Space::new().width(Length::Fixed(style::BADGE_GAP)).into());
        }

        left.push(
            text(label)
                .size(style::LABEL_FONT)
                .color(style::label_for(theme))
                .into(),
        );

        let left = Row::with_children(left).align_y(Alignment::Center);

        // A row with no message is not a destination. It is a label and the box its value is typed
        // into, and it is *outlined*, because the row is the box: what a person types goes inside the
        // row, so the row is the thing that has to look like it can be typed into.
        //
        // The box takes what is left of the row, and it takes it from the left. A value that grew
        // leftward from a right edge would slide the text under the eye on every key press.
        let Some(message) = message else {
            let mut row: Vec<UI<'a>> = vec![left.into()];

            if let Some(secondary) = secondary {
                row.push(Space::new().width(Length::Fixed(style::VALUE_GAP)).into());
                row.push(secondary);
            }

            return container(
                Row::with_children(row)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .padding([style::ROW_PADDING_V, style::ROW_PADDING_H])
            .style(move |_theme| container::Style {
                border: Border {
                    // The colour a hairline between two rows is drawn in: its contrast against a card
                    // is already a settled thing, and a field is a box *on* a card, not on the page.
                    color: style::separator_for(theme),
                    width: style::CARD_BORDER_WIDTH,
                    radius: style::FIELD_RADIUS.into(),
                },
                ..container::Style::default()
            })
            .into();
        };

        // The label begins at `style::ROW_TEXT_INSET` from the card's edge — this row's padding,
        // the badge, the gap — and the hairline above the row is told to start there too. See
        // `common::separator`.
        let mut right: Vec<UI<'a>> = Vec::new();

        if let Some(secondary) = secondary {
            right.push(secondary);
            right.push(Space::new().width(Length::Fixed(style::VALUE_GAP)).into());
        }

        if let Some(trailing) = trailing {
            right.push(
                text(trailing.glyph())
                    .font(pomelo_material_symbols::font())
                    .size(style::CHEVRON_FONT)
                    .color(style::chevron_for(theme))
                    .into(),
            );
        }

        let contents = Row::with_children(vec![
            left.into(),
            Space::new().width(Length::Fill).into(),
            Row::with_children(right).align_y(Alignment::Center).into(),
        ])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        button(
            container(contents)
                .width(Length::Fill)
                .padding([style::ROW_PADDING_V, style::ROW_PADDING_H]),
        )
        .width(Length::Fill)
        .padding(0)
        .style(move |_theme, status| row_style(theme, status))
        .on_press(message)
        .into()
    }
}

/// A card: a rounded surface holding a column, with a hairline between two tiles.
///
/// A builder and not a widget — what a card *is* is `container` plus `Column`, and both already
/// exist. Implementing `Widget` would buy tree-level control (a tag, state, an overlay) that a
/// column of things does not want. iced ships no `TileGroup` either, for the same reason: a list is
/// a layout, and layouts are composed.
pub(crate) struct Card<'a> {
    children: Vec<UI<'a>>,
    theme: ThemeMode,
}

impl<'a> Card<'a> {
    pub(crate) fn new(theme: ThemeMode) -> Self {
        Self {
            children: Vec::new(),
            theme,
        }
    }

    /// Adds a tile, with a hairline above it unless it is the card's first child.
    ///
    /// Above, and not below: "not the last one" is a fact about a card's *end*, which the loop that
    /// builds it knows and a card does not; "not the first one" is a fact about its *contents*,
    /// which is exactly what a builder has.
    pub(crate) fn tile(mut self, tile: Tile<'a>) -> Self {
        if !self.children.is_empty() {
            self.children.push(separator(self.theme));
        }

        self.children.push(tile.view(self.theme));
        self
    }

    pub(crate) fn view(self) -> UI<'a> {
        card_surface(
            Column::with_children(self.children).width(Length::Fill),
            self.theme,
        )
    }
}

/// The head of a sub-page: the section's icon, the page's title beside it, and the way back above
/// both — the arrow [`capsule_button`] draws, in [`nav_style`]'s fill.
///
/// The same card the list is made of, holding no tiles at all — which is why [`Card`] is a surface
/// and not a group of tiles. It is what lets a sub-page go without a navigation bar: the heading
/// belongs *in* the page and scrolls with it, and a bar is a thing that covers content.
///
/// The icon is the one the list row carried, in the fill that row carried, so walking in from the
/// list and arriving here shows the same square in the same colour.
pub(crate) struct Header {
    icon: Icon,
    color: style::IconColor,
    title: &'static str,
}

impl Header {
    /// The head of the page for `section`.
    pub(crate) fn section(icon: Icon, section: SettingsSection, title: &'static str) -> Self {
        Self {
            icon,
            color: style::IconColor::for_section(section),
            title,
        }
    }

    /// The head, as a widget: the navigation row below the page's top edge, then the card.
    ///
    /// The gap under the card is `CARD_GAP`, the body's own spacing: a head card is a card, and the
    /// distance from one card to the next does not depend on which of the two it is.
    pub(crate) fn view<'a>(self, theme: ThemeMode) -> UI<'a> {
        let title = Row::with_children(vec![
            badge(self.icon, self.color, style::Badge::HEAD),
            Space::new().width(Length::Fixed(style::HEAD_GAP)).into(),
            text(self.title)
                .size(style::TITLE_FONT)
                .color(style::label_for(theme))
                .into(),
        ])
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let nav = Row::with_children(vec![
            capsule_button(Icon::ARROW_BACK, Some(Message::Back), nav_style, theme),
            Space::new().width(Length::Fill).into(),
        ])
        .width(Length::Fill);

        container(
            Column::with_children(vec![
                nav.into(),
                Space::new().height(Length::Fixed(style::NAV_GAP)).into(),
                card_surface(container(title).padding(style::HEAD_PADDING), theme),
            ])
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .padding(Padding {
            top: style::NAV_TOP_GAP,
            bottom: style::CARD_GAP,
            left: style::PAGE_MARGIN,
            right: style::PAGE_MARGIN,
        })
        .into()
    }
}

/// The icon square at the left of a tile or at the head of a page: a glyph out of the icon font, on
/// a solid of the palette.
fn badge<'a>(icon: Icon, color: style::IconColor, size: style::Badge) -> UI<'a> {
    container(
        text(icon.glyph())
            .font(pomelo_material_symbols::font())
            .size(size.glyph)
            .color(Color::WHITE),
    )
    .width(Length::Fixed(size.side))
    .height(Length::Fixed(size.side))
    .center_x(Length::Fixed(size.side))
    .center_y(Length::Fixed(size.side))
    .style(move |_theme| container::Style {
        background: Some(color.color().into()),
        border: Border {
            radius: size.radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}
