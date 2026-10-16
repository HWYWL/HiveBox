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

/// The most a glyph table may be, in bytes, before this boot refuses to read it.
///
/// The 24 px table this image carries is 234 KiB, so this is several times the real thing and still
/// leaves room for a table baked over a much wider charset — which is the whole point of reading one off
/// a card. It is a bound on a *surprising* file rather than on the real one, and it is here because of
/// where these bytes come from: a card, written by a download. A boot that read whatever size a file
/// claimed is a boot that one bad file could take the heap with, and not reading it is the honest answer
/// to "this is not a glyph table".
const FONT_TABLE_MAX: u64 = 2 * 1024 * 1024;

/// Draws from the newest glyph table on the card, if there is one, instead of the one in this image.
///
/// The image carries a 24 px table covering exactly the characters this source tree writes, which is what
/// lets a box with no card draw its own interface. That is enough for the interface and not for
/// everything that reaches the panel: a file name read off a card, a Wi-Fi name, a line of text from a
/// server — characters nobody wrote into the source — have no glyph in the subset, and draw as boxes.
///
/// A card carrying a table baked over a wider charset is how those get drawn without a reflash: the
/// update page writes newer tables to `updater::FONT_DIR`, and this is the line that makes the newest of
/// them the one the interface is drawn with.
///
/// Every way this can fail ends with the baked table still in place: no card, no table on it, a file too
/// big to be one, bytes that are not a table at all. That is not politeness. The boot has a panel to
/// bring up and somebody watching it, and "there is no font" is not among the things worth doing about a
/// file — the interface that draws the wrong glyphs is a worse boot than the one that draws the ones it
/// shipped with, but only just, and neither is worth a panic here.
///
/// The bytes are read into memory and leaked, and that is a real cost: a table is borrowed by the
/// renderer for as long as the program runs, a card has no `mmap` to borrow from, and the baked table is
/// the only one that costs no RAM at all because it is read in place out of flash. A quarter of a
/// megabyte, once per boot, is what drawing from the card costs today — and the table on the card is what
/// decides that number, so a download is the one thing that can make this boot allocate more.
fn prefer_card_font() {
    let Some((_, path)) = updater::newest_font_table() else {
        // No card, or nothing on it. The common case for a box that has never been given a table, and
        // not worth a line: the baked one is what this image was built with either way.
        return;
    };

    let Ok(metadata) = std::fs::metadata(&path) else {
        eprintln!("[font] {path} cannot be read");
        return;
    };

    if metadata.len() > FONT_TABLE_MAX {
        eprintln!(
            "[font] {path} is {} B, over the {FONT_TABLE_MAX} B a glyph table may be",
            metadata.len()
        );
        return;
    }

    // Asked for before it is filled rather than grown into: a `Vec` that reallocates on the way does so
    // by aborting when it cannot, and this is the one allocation in the boot that a file's size chooses.
    let mut bytes: Vec<u8> = Vec::new();
    if bytes.try_reserve_exact(metadata.len() as usize).is_err() {
        eprintln!(
            "[font] no room for the {} B of {path}; drawing from the built-in table",
            metadata.len()
        );
        return;
    }

    if let Err(error) = std::fs::File::open(&path).and_then(|mut file| file.read_to_end(&mut bytes)) {
        eprintln!("[font] {path} cannot be read: {error}");
        return;
    }

    // Leaked because that is what "as long as the program runs" costs: the renderer holds the table by
    // reference and reads a glyph out of it for every glyph it draws.
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let tables: &'static [&'static [u8]] = Box::leak(vec![bytes].into_boxed_slice());

    match iced_pomelo_winit::fonts::prefer_tables(tables) {
        Ok(()) => println!("[font] drawing from {path} ({} B)", metadata.len()),
        Err(error) => {
            eprintln!("[font] {path} is not a glyph table ({error}); drawing from the built-in one")
        }
    }
}
