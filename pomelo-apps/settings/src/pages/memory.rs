//! The memory page: a usage bar and heap details.

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::Header;
use crate::pages::common::{body, detail_card, page, usage_bar, UI};
use crate::style;
use crate::SettingsSection;
use pomelo_material_symbols::Icon;
use pomelo_widgets::SystemPreferences;

pub(crate) fn memory_page<'a>(preferences: SystemPreferences) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    let bar = usage_bar(
        language.text(Key::MemoryUsage),
        format!("21.7% {}", language.text(Key::Used)),
        style::memory_bar(),
        21.7,
        style::memory_bar(),
        theme,
    );

    let details = vec![
        (
            language.text(Key::Total),
            "8,519,680 bytes (8.5 MB)".to_string(),
        ),
        (
            language.text(Key::InternalSram),
            "512 KB (kernel and DMA)".to_string(),
        ),
        (
            language.text(Key::Psram),
            "8.0 MB Octal-SPI @ 80MHz".to_string(),
        ),
        (
            language.text(Key::HeapUsed),
            "1,852,416 bytes (1.85 MB)".to_string(),
        ),
        (
            language.text(Key::HeapFree),
            "6,667,264 bytes (6.66 MB)".to_string(),
        ),
        (
            language.text(Key::Framebuffer),
            "921.6 KB (480x480 RGB565)".to_string(),
        ),
        (
            language.text(Key::Health),
            "good (0% fragmentation)".to_string(),
        ),
    ];

    page(
        Header::section(
            Icon::MEMORY,
            SettingsSection::Memory,
            language.text(Key::MemoryTitle),
        )
        .view(theme),
        body(vec![bar, detail_card(details, theme)]),
    )
}
