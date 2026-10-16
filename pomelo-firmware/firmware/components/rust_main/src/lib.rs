//! Rust firmware entrypoint.

use std::io::Read;
use std::sync::Arc;

/// Native firmware entrypoint called by main.c
#[no_mangle]
pub extern "C" fn rust_main_entry() {
    println!("[POMELO UI] rust main entry started");

    std::panic::set_hook(Box::new(|info| {
        eprintln!("\n🚨 [RUST PANIC] {info}\n");
    }));

    let board = pomelo_hal_esp32::board();
    board.init();

    // The span kernels, measured on this chip rather than reasoned about from a host's.
    //
    // It runs here, first, for two reasons. The buffer is a megabyte-class allocation, and the only
    // reliable moment to ask for one of those is before anything else has taken a piece of the heap
    // — an `try_reserve_exact` that fails prints a line and gives up rather than panicking, but a
    // failure would mean no numbers at all. And the figures are wanted once, not per frame, so this
    // is a boot cost rather than a cost in the loop.
    //
    // It is the other half of what `render-debug` prints. The `[phase]` line is a *real* frame split
    // by phase; this is the same kernels run in isolation. When the two disagree, the difference is
    // the answer: a full-screen `fill_rect` that is fast here and slow in `clear` is a memory
    // problem, not an arithmetic one, and arithmetic is the only thing a vector kernel can fix.
    //
    // Each kernel is timed for a fixed wall-clock budget rather than a fixed iteration count, which
    // is what makes this identical to what the host runs. It also bounds the worst case: the first
    // version of this used a count tuned on a host, which on the panel meant seventeen seconds in a
    // tight loop with no yield — long enough to starve the idle task and trip the task watchdog
    // during boot. The budget keeps every measurement to a few hundred milliseconds.
    #[cfg(feature = "render-debug")]
    {
        println!("[bench] span kernels at the device's own clock");
        pomelo_gfx::bench::report();
        println!("[bench] done");
    }

    // The glyph table the interface will be drawn with, decided here because this is the last moment at
    // which nothing has drawn yet and the first at which the card is mounted. See the function.
    //
    // After the benchmark above and not before it, which is the one ordering that matters: that
    // measurement wants a megabyte-class allocation and wants it early, and this one takes whatever the
    // table on the card turns out to be and keeps it for the life of the program.
    prefer_card_font();

    // The radio comes up, and maybe connects, on a thread of its own. Boot is for the panel: a
    // connection is a thing that happens *later*, and a board that waited for one would show nothing
    // for as long as the network took. No file, or a switch left off, and this thread does nothing
    // at all.
    {
        let board = Arc::clone(&board);

        std::thread::Builder::new()
            .name("wifi_autoconnect".into())
            .spawn(move || {
                if let Err(error) = board.wifi().autoconnect() {
                    eprintln!("[wifi] autoconnect: {error:?}");
                }
            })
            .expect("failed to spawn wifi_autoconnect");
    }

    // The management server comes up with the board, and this is the one line that decides it.
    //
    // Up front rather than at the tap of a tile, because the page is for reaching the box *from another
    // device*: a server that has to be switched on at the panel is one you have to walk over to the box
    // to use, which is the thing the page exists to avoid. Started here rather than by the app of the
    // same name because it is not that app's to decide — the app is a switch *over* a board service, and
    // this is the board's boot.
    //
    // Before Wi-Fi, which costs nothing: binding a port needs no network, and the address appears with
    // the connection the thread above is making (see `WebStatus::url`).
    //
    // What it means, said plainly here because the panel cannot say it until somebody has already tapped
    // the tile: **the page has no password.** From this moment, anyone on the same network can read and
    // write the box's files and change its network. The app's switch turns it off, and nothing turns it
    // back on until the next boot.
    match board.web().start(pomelo_hal::WebStatus::DEFAULT_PORT) {
        Ok(()) => println!(
            "[web] management server on port {}",
            pomelo_hal::WebStatus::DEFAULT_PORT
        ),
        Err(error) => eprintln!("[web] management server did not start: {error:?}"),
    }

    println!("[iced] starting app-launcher on AMOLED panel (480x480)");

    if let Err(error) = app_launcher::program(Arc::clone(&board)).run() {
        eprintln!("[iced] the application stopped: {error:?}");
    }
}

/// The most a glyph table read off the card may be, in bytes, before this boot refuses to read it.
///
/// The table this image carries is 234 KiB, and the widest one a card is meant to hold — the one baked
/// over a GB2312-level-1 charset, 4536 glyphs — is 2.3 MB, so this is that size with room to spare. It
/// is a bound on a *surprising* file rather than on the real one, and it is here because of where these
/// bytes come from: a card, written by a download. A boot that read whatever size a file claimed is a
/// boot that one bad file could take the heap with, and not reading it is the honest answer to "this is
/// not a glyph table".
const FONT_TABLE_MAX: u64 = 3 * 1024 * 1024;

/// The most a font read off the card may be, for the same reason as the table above and against the same
/// file: the face of the GB2312-level-1 package is 1.9 MB. Both halves are read into memory at once, so
/// a bound that is generous about one of them is not generous about the two of them together.
const FONT_FACE_MAX: u64 = 3 * 1024 * 1024;

/// Draws with the newest font on the card, if there is one, instead of the one in this image.
///
/// The image carries a 24 px table covering exactly the characters this source tree writes, and a subset
/// of a font to match, which is what lets a box with no card draw its own interface. That is enough for
/// the interface and not for everything that reaches the panel: a file name read off a card, a Wi-Fi
/// name, a line of text from a server — characters nobody wrote into the source — have no glyph in the
/// subset, and draw as boxes.
///
/// A *table* alone cannot fix that, which is worth saying twice because the update page publishes little
/// else: a table is a cache of glyph masks keyed by glyph number, the numbers are the numbers of the font
/// that was rasterised, and this image's subset has 528 characters to number. A card that wants to draw a
/// character the subset never had has to bring the font too. So there are two shapes here and one
/// decision — `updater::card_font`, where the decision is made, and which both this boot and that page
/// read:
///
/// - **a face and the table baked from it**, put on the card together: the face is installed as the
///   interface's font, and the table is bound to it because the header says it was baked from that face.
///   This is the shape that draws the characters the subset never had;
/// - **a table on its own**, which is coverage for the face in this image: newer masks for the same 528
///   characters, and the shape the update page writes.
///
/// Every way this can fail ends with the image's own font still in place: no card, no font on it, a file
/// too big to be one, bytes that are not a font at all, a table that was not baked from the face beside
/// it. That is not politeness. The boot has a panel to bring up and somebody watching it, and "there is
/// no font" is not among the things worth doing about a file — the interface that draws the wrong glyphs
/// is a worse boot than the one that draws the ones it shipped with, but only just, and neither is worth
/// a panic here.
///
/// The bytes are read into memory and leaked, and that is a real cost: a font is borrowed by the renderer
/// for as long as the program runs, a card has no `mmap` to borrow from, and the baked font is the only
/// one that costs no RAM at all because it is read in place out of flash. A quarter of a megabyte, once
/// per boot, is what drawing from the card costs a box with no font on its card — and the files on the
/// card are what decide that number, so a download is the one thing that can make this boot allocate
/// more.
fn prefer_card_font() {
    let Some(font) = updater::card_font() else {
        // No card, or nothing on it. The common case for a box that has never been given a font, and not
        // worth a line: the baked one is what this image was built with either way.
        return;
    };

    let updater::CardFont {
        version: _,
        face,
        table,
    } = font;

    // The face first, and both read before either is handed over: they are one font, and a table whose
    // face could not be read is coverage for a font that is not going to be installed.
    let face_bytes = match &face {
        Some(path) => match read_card_file(path, FONT_FACE_MAX) {
            Some(bytes) => Some(bytes),
            None => return,
        },
        None => None,
    };

    let Some(table_bytes) = read_card_file(&table, FONT_TABLE_MAX) else {
        return;
    };

    // Leaked because that is what "as long as the program runs" costs: the renderer holds the table by
    // reference and reads a glyph out of it for every glyph it draws.
    let tables: &'static [&'static [u8]] = Box::leak(vec![table_bytes].into_boxed_slice());

    // Preferred before the face is installed, and refused here rather than left for the first frame, which
    // is the only reason this is not simply a couple of `install` calls: a file that is not a table is a
    // file somebody renamed, and the image's own font is still what the box draws with.
    if let Err(error) = iced_pomelo_winit::fonts::prefer_tables(tables) {
        eprintln!("[font] {table} is not a glyph table ({error}); drawing from the built-in one");
        return;
    }

    let Some(face_bytes) = face_bytes else {
        // A table on its own is coverage for the font in this image — the face that is installed either
        // way, and the one `fonts` binds it to if the header agrees that it was baked from it.
        println!("[font] a glyph table on the card: {table}");
        return;
    };

    // `install` is what binds the two: it is the one call that knows which face the interface ends up
    // drawn with, and it asks the table's header whether that is the face it was baked from. A card's
    // face with a table baked from this image's subset would draw the wrong glyphs rather than none, so
    // neither is bound — the device rasterises instead, which is slow and correct.
    iced_pomelo_winit::fonts::install(face_bytes);
    println!(
        "[font] drawing from {} with {table}",
        face.as_deref().unwrap_or_default()
    );
}

/// The bytes of a file on the card, if it is there and no bigger than `max`, leaked for the life of the
/// program.
///
/// Leaked because that is what the renderer's borrow costs: it holds a font and its table by reference
/// and reads a glyph out of them for every glyph it draws, and a card has no `mmap` to borrow from.
///
/// Every refusal is a line and a `None`, and none of them is a panic — see [`prefer_card_font`] for why
/// that is the shape this takes: the box has a panel to bring up, and it has a font already.
fn read_card_file(path: &str, max: u64) -> Option<&'static [u8]> {
    let Ok(metadata) = std::fs::metadata(path) else {
        eprintln!("[font] {path} cannot be read");
        return None;
    };

    if metadata.len() > max {
        eprintln!(
            "[font] {path} is {} B, over the {max} B this boot reads",
            metadata.len()
        );
        return None;
    }

    // Asked for before it is filled rather than grown into: a `Vec` that reallocates on the way does so
    // by aborting when it cannot, and these are the allocations in the boot that a file's size chooses.
    let mut bytes: Vec<u8> = Vec::new();
    if bytes.try_reserve_exact(metadata.len() as usize).is_err() {
        eprintln!(
            "[font] no room for the {} B of {path}; keeping the font this image came with",
            metadata.len()
        );
        return None;
    }

    if let Err(error) = std::fs::File::open(path).and_then(|mut file| file.read_to_end(&mut bytes)) {
        eprintln!("[font] {path} cannot be read: {error}");
        return None;
    }

    Some(Box::leak(bytes.into_boxed_slice()))
}
