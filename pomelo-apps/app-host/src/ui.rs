//! The iced side of the runtime: a [`DisplayList`] as an element.
//!
//! # Why shapes are canvas and words are not
//!
//! Everything an application draws that is a rectangle, a line or a circle goes through iced's canvas:
//! one path per command, recorded once and rasterised as one geometry. Text does **not**: `fill_text`
//! is a `todo!()` in the panel's geometry backend — see `iced-pomelo-gfx` — and a page that panicked
//! the moment an application wrote a label would be a runtime that runs nothing. So a text command
//! becomes an ordinary `text` widget, absolutely positioned over the canvas, and an icon becomes the
//! same widget in the icon font. It is the fallback the plan named, and it has an advantage worth
//! keeping: the words are laid out by the same engine as every other word on this panel, so an
//! application's label and the launcher's own labels are the same shape.
//!
//! # What is not drawn
//!
//! `PushClip` and `PopClip` are dropped. iced's canvas has no clip stack a host can push, and drawing
//! unclipped is the failure that shows less, not the one that shows the wrong thing. Applications that
//! need a scroll should draw their own viewport — the SDK's list helper does.
//!
//! # Coordinates
//!
//! The ABI's pixels are the surface's logical pixels, which is what the element is given: the
//! launcher hands a page the whole panel, so a command at `(0, 0)` is the top-left of the screen.

use app_abi::align;
use iced::widget::canvas::{self, Action, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{container, text, Stack};
use iced::{
    mouse, Alignment, Color, Element, Font, Length, Padding, Point, Rectangle, Size, Theme,
};

use crate::display::{DisplayList, DrawCommand};

/// The two faces a display list may name: the interface's own, and the icon font.
///
/// Passed in rather than assumed, because the font an icon is drawn in belongs to whoever installed
/// it — the launcher's program — and this crate deliberately carries no fonts of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fonts {
    /// What a `TEXT` command is drawn in.
    pub text: Font,
    /// What an `ICON` command is drawn in.
    pub icons: Font,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            text: Font::DEFAULT,
            icons: Font::DEFAULT,
        }
    }
}

/// One frame as an element: the shapes as a canvas, the words over it.
///
/// `input` is how the caller hears about a finger: the canvas turns a press, a drag and a release
/// into the three [`app_abi::event`] kinds the ABI names and hands each one to this closure, in the
/// application's own coordinates. It is a closure rather than a `Message` because the message type
/// belongs to the launcher, and an index — *which* application this page is — belongs beside it.
pub fn view<'a, Message: 'a, F>(
    list: &'a DisplayList,
    fonts: Fonts,
    input: F,
) -> Element<'a, Message>
where
    F: Fn(app_abi::Event) -> Message + 'a,
{
    let shapes = Canvas::new(Shapes { list, input })
        .width(Length::Fill)
        .height(Length::Fill);

    let mut layers: Vec<Element<'a, Message>> = vec![shapes.into()];

    for command in &list.commands {
        match command {
            DrawCommand::Text {
                x,
                y,
                max_width,
                size,
                color,
                align,
                text: content,
            } => layers.push(word(
                content,
                *x,
                *y,
                *max_width,
                *size,
                argb(*color),
                alignment(*align),
                fonts.text,
            )),
            DrawCommand::Icon {
                x,
                y,
                size,
                color,
                glyph,
            } => layers.push(word(
                &glyph.to_string(),
                *x,
                *y,
                0.0,
                *size,
                argb(*color),
                Alignment::Start,
                fonts.icons,
            )),
            // The rest are the canvas's, above.
            _ => {}
        }
    }

    if layers.len() == 1 {
        return layers.pop().expect("one layer was just pushed");
    }

    Stack::with_children(layers).width(Length::Fill).height(Length::Fill).into()
}

/// One string at `(x, y)`, within `max_width`, aligned the way the command said.
fn word<'a, Message: 'a>(
    content: &str,
    x: f32,
    y: f32,
    max_width: f32,
    size: f32,
    color: Color,
    alignment: Alignment,
    font: Font,
) -> Element<'a, Message> {
    let mut line: Element<'a, Message> = text(content.to_string())
        .size(size)
        .color(color)
        .font(font)
        .into();

    // A command that named a width is one whose application wants its text centred or right-aligned
    // in that box, and it is the box that says where the alignment starts. Without one the string
    // is as wide as it is.
    if max_width > 0.0 {
        line = container(line)
            .width(Length::Fixed(max_width))
            .align_x(alignment)
            .into();
    }

    container(line)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .padding(Padding {
            top: y.max(0.0),
            left: x.max(0.0),
            right: 0.0,
            bottom: 0.0,
        })
        .into()
}

/// The ABI writes colours as `0x00RRGGBB`.
fn argb(color: u32) -> Color {
    Color::from_rgb8(
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
    )
}

fn alignment(value: u32) -> Alignment {
    match value {
        align::CENTER => Alignment::Center,
        align::RIGHT => Alignment::End,
        _ => Alignment::Start,
    }
}

/// The shapes of one display list, recorded as a canvas, and the finger on it.
struct Shapes<'a, F> {
    list: &'a DisplayList,
    input: F,
}

impl<Message, Renderer, F> canvas::Program<Message, Theme, Renderer> for Shapes<'_, F>
where
    F: Fn(app_abi::Event) -> Message,
    Renderer: iced::advanced::graphics::geometry::Renderer + 'static,
{
    /// Whether a finger is down: a move is only part of a drag if a press started it, and a release
    /// that never had a press is a gesture that began on the page behind this one.
    type State = bool;

    fn update(
        &self,
        pressed: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let kind = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                *pressed = true;
                app_abi::event::TOUCH_DOWN
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if *pressed => {
                app_abi::event::TOUCH_MOVE
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                *pressed = false;
                app_abi::event::TOUCH_UP
            }
            _ => return None,
        };

        // The canvas's coordinates rather than the panel's: a command at `(0, 0)` is the top-left of
        // the surface the application was opened on, which is where its own hit testing looks.
        let position = cursor.position()?;
        let event = app_abi::Event::new(
            kind,
            (position.x - bounds.x) as i32,
            (position.y - bounds.y) as i32,
            0,
            0,
        );

        Some(Action::publish((self.input)(event)).and_capture())
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());

        for command in &self.list.commands {
            draw(&mut frame, command, bounds.size());
        }

        vec![frame.into_geometry()]
    }
}

/// One command, on the canvas.
fn draw<Renderer>(frame: &mut Frame<Renderer>, command: &DrawCommand, surface: Size)
where
    Renderer: iced::advanced::graphics::geometry::Renderer + 'static,
{
    match command {
        // A clear is the whole surface filled, because that is what it means to the application: not
        // "forget the last frame" but "this colour is behind everything".
        DrawCommand::Clear { color } => {
            frame.fill_rectangle(Point::ORIGIN, surface, argb(*color));
        }
        DrawCommand::Rect {
            x,
            y,
            width,
            height,
            color,
        } => {
            frame.fill_rectangle(
                Point::new(*x, *y),
                Size::new(width.max(0.0), height.max(0.0)),
                argb(*color),
            );
        }
        DrawCommand::RoundedRect {
            x,
            y,
            width,
            height,
            radius,
            color,
        } => {
            let path = Path::rounded_rectangle(
                Point::new(*x, *y),
                Size::new(width.max(0.0), height.max(0.0)),
                (*radius).into(),
            );

            frame.fill(&path, argb(*color));
        }
        DrawCommand::RoundedStroke {
            x,
            y,
            width,
            height,
            radius,
            stroke,
            color,
        } => {
            let path = Path::rounded_rectangle(
                Point::new(*x, *y),
                Size::new(width.max(0.0), height.max(0.0)),
                (*radius).into(),
            );

            frame.stroke(
                &path,
                Stroke {
                    style: argb(*color).into(),
                    width: *stroke,
                    ..Stroke::default()
                },
            );
        }
        DrawCommand::Line {
            from,
            to,
            width,
            color,
        } => {
            if (from.0 - to.0).abs() < f32::EPSILON && (from.1 - to.1).abs() < f32::EPSILON {
                return;
            }

            frame.stroke(
                &Path::line(Point::new(from.0, from.1), Point::new(to.0, to.1)),
                Stroke {
                    style: argb(*color).into(),
                    width: *width,
                    ..Stroke::default()
                },
            );
        }
        DrawCommand::Circle {
            x,
            y,
            radius,
            color,
        } => {
            frame.fill(&Path::circle(Point::new(*x, *y), *radius), argb(*color));
        }
        // The two the canvas cannot do, and the words, which are layers above it.
        DrawCommand::Text { .. } | DrawCommand::Icon { .. } | DrawCommand::PushClip { .. }
        | DrawCommand::PopClip => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::display::DisplayList;

    fn list(commands: Vec<DrawCommand>) -> DisplayList {
        DisplayList { commands }
    }

    /// Every shape is recorded, and a text command becomes a layer rather than a panic: the panel's
    /// rasteriser has no `fill_text`, so this is the one property that has to hold on both backends.
    #[test]
    fn every_command_becomes_an_element() {
        let list = list(vec![
            DrawCommand::Clear { color: 0x0010_1010 },
            DrawCommand::Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                color: 0x00FF_FFFF,
            },
            DrawCommand::RoundedRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                radius: 3.0,
                color: 0x00FF_FFFF,
            },
            DrawCommand::RoundedStroke {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                radius: 3.0,
                stroke: 2.0,
                color: 0x00FF_FFFF,
            },
            DrawCommand::Line {
                from: (0.0, 0.0),
                to: (10.0, 10.0),
                width: 1.0,
                color: 0x00FF_FFFF,
            },
            DrawCommand::Text {
                x: 12.0,
                y: 20.0,
                max_width: 100.0,
                size: 18.0,
                color: 0x00FF_FFFF,
                align: align::CENTER,
                text: "7 × 8".to_string(),
            },
            DrawCommand::Icon {
                x: 0.0,
                y: 0.0,
                size: 24.0,
                color: 0x00FF_FFFF,
                glyph: '\u{e8b0}',
            },
            DrawCommand::Circle {
                x: 5.0,
                y: 5.0,
                radius: 2.0,
                color: 0x00FF_FFFF,
            },
            DrawCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            DrawCommand::PopClip,
        ]);

        let _: Element<'_, ()> = view(&list, Fonts::default(), |_| ());
    }

    /// An empty frame is a canvas and nothing else: an application that has not drawn yet is a black
    /// screen rather than a missing page.
    #[test]
    fn an_empty_list_is_one_blank_canvas() {
        let empty = DisplayList::default();
        let _: Element<'_, ()> = view(&empty, Fonts::default(), |_| ());
    }

    /// The three alignments the ABI names are three different alignments, and an unknown one is the
    /// left edge rather than a panic.
    #[test]
    fn the_alignments_are_told_apart() {
        assert_eq!(alignment(align::LEFT), Alignment::Start);
        assert_eq!(alignment(align::CENTER), Alignment::Center);
        assert_eq!(alignment(align::RIGHT), Alignment::End);
        assert_eq!(alignment(99), Alignment::Start);
    }

    /// The colour word is `0x00RRGGBB` rather than the panel's native order, which is the one thing
    /// every command carries and the one thing a wrong reading of would paint every page wrong.
    #[test]
    fn a_colour_word_is_read_as_rgb() {
        assert_eq!(
            argb(0x00F6_9E22),
            Color::from_rgb8(0xF6, 0x9E, 0x22)
        );
    }
}
