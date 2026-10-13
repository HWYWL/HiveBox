//! The theme page: dark/light mode switch, colour palette and display details.

use iced::widget::{container, text, Column, Row, Space};
use iced::{Alignment, Border, Length};
use pomelo_material_symbols::Icon;
use pomelo_widgets::SystemPreferences;

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::{Card, Header, Tile};
use crate::pages::common::{body, card_surface, detail_card, page, UI};
use crate::style;
use crate::{Message, SettingsSection};

pub(crate) fn theme_page<'a>(preferences: SystemPreferences) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;

    // The row that changes something is a tile — the same row the main list is built from — and not
    // a layout of its own: a value beside a label is exactly what a tile is. The chevron is the one
    // part of it it does not want, because it opens nothing.
    //
    // There used to be a second tile here, for the font size, and it cycled four tiers. The tiers are
    // gone: the interface draws at one size, so there is nothing left to point at.
    let controls = Card::new(theme)
        .tile(
            Tile::action(
                if preferences.theme.is_dark() {
                    Icon::DARK_MODE
                } else {
                    Icon::LIGHT_MODE
                },
                style::IconColor::for_section(SettingsSection::Theme),
                language.text(Key::DarkMode),
                Message::SetTheme(preferences.theme.other()),
            )
            .secondary(text(preferences.theme.name(language)))
            .trailing(None),
        )
        .view();

    let colors = style::palette();

    let top = Row::with_children(
        colors[..4]
            .iter()
            .map(|color| chip(*color, theme))
            .collect::<Vec<_>>(),
    )
    .spacing(style::CHIP_GAP);
    let bottom = Row::with_children(
        colors[4..]
            .iter()
            .map(|color| chip(*color, theme))
            .collect::<Vec<_>>(),
    )
    .spacing(style::CHIP_GAP);

    let palette = card_surface(
        container(
            Column::with_children(vec![
                text(language.text(Key::Presets))
                    .size(style::DETAIL_FONT)
                    .color(style::label_for(theme))
                    .into(),
                Space::new().height(Length::Fixed(style::CHIP_GAP)).into(),
                top.into(),
                Space::new().height(Length::Fixed(style::CHIP_GAP)).into(),
                bottom.into(),
            ])
            .width(Length::Fill)
            .align_x(Alignment::Center),
        )
        .padding(style::PALETTE_PADDING),
        theme,
    );

    let details = vec![
        (
            language.text(Key::Style),
            preferences.theme.name(language).to_string(),
        ),
        // Not a picture and not another system's picture at that: the desktop's background is a colour,
        // and it is black so that the panel's bezel has no edge drawn on the screen. There is no wallpaper
        // to name, and naming one would be this row telling the reader about a file the image never had.
        (
            language.text(Key::Wallpaper),
            "pure black (no edge on the bezel)".to_string(),
        ),
        (
            language.text(Key::Emissive),
            "AMOLED (no backlight bleed)".to_string(),
        ),
        (
            language.text(Key::RefreshRate),
            "60 Hz Direct QSPI DMA".to_string(),
        ),
        (
            language.text(Key::AntiAliasing),
            "subpixel coverage (analytical AA)".to_string(),
        ),
    ];

    page(
        Header::section(
            Icon::PALETTE,
            SettingsSection::Theme,
            language.text(Key::ThemeTitle),
        )
        .view(theme),
        body(vec![controls, palette, detail_card(details, theme)]),
    )
}

/// One preset colour chip, 60x40.
fn chip<'a>(color: iced::Color, theme: pomelo_widgets::ThemeMode) -> UI<'a> {
    container(Space::new())
        .width(Length::Fixed(style::CHIP_W))
        .height(Length::Fixed(style::CHIP_H))
        .style(move |_theme| container::Style {
            background: Some(color.into()),
            border: Border {
                color: style::chip_border_for(theme),
                width: 1.0,
                radius: style::CHIP_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}
