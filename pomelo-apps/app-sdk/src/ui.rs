//! Drawing: a display list, built one command at a time.
//!
//! Nothing here retains a tree, measures a constraint or runs a layout algorithm. [`App::view`]
//! writes what the frame looks like — a rectangle here, a string there — into a fixed buffer, and the
//! host draws it. That is why a frame is one call across the sandbox boundary rather than one per
//! pixel, and why an application's drawing code reads like a description of a screen.
//!
//! Coordinates are logical pixels of whatever surface the host said it has; the panel is 480 × 480.
//! Colours are `0x00RRGGBB` and the constants in [`crate::theme`] are the interface's own.
//!
//! # The buffer is finite, and full is not a crash
//!
//! A frame may hold [`crate::runtime::COMMAND_CAPACITY`] commands and
//! [`crate::runtime::TEXT_CAPACITY`] bytes of string. Drawing past either is dropped and
//! [`Ui::overflowed`] says so — a screen that lost its last few lines is a bug to fix, not a panic to
//! recover from.

use app_abi::{align, op, Align, Command, MAX_TEXT_BYTES};

use crate::runtime::{COMMAND_CAPACITY, TEXT_CAPACITY};

/// A colour, `0x00RRGGBB`.
pub type Color = u32;

/// Builds a colour out of its three channels, the way the interface's constants are written.
pub const fn rgb(red: u8, green: u8, blue: u8) -> Color {
    ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}

/// A rectangle in logical pixels: the one shape every layout here is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether a point is inside, edges included.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// A rectangle inset by the same amount on every side.
    pub fn inset(&self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - amount * 2.0).max(0.0),
            height: (self.height - amount * 2.0).max(0.0),
        }
    }

    /// A rectangle moved to a new origin, keeping its size.
    pub fn at(&self, x: f32, y: f32) -> Self {
        Self { x, y, ..*self }
    }

    /// The part of a horizontal band this rectangle occupies: the layout two columns are built with.
    pub fn slice_x(&self, offset: f32, width: f32) -> Self {
        Self {
            x: self.x + offset,
            y: self.y,
            width,
            height: self.height,
        }
    }
}

/// One frame's display list.
pub struct Ui {
    commands: &'static mut [Command],
    count: usize,
    text: &'static mut [u8],
    text_len: usize,
    /// The surface the host gave this frame.
    pub width: f32,
    pub height: f32,
    overflowed: bool,
}

impl Ui {
    /// Builds a frame over buffers of the ABI's maximum size.
    pub fn new(
        commands: &'static mut [Command],
        text: &'static mut [u8],
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            commands,
            count: 0,
            text,
            text_len: 0,
            width,
            height,
            overflowed: false,
        }
    }

    /// How many commands the frame holds.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether a command or a string was dropped for want of room.
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// How many bytes of string the frame holds.
    pub fn text_used(&self) -> usize {
        self.text_len
    }

    /// Fills the whole surface.
    pub fn clear(&mut self, color: Color) {
        let mut command = Command::new(op::CLEAR);
        command.color = color;
        self.push(command);
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        let mut command = Command::new(op::RECT);
        self.geometry(&mut command, rect);
        command.color = color;
        self.push(command);
    }

    pub fn rounded_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        let mut command = Command::new(op::ROUNDED_RECT);
        self.geometry(&mut command, rect);
        command.color = color;
        command.arg0 = radius.max(0.0) as u32;
        self.push(command);
    }

    pub fn rounded_stroke(&mut self, rect: Rect, radius: f32, stroke: f32, color: Color) {
        let mut command = Command::new(op::ROUNDED_STROKE);
        self.geometry(&mut command, rect);
        command.color = color;
        command.arg0 = radius.max(0.0) as u32;
        command.arg1 = stroke.max(1.0) as u32;
        self.push(command);
    }

    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), width: f32, color: Color) {
        let mut command = Command::new(op::LINE);
        command.x = from.0 as i32 as u32;
        command.y = from.1 as i32 as u32;
        command.w = to.0 as i32 as u32;
        command.h = to.1 as i32 as u32;
        command.color = color;
        command.arg0 = width.max(1.0) as u32;
        self.push(command);
    }

    pub fn circle(&mut self, center: (f32, f32), radius: f32, color: Color) {
        let mut command = Command::new(op::CIRCLE);
        command.x = center.0 as i32 as u32;
        command.y = center.1 as i32 as u32;
        command.w = radius.max(0.0) as u32;
        command.color = color;
        self.push(command);
    }

    /// A Material Symbols glyph, drawn at `size` pixels.
    pub fn icon(&mut self, x: f32, y: f32, size: f32, color: Color, glyph: char) {
        let mut command = Command::new(op::ICON);
        command.x = x as i32 as u32;
        command.y = y as i32 as u32;
        command.h = size.max(1.0) as u32;
        command.color = color;
        command.arg0 = glyph as u32;
        self.push(command);
    }

    /// Text with its top left at `(x, y)`, unbounded to the right.
    pub fn text(&mut self, x: f32, y: f32, text: &str, size: f32, color: Color) {
        self.text_aligned(x, y, 0.0, text, size, color, Align::Left);
    }

    /// Text fitted to a rectangle's width with its own alignment.
    pub fn text_in(&mut self, rect: Rect, text: &str, size: f32, color: Color, align: Align) {
        self.text_aligned(rect.x, rect.y, rect.width, text, size, color, align);
    }

    /// Text centred in a rectangle, horizontally and vertically by the caller's own baseline.
    pub fn text_centered(&mut self, rect: Rect, text: &str, size: f32, color: Color) {
        let y = rect.y + (rect.height - size) / 2.0;
        self.text_aligned(rect.x, y, rect.width, text, size, color, Align::Center);
    }

    /// A string's width in the host's font. Costs one call across the boundary: layout that needs it
    /// should ask once, not once per character.
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        crate::host::text_width(text, size) as f32
    }

    /// Everything drawn after this is clipped to `rect`, until [`Ui::clip_end`].
    pub fn clip(&mut self, rect: Rect) {
        let mut command = Command::new(op::PUSH_CLIP);
        self.geometry(&mut command, rect);
        self.push(command);
    }

    pub fn clip_end(&mut self) {
        self.push(Command::new(op::POP_CLIP));
    }

    fn text_aligned(
        &mut self,
        x: f32,
        y: f32,
        max_width: f32,
        text: &str,
        size: f32,
        color: Color,
        align: Align,
    ) {
        if text.is_empty() {
            return;
        }

        let bytes = text.as_bytes();
        let room = self.text.len().saturating_sub(self.text_len);
        let mut len = bytes.len().min(room).min(MAX_TEXT_BYTES);

        // Truncating in the middle of a UTF-8 sequence would draw a replacement glyph for a
        // character the frame did not have room for; cutting at a boundary draws one fewer.
        while len > 0 && !text.is_char_boundary(len) {
            len -= 1;
        }

        if len == 0 {
            self.overflowed = true;
            return;
        }

        if len < bytes.len() {
            self.overflowed = true;
        }

        let start = self.text_len;
        self.text[start..start + len].copy_from_slice(&bytes[..len]);
        self.text_len += len;

        let pointer = unsafe { self.text.as_ptr().add(start) } as usize as u32;

        let mut command = Command::new(op::TEXT);
        command.x = x as i32 as u32;
        command.y = y as i32 as u32;
        command.w = max_width.max(0.0) as u32;
        command.h = size.max(1.0) as u32;
        command.color = color;
        command.arg0 = pointer;
        command.arg1 = len as u32;
        command.arg2 = match align {
            Align::Left => align::LEFT,
            Align::Center => align::CENTER,
            Align::Right => align::RIGHT,
        };

        self.push(command);
    }

    fn geometry(&self, command: &mut Command, rect: Rect) {
        command.x = rect.x as i32 as u32;
        command.y = rect.y as i32 as u32;
        command.w = rect.width.max(0.0) as i32 as u32;
        command.h = rect.height.max(0.0) as i32 as u32;
    }

    fn push(&mut self, command: Command) {
        if self.count < self.commands.len() {
            self.commands[self.count] = command;
            self.count += 1;
        } else {
            self.overflowed = true;
        }
    }
}

const _: () = {
    // The buffers the runtime hands `Ui` are exactly this size; a frame that could not hold what the
    // ABI allows would be a limit no application was told about.
    assert!(COMMAND_CAPACITY <= app_abi::MAX_COMMANDS);
    assert!(TEXT_CAPACITY > 0);
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;
    use std::vec;

    fn buffers() -> (&'static mut [Command], &'static mut [u8]) {
        let commands = Box::leak(vec![Command::new(0); COMMAND_CAPACITY].into_boxed_slice());
        let text = Box::leak(vec![0u8; TEXT_CAPACITY].into_boxed_slice());
        (commands, text)
    }

    #[test]
    fn a_rectangle_and_a_string_become_a_command_each() {
        let (commands, text) = buffers();
        let text_base = text.as_ptr() as usize;
        let mut ui = Ui::new(commands, text, 480.0, 480.0);

        ui.clear(0x000000);
        ui.rounded_rect(Rect::new(10.0, 20.0, 100.0, 40.0), 12.0, 0x00FF_0000);
        ui.text(20.0, 30.0, "hello", 24.0, 0x00FF_FFFF);

        assert_eq!(ui.len(), 3);
        assert!(!ui.overflowed());

        let text_command = ui.commands[2];
        assert_eq!(text_command.op, op::TEXT);
        assert_eq!(text_command.arg1, 5);
        // The ABI's pointers are 32-bit because the sandbox's address space is; on a 64-bit test
        // host the value is the low half of the buffer's address, which is the same number a wasm
        // module would have.
        assert_eq!(text_command.arg0, text_base as u32);
    }

    /// A frame that fills the buffer stops drawing and says so, rather than writing past the end.
    #[test]
    fn a_full_frame_drops_commands_and_reports_it() {
        let (commands, text) = buffers();
        let mut ui = Ui::new(commands, text, 480.0, 480.0);

        for _ in 0..COMMAND_CAPACITY + 10 {
            ui.clear(0);
        }

        assert_eq!(ui.len(), COMMAND_CAPACITY);
        assert!(ui.overflowed());
    }

    /// A string that does not fit is cut at a character boundary, never in the middle of a UTF-8
    /// sequence.
    #[test]
    fn a_string_is_cut_where_a_character_ends() {
        let commands = Box::leak(vec![Command::new(0); COMMAND_CAPACITY].into_boxed_slice());
        let text = Box::leak(vec![0u8; 7].into_boxed_slice());
        let mut ui = Ui::new(commands, text, 480.0, 480.0);

        // Four three-byte characters into seven bytes: the second one does not fit and is not
        // split.
        ui.text(0.0, 0.0, "中文中文", 24.0, 0);

        assert_eq!(ui.len(), 1);
        assert_eq!(ui.commands[0].arg1, 6);
        assert!(ui.overflowed());
    }

    #[test]
    fn alignment_is_the_hosts_job_and_costs_no_measurement() {
        let (commands, text) = buffers();
        let mut ui = Ui::new(commands, text, 480.0, 480.0);

        ui.text_centered(Rect::new(0.0, 100.0, 480.0, 40.0), "centred", 24.0, 0);

        assert_eq!(ui.commands[0].arg2, align::CENTER);
        assert_eq!(ui.commands[0].w, 480);
    }
}
