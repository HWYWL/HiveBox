//! The buffers an application draws into, and the state between events.
//!
//! Everything here is a `static mut` because a wasm module has one thread and one instance: the host
//! writes an event into [`SCRATCH`], calls `hb_event`, and the macro in the crate root reads it; a
//! frame is written into [`COMMANDS`] and [`TEXT`], whose addresses the host asks for once. The
//! `unsafe` is confined to this module and to the macro that drives it — an application never takes a
//! reference to any of it.

use app_abi::{Command, Event};

use crate::input::Input;
use crate::ui::Ui;

/// How many commands one frame may hold.
///
/// The ABI allows [`app_abi::MAX_COMMANDS`]; this is what the SDK actually gives an application. It
/// is a static array in the module's data, so every one of these is memory the sandbox is measured
/// against — 512 commands is twenty kilobytes and more than any page here has ever drawn.
pub const COMMAND_CAPACITY: usize = 512;

/// How many bytes of string one frame may hold.
pub const TEXT_CAPACITY: usize = 12 * 1024;

/// How many bytes of event the host may write.
pub const SCRATCH_LEN: usize = 256;

static mut COMMANDS: [Command; COMMAND_CAPACITY] = [Command::new(0); COMMAND_CAPACITY];
static mut TEXT: [u8; TEXT_CAPACITY] = [0; TEXT_CAPACITY];
static mut SCRATCH: [u8; SCRATCH_LEN] = [0; SCRATCH_LEN];

/// What an application knows about the surface and the moment, between events.
#[derive(Clone, Debug)]
pub struct Context {
    pub width: f32,
    pub height: f32,
    /// The host's display scale, `1.0` on the panel.
    pub scale: f32,
    /// Milliseconds since boot, updated once a frame.
    pub now_ms: i64,
    pub input: Input,
    close: bool,
}

impl Context {
    pub fn new(width: i32, height: i32, scale: f32, now_ms: i64) -> Self {
        Self {
            width: width.max(1) as f32,
            height: height.max(1) as f32,
            scale: scale.max(0.25),
            now_ms,
            input: Input::default(),
            close: false,
        }
    }

    /// Whether the tap that ended in this frame landed inside a rectangle.
    pub fn tapped(&self, rect: crate::ui::Rect) -> bool {
        self.input.tapped(rect)
    }

    /// The surface, as a rectangle.
    pub fn screen(&self) -> crate::ui::Rect {
        crate::ui::Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// Asks the host to close the application at the end of this event.
    pub fn close(&mut self) {
        self.close = true;
    }

    pub fn close_requested(&self) -> bool {
        self.close
    }
}

/// A frame over the static buffers.
pub fn begin(width: f32, height: f32) -> Ui {
    unsafe {
        Ui::new(
            &mut *core::ptr::addr_of_mut!(COMMANDS),
            &mut *core::ptr::addr_of_mut!(TEXT),
            width,
            height,
        )
    }
}

pub fn scratch_ptr() -> u32 {
    core::ptr::addr_of!(SCRATCH) as *const u8 as u32
}

pub fn scratch_len() -> u32 {
    SCRATCH_LEN as u32
}

pub fn display_ptr() -> u32 {
    core::ptr::addr_of!(COMMANDS) as *const Command as u32
}

pub fn display_capacity() -> u32 {
    COMMAND_CAPACITY as u32
}

/// Reads the event the host wrote into the scratch buffer.
///
/// A length that is not exactly one event is not one: the host and the ABI agree on
/// [`Event::LEN`], and a host that sent something else is a host this package does not understand.
pub fn read_event(len: u32) -> Option<Event> {
    if len as usize != Event::LEN {
        return None;
    }

    let bytes = unsafe { &*core::ptr::addr_of!(SCRATCH) };
    let word = |at: usize| i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);

    Some(Event::new(
        word(0) as u32,
        word(4),
        word(8),
        word(12),
        word(16),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_goes_into_the_buffers_the_host_reads() {
        let mut ui = begin(480.0, 480.0);
        ui.clear(0x0012_3456);
        assert_eq!(ui.len(), 1);

        let pointer = display_ptr();
        assert_eq!(
            pointer,
            core::ptr::addr_of!(COMMANDS) as *const _ as usize as u32
        );
        assert_eq!(display_capacity(), COMMAND_CAPACITY as u32);
    }

    #[test]
    fn an_event_is_read_out_of_the_scratch_buffer() {
        let event = Event::new(app_abi::event::TOUCH_DOWN, 12, 34, 0, 0);
        let words = [
            event.kind,
            event.x as u32,
            event.y as u32,
            event.a as u32,
            event.b as u32,
        ];

        unsafe {
            let scratch = &mut *core::ptr::addr_of_mut!(SCRATCH);

            for (at, word) in words.iter().enumerate() {
                scratch[at * 4..at * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
        }

        assert_eq!(read_event(Event::LEN as u32), Some(event));
        assert_eq!(read_event(3), None);
    }

    #[test]
    fn the_context_starts_open_and_has_a_surface() {
        let context = Context::new(480, 480, 1.0, 7);
        assert!(!context.close_requested());
        assert_eq!(context.screen().width, 480.0);
        assert_eq!(context.now_ms, 7);
    }
}
