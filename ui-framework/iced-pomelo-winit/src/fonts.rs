//! Getting a font into iced, which has none on a device.
//!
//! `iced_graphics::text::font_system()` is a global `RwLock<FontSystem>`, and iced initialises it
//! with `FontSystem::new_with_fonts`, which *also* scans the operating system's font
//! directories. A desktop has those; ESP-IDF does not. What is left after the scan is whatever
//! iced embedded itself: `Iced-Icons.ttf`, an icon font, plus `FiraSans-Regular.ttf` if the
//! `fira-sans` feature is on — which it is not in our feature selection, and which would cost
//! 441 KiB of flash if it were.
//!
//! So we load our own. Two steps, and the second is the one that matters:
//!
//! 1. `FontSystem::load_font` puts the bytes in the database.
//! 2. `iced_core::Font::default()` is `Family::SansSerif` — not a name — so the face has to be
//!    made *the* sans-serif family, or the default would resolve to the icon font and every
//!    label would come out as glyphs of nothing.
//!
//! Step 2 asks the database which family it just indexed instead of taking a name from the
//! caller: a name that does not match byte for byte fails silently, and silently missing text is
//! exactly the failure this module exists to prevent.

//! # The default font
//!
//! An app written for iced knows nothing about this board's fonts, and would draw no text at all
//! if it had to install one. So this crate carries its own: [`install_default`] puts a 0.19 MB
//! Simplified-Chinese subset of Source Han Sans (see `pomelo-font`'s README) in the database
//! unless something else already did, and the host calls it on the way in —
//! [`Application::new`](crate::Application::new) and [`Host::new`](crate::Host::new).
//!
//! Two rules, and the order between them is the whole design:
//!
//! * [`install`] always wins. It is how an app or a firmware says "this is the typeface", and the
//!   face it installs becomes the *sans-serif* family — which is what `Font::default()` resolves
//!   to — so the last explicit install is what every unqualified label gets.
//! * [`install_default`] only fills a gap. It is a no-op the moment anything has been installed,
//!   so an app that installs its own font in `boot()` keeps it, whichever order the two happen to
//!   run in.
//!
//! A program can also bring fonts through iced's own channel — `Program::settings().fonts` — and
//! [`Host::new`](crate::Host::new) loads those. They are extra faces to name explicitly
//! (`Font::with_name`), which is iced's meaning for them, and they do not touch the default family.

//! # A wider face, from the card
//!
//! The subset above is what the *image* carries, and it is what a box draws with until somebody
//! gives it more: a font file and the table baked from it, both on the card. The firmware's boot
//! names the table with [`prefer_tables`] and installs the face through [`install`], and the two are
//! matched by the SHA-256 in the table's header — the one the baker wrote down — rather than by the
//! order the two calls happened in. That matters because a table is not interchangeable: its glyph
//! numbers are the numbers of the font it was baked from, so the card's table means nothing to this
//! crate's subset and the baked one means nothing to a face that arrived on a card.
//!
//! Nothing else changes: an app that installs a face of its own still gets no tables, and a size no
//! table covers is still rasterised on the device.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "renderer")]
use std::sync::OnceLock;

#[cfg(feature = "renderer")]
use sha2::{Digest, Sha256};

use iced_graphics::text::cosmic_text::fontdb::Family;

/// What this crate embeds, so that a program which installs nothing still draws text.
///
/// One font, not two: CJK and Latin live in the same face, because mixing a Latin font with a
/// Chinese one means deciding twice per label which one draws it — and `Font::default()` can only
/// name one family.
///
/// The subset is Source Han Sans SC cut to the characters this source tree writes, plus ASCII and a
/// fullwidth space: `tools/scan_charset.py` reads them out of the string and character literals, so
/// a label that somebody wrote is a label with glyphs, and there is no second list to keep in step
/// with the code. `pomelo-font`'s README has the exact commands — and the reason the cut is this
/// narrow: a GB2312 level-1 charset is 3.3 MB larger, which is the difference between one app slot
/// on a 16 MB part and two.
const DEFAULT: &[u8] = include_bytes!("../fonts/dist/SourceHanSansSC-Regular-Subset.otf");

/// The glyphs of [`DEFAULT`] that were rasterised on the build machine, at the one size the
/// interface draws at.
///
/// Flash, not RAM: the renderer reads the table in place, and a screen drawn at the baked size never
/// rasterises anything. The format is in `pomelo-font`'s `dist/MANIFEST.md` — a size that is not here
/// is rasterised on the device as it always was, so this is a speed setting and not a correctness one.
///
/// One table, because there is one size: the four-tier font scale is gone, and the size every label
/// is drawn at is `FONT_BASE`'s 24 px. The lookup compares the size bit for bit, so a table cut at
/// any other size would simply never be read.
#[cfg(feature = "renderer")]
const BAKED: &[&[u8]] = &[
    include_bytes!("../fonts/dist/SourceHanSansSC-Regular-Subset-common@24px.bin"),
];

/// The tables to install instead of [`BAKED`], if a firmware put any here — see [`prefer_tables`].
#[cfg(feature = "renderer")]
static PREFERRED: OnceLock<&'static [&'static [u8]]> = OnceLock::new();

/// Says that the tables to bind are `tables`, rather than the ones baked into this image.
///
/// The one caller is a firmware's boot, and the one reason is size: the 24 px coverage is a couple of
/// megabytes, which is a couple of megabytes a board that wants two app slots cannot spare — so the
/// coverage is published on its own, a box is given a newer one by writing a file, and this is how the
/// interface is told to draw from it. A face can arrive the same way, with the table that was baked
/// from it, and the same boot installs that through [`install`] — which is what the SHA-256 in the
/// table's header is for: it decides which of the two sets belongs to the face that ends up installed,
/// and a set that belongs to neither is not bound at all. The bytes are borrowed for as long as the
/// process runs, so the caller has to hand over memory that lives that long (`Box::leak`; there is one
/// of these per boot).
///
/// Call this **before** anything installs a font — before the first frame, and therefore before
/// [`install_default`] runs from either entry point. A later call is the same as no call at all:
/// [`install`] reads this once, at the moment it binds the tables to the face.
///
/// # Errors
///
/// If a table is not one. That check is here, and not left to [`iced_pomelo_gfx::baked::install`],
/// precisely because these bytes did *not* come out of the image: a table that arrived over the network
/// is a table that can be truncated, and the documented answer to a bad one is a panic — which is the
/// right answer for `include_bytes!` and the wrong one for a download. So a caller that gets an `Err`
/// logs it and carries on with the baked table, and a caller that gets an `Ok` knows the renderer will
/// not be handed anything it refuses.
#[cfg(feature = "renderer")]
pub fn prefer_tables(
    tables: &'static [&'static [u8]],
) -> Result<(), iced_pomelo_gfx::baked::Error> {
    for bytes in tables {
        iced_pomelo_gfx::baked::Atlas::new(bytes)?;
    }

    // Lost, and not an error: the tables are read once, so a second preference is a second boot's
    // question and this one has already been answered.
    let _ = PREFERRED.set(tables);
    Ok(())
}

/// The tables to bind to `bytes`, the face that was just installed: the ones a firmware preferred,
/// or the ones this image was built with — whichever of the two was baked from *this* face.
///
/// The question is the face's and not the caller's, and it is asked of the SHA-256 the baker put in
/// the table's header. It used to be `std::ptr::eq(bytes, DEFAULT)`, which said the same thing while
/// the built-in subset was the only face a table could be baked for. A card can bring a face of its
/// own now, and with it the case that test got wrong in both directions: this image's table is a
/// stranger to the card's face, and an app's face is a stranger to both.
///
/// Nothing matching is an ordinary answer — an app that installs a face of its own rasterises as it
/// always did — and it is why this returns a slice rather than an `Option`: the caller has to be
/// able to tell "these are the tables" from "there are none".
#[cfg(feature = "renderer")]
fn tables_for(bytes: &'static [u8]) -> &'static [&'static [u8]] {
    let digest = Sha256::digest(bytes);

    if let Some(preferred) = PREFERRED.get().copied() {
        if baked_from(preferred, &digest) {
            return preferred;
        }
    }

    if baked_from(BAKED, &digest) {
        return BAKED;
    }

    &[]
}

/// Whether every table in `tables` was baked from the face whose SHA-256 is `digest`.
///
/// All of them and not any: a set of tables is one face's coverage at several sizes, so half of it
/// matching is a set that was put together wrong, and drawing from the wrong half is worse than
/// drawing from none.
///
/// A table that does not parse counts as not matching. It cannot get this far — [`prefer_tables`]
/// refuses one at the door, and `include_bytes!` data is a build that should not have shipped — but
/// the answer here is a fallback rather than a panic, so nothing needs to be said about it twice.
#[cfg(feature = "renderer")]
fn baked_from(tables: &'static [&'static [u8]], digest: &[u8]) -> bool {
    tables.iter().all(|bytes| {
        iced_pomelo_gfx::baked::Atlas::new(bytes)
            .map(|table| table.font_hash() == &digest[..16])
            .unwrap_or(false)
    })
}

/// Whether anyone has installed a font yet — an app, a firmware, or [`install_default`] itself.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Installs `bytes` as iced's default font.
///
/// Call this before the first frame. Calling it twice with the same bytes is free — iced keys
/// loaded fonts by address for borrowed slices, which static `include_bytes!` data is.
pub fn install(bytes: &'static [u8]) {
    let mut system = iced_graphics::text::font_system()
        .write()
        .expect("write font system");

    system.load_font(Cow::Borrowed(bytes));

    // The face we just added is the last one indexed. Its id is what the baked tables bind to —
    // glyph numbers only mean anything against the font they came from.
    let face = system.raw().db_mut().faces().last().map(|face| {
        (
            face.id,
            face.families.first().map(|(family, _)| family.clone()),
        )
    });

    if let Some((_, Some(family))) = &face {
        system.raw().db_mut().set_sans_serif_family(family.clone());
    }

    INSTALLED.store(true, Ordering::Relaxed);

    // The tables are installed by whoever installed the face they were baked from: an app that
    // brings a face of its own gets none of this, and the renderer would refuse them by `font_id`
    // even if it did.
    //
    // Which tables, and not whether: a firmware that found a wider face on a card said so with
    // [`prefer_tables`] and installed that face through this same call, so this is where the two
    // halves meet — once, because this is the moment the face and its glyph numbers both exist.
    #[cfg(feature = "renderer")]
    {
        let tables = tables_for(bytes);

        // No tables is not an empty install: a face nothing was baked for is a face the renderer
        // rasterises for, which is what it already does with nothing installed at all.
        if !tables.is_empty() {
            if let Some((id, _)) = face {
                iced_pomelo_gfx::baked::install(id, tables);
            }
        }
    }
}

/// Installs this crate's own subset, unless an app or a firmware installed a font first.
///
/// The host calls this at both entry points, which is what lets a program written for iced show
/// text without knowing anything about fonts. See the module docs for the rule.
pub fn install_default() {
    if installed() {
        return;
    }

    install(DEFAULT);
}

/// Whether a font has been installed by anyone — an app, a firmware, or this crate's default.
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// The family `Font::default()` resolves to, which is the sans-serif one.
///
/// For diagnostics, and the reason [`install`] sets it: a device that renders the wrong glyphs
/// should be asked what it actually has.
pub fn sans_serif() -> String {
    // A write guard, not a read one: iced only exposes `raw()` mutably.
    let mut system = iced_graphics::text::font_system()
        .write()
        .expect("write font system");

    system.raw().db().family_name(&Family::SansSerif).to_owned()
}

/// The names of the families currently in the database. For diagnostics — a device that renders
/// no text should be asked what it actually has.
pub fn families() -> Vec<String> {
    // A write guard, not a read one: iced only exposes `raw()` mutably.
    let mut system = iced_graphics::text::font_system()
        .write()
        .expect("write font system");

    system
        .raw()
        .db()
        .faces()
        .flat_map(|face| face.families.iter().map(|(family, _)| family.clone()))
        .collect()
}
