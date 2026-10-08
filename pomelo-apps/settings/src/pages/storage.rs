//! The storage page: every volume the board has, mounted or in the slot.
//!
//! The page used to be a table of numbers written down in this file; it is now a *reading* of the
//! board, the second page after Wi-Fi that has one. What changed is what it is about: the built-in
//! partition is one volume of two, and the other is a card that may or may not be in the slot —
//! neither of which a literal can know.
//!
//! It does not subscribe to frames, which is where it parts company with the Wi-Fi page. A card is
//! not something that arrives between two frames, and finding out whether one is there is not free:
//! the slot has no card-detect line, so the board answers by *trying to mount* whatever is in it.
//! So the page asks when the page opens and when its button is pressed, and not once a frame.
//!
//! # The chip above the filesystems
//!
//! A page of mounted volumes answers "how much room is left", and on this board that answer is
//! 3 MB — the built-in partition — on a chip of 16. The other 13 MB are the firmware and the
//! reservations around it, none of which is a filesystem and none of which a volume can describe.
//! So the page is headed by the chip instead: one bar over the whole of it ([`flash_card`]), each
//! region coloured by what it is for, and the volumes drawn below as the one region a person can
//! actually write.

use std::sync::Arc;

use iced::widget::{container, text, Column, Row, Space};
use iced::{Alignment, Border, Color, Length};
use pomelo_hal::{Board, FlashLayout, FlashRegion, FlashRegionKind, VolumeInfo, VolumeKind};
use pomelo_material_symbols::Icon;
use pomelo_widgets::{Language, SystemPreferences, ThemeMode};

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::Header;
use crate::pages::common::{
    body, card_button, card_surface, detail_card, notice_card, page, segmented_bar, separator,
    usage_bar, UI,
};
use crate::style;
use crate::{Message, SettingsSection};

/// The storage page: the board's volumes, the chip they live on, and the one thing it can do.
pub struct Storage {
    /// The board. Held rather than handed a snapshot, because the answer changes: a card arrives, a
    /// card leaves, and the page is where someone goes to find out which.
    board: Arc<Board>,
    /// What the board said when it was last asked. Empty until the page is opened.
    volumes: Vec<VolumeInfo>,
    /// The flash chip's map, or `None` if the board could not be asked for it.
    flash: Option<FlashLayout>,
}

impl Storage {
    /// The page before the board has been asked anything.
    pub(crate) fn new(board: Arc<Board>) -> Self {
        Self {
            board,
            volumes: Vec::new(),
            flash: None,
        }
    }

    /// Every volume that is mounted: the built-in one first, then the card, if there is one.
    pub fn volumes(&self) -> &[VolumeInfo] {
        &self.volumes
    }

    /// The card, when one is in the slot.
    pub fn card(&self) -> Option<&VolumeInfo> {
        self.volumes
            .iter()
            .find(|volume| volume.kind == VolumeKind::Removable)
    }

    /// The flash chip's map, when the board could be asked for it.
    pub fn flash(&self) -> Option<&FlashLayout> {
        self.flash.as_ref()
    }

    /// Looks again: re-probes the slot, and reads whatever is mounted.
    ///
    /// What opening the page does, and what its button does. Deliberately not a timer — see the
    /// module docs for why a probe is something to ask for rather than something to poll.
    pub(crate) fn detect(&mut self) {
        if let Err(error) = self.board.storage().refresh() {
            eprintln!("[storage] the slot could not be probed: {error}");
        }

        self.read();
        self.read_flash();
    }

    /// Reads the flash chip's map.
    ///
    /// A partition table is written by whoever flashed the board and does not move while it runs, so
    /// this answer could be taken once and kept. It is taken again on every look all the same: what
    /// it costs is a walk of the table, and taking it here is what keeps [`Storage::detect`] one
    /// thing — every question the page puts to the board, in one place.
    fn read_flash(&mut self) {
        match self.board.storage().flash() {
            Ok(layout) => self.flash = Some(layout),
            // The map stays as it was, like the volumes above: a chip drawn a moment ago is more use
            // than a page that lost a card because the flash could not be asked about.
            Err(error) => eprintln!("[storage] the flash map could not be read: {error}"),
        }
    }

    /// Reads the volumes, without touching the slot.
    fn read(&mut self) {
        match self.board.storage().volumes() {
            Ok(volumes) => self.volumes = volumes,
            // The board could not be asked at all. What is on screen stays there: the volumes drawn
            // a moment ago are more use than an empty page, and the log has the reason.
            Err(error) => eprintln!("[storage] the board could not be read: {error}"),
        }
    }
}

/// The storage page: one bar and one card per volume, and the way to look again.
pub(crate) fn storage_page<'a>(preferences: SystemPreferences, storage: &Storage) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    let mut parts: Vec<UI<'a>> = Vec::new();

    // The chip first: it is the answer that contains the others. The volumes below are regions of
    // it, and "3 MB of 16" is a comparison no single volume can make about itself.
    if let Some(flash) = storage.flash() {
        parts.push(flash_card(language, flash, theme));
    }

    for volume in storage.volumes() {
        parts.push(volume_bar(language, volume, theme));
        parts.push(detail_card(volume_rows(language, volume), theme));
    }

    // The slot says so itself when it is empty, rather than being a bar at zero: a card that is not
    // there is not a card with nothing on it, and a bar is what "nothing on it" looks like.
    if storage.card().is_none() {
        parts.push(notice_card(language.text(Key::CardSlotEmpty), theme));
    }

    // A card and not a bare word: it is the page's last line and belongs to no row above it, so it
    // ends where the cards end — see [`card_button`].
    parts.push(card_button(
        language.text(Key::CheckAgain),
        Message::StorageDetect,
        theme,
    ));

    page(
        Header::section(
            Icon::STORAGE,
            SettingsSection::Storage,
            language.text(Key::Storage),
        )
        .view(theme),
        body(parts),
    )
}

/// The flash chip: its size, what every region of it is for, and the way to tell them apart.
///
/// A picture and its key in one card, because a bar divided by colour is unreadable without one and
/// a key in another card is a key nobody connects to the picture. The heading carries the number the
/// whole card exists for: how much of the chip is claimed, against how much of it there is.
fn flash_card<'a>(language: Language, flash: &FlashLayout, theme: ThemeMode) -> UI<'a> {
    let segments: Vec<(u32, Color)> = flash
        .regions
        .iter()
        .map(|region| (region.size, style::flash_region_for(region.kind, theme)))
        .collect();

    let heading = Row::with_children(vec![
        text(language.text(Key::InternalFlash))
            .size(style::DETAIL_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new().width(Length::Fill).into(),
        text(format!(
            "{} / {}",
            scaled(u64::from(flash.allocated_bytes())),
            scaled(u64::from(flash.total_bytes))
        ))
        .size(style::DETAIL_FONT)
        .color(style::storage_bar())
        .into(),
    ])
    .width(Length::Fill);

    let mut children: Vec<UI<'a>> = vec![
        container(
            Column::with_children(vec![
                heading.into(),
                Space::new().height(Length::Fixed(style::BAR_GAP)).into(),
                segmented_bar(segments, flash.total_bytes),
            ])
            .width(Length::Fill),
        )
        .padding(style::USAGE_PADDING)
        .into(),
    ];

    children.extend(region_rows(language, flash, theme));

    card_surface(Column::with_children(children).width(Length::Fill), theme)
}

/// One row per region of the chip, hairlines between them like any other table of rows.
///
/// The regions and not the four kinds: a kind is a colour, and two `nvs`-sized neighbours that were
/// folded into one line would be a row whose size nobody could check against anything.
fn region_rows<'a>(language: Language, flash: &FlashLayout, theme: ThemeMode) -> Vec<UI<'a>> {
    let last = flash.regions.len().saturating_sub(1);
    let mut children: Vec<UI<'a>> = Vec::new();

    for (index, region) in flash.regions.iter().enumerate() {
        children.push(region_row(language, region, theme));

        if index < last {
            children.push(separator(theme));
        }
    }

    children
}

/// A region's row: its colour, its name, its size, and what may be done with it.
///
/// The label is the board's — `nvs`, `phy_init`, `internal` — and stays untranslated for the reason
/// a mount point does: it is how this machine spells that region, not a word in a language.
fn region_row<'a>(language: Language, region: &FlashRegion, theme: ThemeMode) -> UI<'a> {
    // Copied out rather than reached for from inside the style closure: a closure that borrowed the
    // region would tie the element's lifetime to the map it came from, and the page only has to
    // outlive the frame it is drawn in.
    let kind = region.kind;

    let chip = container(Space::new())
        .width(Length::Fixed(style::FLASH_CHIP))
        .height(Length::Fixed(style::FLASH_CHIP))
        .style(move |_theme| container::Style {
            background: Some(style::flash_region_for(kind, theme).into()),
            border: Border {
                radius: style::FLASH_CHIP_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    let line = Row::with_children(vec![
        chip.into(),
        Space::new()
            .width(Length::Fixed(style::FLASH_CHIP_GAP))
            .into(),
        // Cloned, not borrowed: the page is built for as long as the view lives, and the layout it
        // was read from only has to live until it has been drawn.
        text(region.label.clone())
            .size(style::DETAIL_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new().width(Length::Fill).into(),
        text(scaled(u64::from(region.size)))
            .size(style::DETAIL_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new().width(Length::Fixed(style::VALUE_GAP)).into(),
        text(region_status(language, region.kind))
            .size(style::DETAIL_FONT)
            .color(style::muted_for(theme))
            .into(),
    ])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    container(line)
        .width(Length::Fill)
        .padding([style::DETAIL_PADDING_V, style::DETAIL_PADDING_H])
        .into()
}

/// What may be done with a region, in a word.
///
/// Four answers rather than two, because "read-only" is the wrong thing to say about flash nothing
/// has claimed: one is space with an owner and the other is space with none, and a column that
/// called both of them read-only would be hiding the one region on the chip that is actually free.
fn region_status(language: Language, kind: FlashRegionKind) -> &'static str {
    match kind {
        FlashRegionKind::System => language.text(Key::Reserved),
        FlashRegionKind::Firmware => language.text(Key::ReadOnly),
        FlashRegionKind::Data => language.text(Key::Writable),
        FlashRegionKind::Unallocated => language.text(Key::Unallocated),
    }
}

/// One volume's usage bar: how full it is, in the colour the section is drawn in.
fn volume_bar<'a>(language: Language, volume: &VolumeInfo, theme: ThemeMode) -> UI<'a> {
    let used = volume.used_percent();

    usage_bar(
        kind(language, volume),
        format!("{used:.1}% {}", language.text(Key::Used)),
        style::storage_bar(),
        used,
        style::storage_bar(),
        theme,
    )
}

/// One volume's rows: what it is, where it is, and how much of it there is.
fn volume_rows(language: Language, volume: &VolumeInfo) -> Vec<(&'static str, String)> {
    vec![
        (language.text(Key::Filesystem), volume.filesystem.clone()),
        (language.text(Key::MountPoint), volume.mount_point.clone()),
        (language.text(Key::Total), size(volume.total_bytes)),
        (language.text(Key::Used), size(volume.used_bytes())),
        (language.text(Key::Free), size(volume.free_bytes)),
    ]
}

/// What a volume is, in words: the two kinds this board has, and nothing about the volume itself.
///
/// The *kind* and not the mount point, because this is the one line of the card that is interface
/// rather than data — `/sdcard` is a fact about this board, "SD card" is a word in a language.
fn kind(language: Language, volume: &VolumeInfo) -> &'static str {
    match volume.kind {
        VolumeKind::Internal => language.text(Key::InternalStorage),
        VolumeKind::Removable => language.text(Key::SdCard),
    }
}

/// A byte count the way a row writes it: exact, grouped, and with the unit a person reads.
///
/// Both, and not one. The exact number is what the filesystem reported — a card sold as 32 GB is
/// 31.9 GiB of blocks — and the rounded one is what makes it legible at a glance.
fn size(bytes: u64) -> String {
    format!("{} bytes ({})", grouped(bytes), scaled(bytes))
}

/// `1234567` as `1,234,567`.
fn grouped(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.chars().enumerate() {
        // A separator before this digit when a multiple of three digits follow it — which is every
        // group but the first, and there a group is one to three digits.
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }

    out
}

/// The same count in the largest unit that still leaves a digit before the point.
fn scaled(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    let (unit, scale) = if bytes >= GB {
        ("GB", GB)
    } else if bytes >= MB {
        ("MB", MB)
    } else if bytes >= KB {
        ("KB", KB)
    } else {
        // A filesystem smaller than a kilobyte: not worth a decimal point, and `0.0 KB` would be a
        // worse description of it than the count itself.
        return format!("{bytes} bytes");
    };

    format!("{:.1} {unit}", bytes as f64 / scale as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_count_is_grouped_the_way_the_rows_read() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(8_192), "8,192");
        assert_eq!(grouped(123_456), "123,456");
        assert_eq!(grouped(3_145_728), "3,145,728");
        assert_eq!(grouped(31_890_161_664), "31,890,161,664");
    }

    #[test]
    fn a_byte_count_gets_the_unit_it_is_read_in() {
        assert_eq!(scaled(512), "512 bytes");
        assert_eq!(scaled(2_048), "2.0 KB");
        assert_eq!(scaled(3_145_728), "3.0 MB");
        assert_eq!(scaled(31_890_161_664), "29.7 GB");
    }

    /// The two halves of the one number a row prints.
    #[test]
    fn a_size_is_exact_and_readable_at_once() {
        assert_eq!(size(3_145_728), "3,145,728 bytes (3.0 MB)");
    }
}
