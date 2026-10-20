//! Writing a HiveBox application.
//!
//! An application is a wasm module that draws by filling in a list of commands and reacts to a
//! stream of touches. There is no widget tree to hand over and no retained layout: [`App::view`] is
//! called for one frame, writes what that frame looks like into [`Ui`], and returns. That is the whole
//! interface, and it is the shape that crosses an interpreter boundary cheaply.
//!
//! ```ignore
//! #![no_std]
//! use app_sdk::{app, App, Context, Event, Ui};
//!
//! struct Counter { value: i32 }
//!
//! impl App for Counter {
//!     fn new(_: &Context) -> Self { Self { value: 0 } }
//!
//!     fn update(&mut self, _: Event, ctx: &mut Context) {
//!         if ctx.tapped(Rect::new(24.0, 200.0, 200.0, 64.0)) {
//!             self.value += 1;
//!         }
//!     }
//!
//!     fn view(&mut self, ui: &mut Ui) {
//!         ui.clear(theme::BLACK);
//!         widgets::button(ui, Rect::new(24.0, 200.0, 200.0, 64.0), "+1", theme::ACCENT, theme::TEXT);
//!         ui.text(24.0, 80.0, "42", 48.0, theme::TEXT);
//!     }
//! }
//!
//! app!(Counter);
//! ```
//!
//! # No allocation, on purpose
//!
//! This crate is `no_std` and does not even use `alloc`. An application's state is its own struct,
//! its strings are formatted into fixed buffers ([`text::Buf`]), and its drawing goes into a static
//! display list. The result is a package measured in tens of kilobytes rather than hundreds, and a
//! sandbox whose memory is a known number before it starts.
//!
//! # What the host owns
//!
//! Time, text metrics, preferences, files, and the clock the frames are asked on all come from the
//! host. [`host`] is that surface, and it is also where the network, the box's readings, the
//! speaker, the radio, the machines beside the box and an account's machines live. An application
//! that uses any of them must list the matching [`app_abi::perm`] bit in its metadata, or the host
//! refuses the call before it does anything — see the permission story in [`host`].
//!
//! Nothing there blocks. A request is started, then polled once a frame; an answer is taken once.
//! The one service an application has to be careful with is the NAS shell, whose output is a stream
//! rather than a document — [`host::nas_shell_take`] consumes what arrived since the last call.
//!
//! # The exports
//!
//! [`app!`] emits the eight functions the host looks for, and `hb_*` names are the only `unsafe`
//! (and the only `static mut`) in an application that uses this crate normally.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod host;
pub mod icons;
pub mod input;
pub mod runtime;
pub mod text;
pub mod theme;
pub mod ui;
pub mod widgets;

pub use app_abi;
pub use app_abi::Event;
pub use input::Input;
pub use runtime::Context;
pub use ui::{rgb, Color, Rect, Ui};

/// What an application implements.
pub trait App {
    /// Called once, with the surface it was given.
    fn new(context: &Context) -> Self;

    /// One event. State changes here; drawing happens in [`App::view`].
    fn update(&mut self, event: Event, context: &mut Context);

    /// One frame, into the display list.
    fn view(&mut self, ui: &mut Ui);

    /// The screen is going away but the application is not: drop what can be rebuilt.
    fn on_pause(&mut self) {}

    /// The screen is back.
    fn on_resume(&mut self) {}

    /// The application is about to be destroyed. Flush anything a person would notice losing.
    fn on_stop(&mut self) {}
}

/// The ABI word a package reports: this crate's idea of the version it was built against.
pub fn abi_version() -> u32 {
    app_abi::version_word(app_abi::ABI_MAJOR, app_abi::ABI_MINOR)
}

/// Emits the eight exports a package must have.
///
/// This is the only macro an application needs, and it is deliberately the last line of the file:
/// the functions it defines take their names from the ABI, not from the application, and there can
/// only be one set per module.
#[macro_export]
macro_rules! app {
    ($ty:ty) => {
        static mut HB_APP: ::core::option::Option<$ty> = ::core::option::Option::None;
        static mut HB_CONTEXT: ::core::option::Option<$crate::Context> =
            ::core::option::Option::None;

        #[no_mangle]
        pub extern "C" fn hb_abi_version() -> u32 {
            $crate::abi_version()
        }

        #[no_mangle]
        pub extern "C" fn hb_scratch_ptr() -> u32 {
            $crate::runtime::scratch_ptr()
        }

        #[no_mangle]
        pub extern "C" fn hb_scratch_len() -> u32 {
            $crate::runtime::scratch_len()
        }

        #[no_mangle]
        pub extern "C" fn hb_display_ptr() -> u32 {
            $crate::runtime::display_ptr()
        }

        #[no_mangle]
        pub extern "C" fn hb_display_capacity() -> u32 {
            $crate::runtime::display_capacity()
        }

        #[no_mangle]
        pub extern "C" fn hb_init(width: i32, height: i32, scale_q8: i32, now_ms: i64) -> u32 {
            let context = $crate::Context::new(width, height, scale_q8 as f32 / 256.0, now_ms);
            let app = <$ty as $crate::App>::new(&context);

            unsafe {
                *::core::ptr::addr_of_mut!(HB_APP) = ::core::option::Option::Some(app);
                *::core::ptr::addr_of_mut!(HB_CONTEXT) = ::core::option::Option::Some(context);
            }

            0
        }

        #[no_mangle]
        pub extern "C" fn hb_event(len: u32) -> u32 {
            let Some(event) = $crate::runtime::read_event(len) else {
                return 1;
            };

            unsafe {
                let app = &mut *::core::ptr::addr_of_mut!(HB_APP);
                let context = &mut *::core::ptr::addr_of_mut!(HB_CONTEXT);

                let (Some(app), Some(context)) = (app.as_mut(), context.as_mut()) else {
                    return 1;
                };

                context.input.apply(event);

                if event.kind == $crate::app_abi::event::PAUSE {
                    $crate::App::on_pause(app);
                } else if event.kind == $crate::app_abi::event::RESUME {
                    $crate::App::on_resume(app);
                } else {
                    $crate::App::update(app, event, context);
                }

                if context.close_requested() {
                    $crate::host::exit();
                }
            }

            0
        }

        #[no_mangle]
        pub extern "C" fn hb_frame(now_ms: i64) -> u32 {
            unsafe {
                let app = &mut *::core::ptr::addr_of_mut!(HB_APP);
                let context = &mut *::core::ptr::addr_of_mut!(HB_CONTEXT);

                let (Some(app), Some(context)) = (app.as_mut(), context.as_mut()) else {
                    return 0;
                };

                context.now_ms = now_ms;

                let mut ui = $crate::runtime::begin(context.width, context.height);
                $crate::App::view(app, &mut ui);
                let count = ui.len() as u32;
                context.input.end_frame();

                count
            }
        }

        #[no_mangle]
        pub extern "C" fn hb_pause() {
            unsafe {
                let app = &mut *::core::ptr::addr_of_mut!(HB_APP);

                if let Some(app) = app.as_mut() {
                    $crate::App::on_pause(app);
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn hb_resume() {
            unsafe {
                let app = &mut *::core::ptr::addr_of_mut!(HB_APP);

                if let Some(app) = app.as_mut() {
                    $crate::App::on_resume(app);
                }
            }
        }

        #[no_mangle]
        pub extern "C" fn hb_stop() {
            unsafe {
                let app = &mut *::core::ptr::addr_of_mut!(HB_APP);

                if let Some(app) = app.as_mut() {
                    $crate::App::on_stop(app);
                }

                *::core::ptr::addr_of_mut!(HB_APP) = ::core::option::Option::None;
                *::core::ptr::addr_of_mut!(HB_CONTEXT) = ::core::option::Option::None;
            }
        }
    };
}

/// The "allocator" a package links against.
///
/// A `cdylib` compiled for wasm is required to name one even when nothing in it allocates, and
/// nothing in this SDK does: an application's state is a struct, its strings are fixed buffers and
/// its drawing is a static array. This one hands out nothing, which is the truth rather than a
/// limitation — a package that reaches for the heap is a package with a bug, and it fails at the
/// allocation rather than growing a heap the sandbox did not budget for.
#[cfg(target_arch = "wasm32")]
mod no_allocator {
    use core::alloc::{GlobalAlloc, Layout};

    struct NoAllocator;

    unsafe impl GlobalAlloc for NoAllocator {
        unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
            core::ptr::null_mut()
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    #[global_allocator]
    static ALLOCATOR: NoAllocator = NoAllocator;
}

/// A panic in an application stops that application, not the box.
///
/// The host sees the trap, drops the instance and draws its own sentence. Nothing is unwound because
/// there is nothing to unwind: an application's useful state is in its struct and its files, and the
/// struct is about to be dropped anyway.
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}
