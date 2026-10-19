//! A display list, as numbers the host can draw.
//!
//! The application writes [`app_abi::Command`]s; this module turns that flat array into one
//! [`DrawCommand`] per line and copies any text out of wasm memory on the way. The copy is the point:
//! a [`DisplayList`] outlives the call that produced it, and a pointer into linear memory is only
//! valid while the instance that owns it is alive and has not reallocated. Text is at most
//! [`app_abi::MAX_TEXT_BYTES`] per command and a frame is at most [`app_abi::MAX_COMMANDS`], so the
//! worst case is bounded and known before anything is drawn.
//!
//! Unknown operations are dropped rather than refused: a newer package on an older host draws a
//! plainer screen, which is the degradation [`app_abi`] promises.

use app_abi::{align, op, Command, MAX_COMMANDS, MAX_TEXT_BYTES};

/// One thing to draw, in logical pixels of the 480 × 480 surface.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Clear {
        color: u32,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: u32,
    },
    RoundedRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: u32,
    },
    RoundedStroke {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        stroke: f32,
        color: u32,
    },
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        color: u32,
    },
    Text {
        x: f32,
        y: f32,
        max_width: f32,
        size: f32,
        color: u32,
        align: u32,
        text: String,
    },
    Icon {
        x: f32,
        y: f32,
        size: f32,
        color: u32,
        glyph: char,
    },
    Circle {
        x: f32,
        y: f32,
        radius: f32,
        color: u32,
    },
    PushClip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    PopClip,
}

/// A frame, ready to draw: no pointers, no wasm memory, nothing that can go out of scope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    pub commands: Vec<DrawCommand>,
}

impl DisplayList {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Reads `count` commands from a byte slice, calling `text` for each string a text command
    /// names.
    ///
    /// The caller supplies the string read because only it can reach the instance's memory; this
    /// function does the arithmetic and the validation, which is the part that must not be wrong.
    pub fn parse(
        bytes: &[u8],
        count: usize,
        mut text: impl FnMut(u32, u32) -> String,
    ) -> Self {
        let count = count.min(MAX_COMMANDS).min(bytes.len() / Command::LEN);
        let mut commands = Vec::with_capacity(count);

        for at in 0..count {
            let base = at * Command::LEN;

            let mut words = [0u32; Command::WORDS];

            for (index, word) in words.iter_mut().enumerate() {
                let offset = base + index * 4;
                let chunk = &bytes[offset..offset + 4];
                *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }

            let command = Command::from_words(words);

            if let Some(draw) = convert(command, &mut text) {
                commands.push(draw);
            }
        }

        Self { commands }
    }
}

fn convert(command: Command, text: &mut impl FnMut(u32, u32) -> String) -> Option<DrawCommand> {
    let x = command.x as i32 as f32;
    let y = command.y as i32 as f32;
    let width = command.w as i32 as f32;
    let height = command.h as i32 as f32;

    Some(match command.op {
        op::CLEAR => DrawCommand::Clear {
            color: command.color,
        },
        op::RECT => DrawCommand::Rect {
            x,
            y,
            width,
            height,
            color: command.color,
        },
        op::ROUNDED_RECT => DrawCommand::RoundedRect {
            x,
            y,
            width,
            height,
            radius: command.arg0.min(1024) as f32,
            color: command.color,
        },
        op::ROUNDED_STROKE => DrawCommand::RoundedStroke {
            x,
            y,
            width,
            height,
            radius: command.arg0.min(1024) as f32,
            stroke: (command.arg1.max(1)).min(64) as f32,
            color: command.color,
        },
        op::LINE => DrawCommand::Line {
            from: (x, y),
            to: (width, height),
            width: command.arg0.max(1).min(64) as f32,
            color: command.color,
        },
        op::TEXT => {
            let len = command.arg1.min(MAX_TEXT_BYTES as u32);
            let glyphs = text(command.arg0, len);

            if glyphs.is_empty() {
                return None;
            }

            DrawCommand::Text {
                x,
                y,
                max_width: command.w as i32 as f32,
                size: (command.h as i32).clamp(8, 96) as f32,
                color: command.color,
                align: command.arg2.min(align::RIGHT),
                text: glyphs,
            }
        }
        op::ICON => DrawCommand::Icon {
            x,
            y,
            size: (height as i32).clamp(8, 96) as f32,
            color: command.color,
            glyph: char::from_u32(command.arg0).unwrap_or('\u{FFFD}'),
        },
        op::CIRCLE => DrawCommand::Circle {
            x,
            y,
            radius: (width as i32).clamp(0, 512) as f32,
            color: command.color,
        },
        op::PUSH_CLIP => DrawCommand::PushClip {
            x,
            y,
            width,
            height,
        },
        op::POP_CLIP => DrawCommand::PopClip,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_abi::Command;

    fn command_bytes(commands: &[Command]) -> Vec<u8> {
        let mut bytes = Vec::new();

        for command in commands {
            for word in command.to_words() {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }

        bytes
    }

    #[test]
    fn a_rectangle_keeps_its_geometry_and_colour() {
        let mut command = Command::new(op::RECT);
        command.x = 10;
        command.y = 20;
        command.w = 30;
        command.h = 40;
        command.color = 0x00F6_9E22;

        let list = DisplayList::parse(&command_bytes(&[command]), 1, |_, _| String::new());

        assert_eq!(
            list.commands,
            vec![DrawCommand::Rect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
                color: 0x00F6_9E22
            }]
        );
    }

    /// The string is copied out of the instance by the closure, and the address it is copied from is
    /// the one the command named.
    #[test]
    fn a_text_command_asks_for_its_string_and_keeps_what_came_back() {
        let mut command = Command::new(op::TEXT);
        command.x = 4;
        command.y = 8;
        command.w = 100;
        command.h = 24;
        command.arg0 = 0x2000;
        command.arg1 = 6;
        command.arg2 = align::CENTER;

        let mut asked = None;
        let list = DisplayList::parse(&command_bytes(&[command]), 1, |ptr, len| {
            asked = Some((ptr, len));
            "84%".to_string()
        });

        assert_eq!(asked, Some((0x2000, 6)));

        assert_eq!(
            list.commands,
            vec![DrawCommand::Text {
                x: 4.0,
                y: 8.0,
                max_width: 100.0,
                size: 24.0,
                color: 0,
                align: align::CENTER,
                text: "84%".to_string(),
            }]
        );
    }

    /// An operation this host does not know is skipped rather than ending the frame: a newer package
    /// draws less, not nothing.
    #[test]
    fn an_unknown_operation_is_dropped_and_the_rest_is_drawn() {
        let unknown = Command::new(999);
        let mut rect = Command::new(op::RECT);
        rect.w = 1;
        rect.h = 1;

        let list = DisplayList::parse(&command_bytes(&[unknown, rect]), 2, |_, _| String::new());
        assert_eq!(list.commands.len(), 1);
    }

    /// The count is clamped to what the bytes can hold: a module that returns a million commands
    /// cannot make the host read past the buffer it was given.
    #[test]
    fn the_count_is_clamped_to_the_bytes_that_arrived() {
        let mut rect = Command::new(op::RECT);
        rect.w = 1;
        rect.h = 1;

        let bytes = command_bytes(&[rect]);
        let list = DisplayList::parse(&bytes, 10_000, |_, _| String::new());
        assert_eq!(list.commands.len(), 1);
    }

    /// Text length is clamped to the ABI's maximum, and a text command with nothing in it is not a
    /// draw command.
    #[test]
    fn text_is_clamped_and_an_empty_string_draws_nothing() {
        let mut command = Command::new(op::TEXT);
        command.arg0 = 0;
        command.arg1 = u32::MAX;

        let mut asked = 0;
        DisplayList::parse(&command_bytes(&[command]), 1, |_, len| {
            asked = len;
            String::new()
        });

        assert_eq!(asked, MAX_TEXT_BYTES as u32);
    }
}
