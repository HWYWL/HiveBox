//! Rust firmware entrypoint.

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
