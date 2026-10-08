//! Generic gesture recognition widget for touch and mouse interactions.
//!
//! Provides a flexible, Flutter-inspired gesture recognizer supporting:
//! - Taps and double taps with slop filtering
//! - Continuous pan/drag gestures with delta and velocity tracking
//! - Directional swipe gestures (Left, Right, Up, Down)
//! - Gesture disambiguation (suppressing child clicks when a drag is recognized)

use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::Operation;
use iced::advanced::{Clipboard, Shell, Widget};
use iced::touch;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Details provided when a pan/drag gesture begins.
#[derive(Debug, Clone, Copy)]
pub struct PanStartDetails {
    /// The starting point of the pan gesture.
    pub point: Point,
}

/// Details provided during pan/drag gesture updates.
#[derive(Debug, Clone, Copy)]
pub struct PanUpdateDetails {
    /// Current point of the pointer.
    pub point: Point,
    /// Incremental delta since the previous update frame.
    pub delta: Vector,
    /// Total displacement from the start point.
    pub total_delta: Vector,
}

/// Details provided when a pan/drag gesture ends.
#[derive(Debug, Clone, Copy)]
pub struct PanEndDetails {
    /// Estimated velocity of the gesture (pixels per second).
    pub velocity: Vector,
    /// Total displacement from start to release.
    pub total_delta: Vector,
}

/// A container widget that detects gestures over its contents.
pub struct GestureDetector<
    'a,
    Message,
    Theme = iced::Theme,
    Renderer = iced::Renderer,
> {
    content: Element<'a, Message, Theme, Renderer>,
    on_tap: Option<Box<dyn Fn() -> Message + 'a>>,
    on_tap_at: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    on_double_tap: Option<Box<dyn Fn() -> Message + 'a>>,
    on_press: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    on_release: Option<Box<dyn Fn() -> Message + 'a>>,
    on_swipe_left: Option<Message>,
    on_swipe_right: Option<Message>,
    on_swipe_up: Option<Message>,
    on_swipe_down: Option<Message>,
    on_swipe: Option<Box<dyn Fn(SwipeDirection) -> Message + 'a>>,
    on_pan_start: Option<Box<dyn Fn(PanStartDetails) -> Message + 'a>>,
    on_pan_update: Option<Box<dyn Fn(PanUpdateDetails) -> Message + 'a>>,
    on_pan_end: Option<Box<dyn Fn(PanEndDetails) -> Message + 'a>>,
    on_pan_cancel: Option<Message>,
    touch_slop: f32,
    swipe_threshold: f32,
    double_tap_timeout: std::time::Duration,
    intercept_events: bool,
}

impl<'a, Message, Theme, Renderer> GestureDetector<'a, Message, Theme, Renderer> {
    /// Creates a new [`GestureDetector`] wrapping `content`.
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_tap: None,
            on_tap_at: None,
            on_double_tap: None,
            on_press: None,
            on_release: None,
            on_swipe_left: None,
            on_swipe_right: None,
            on_swipe_up: None,
            on_swipe_down: None,
            on_swipe: None,
            on_pan_start: None,
            on_pan_update: None,
            on_pan_end: None,
            on_pan_cancel: None,
            touch_slop: 18.0,
            swipe_threshold: 40.0,
            double_tap_timeout: std::time::Duration::from_millis(300),
            intercept_events: true,
        }
    }

    /// Sets the message emitted when a tap occurs.
    pub fn on_tap(mut self, message: Message) -> Self
    where
        Message: Clone + 'a,
    {
        self.on_tap = Some(Box::new(move || message.clone()));
        self
    }

    /// Sets the callback for a tap with the tap coordinates.
    pub fn on_tap_at(
        mut self,
        on_tap_at: impl Fn(Point) -> Message + 'a,
    ) -> Self {
        self.on_tap_at = Some(Box::new(on_tap_at));
        self
    }

    /// Sets the message emitted on a double tap.
    pub fn on_double_tap(mut self, message: Message) -> Self
    where
        Message: Clone + 'a,
    {
        self.on_double_tap = Some(Box::new(move || message.clone()));
        self
    }

    /// Sets the message emitted when a pointer presses down.
    pub fn on_press(mut self, message: Message) -> Self
    where
        Message: Clone + 'a,
    {
        self.on_press = Some(Box::new(move |_| message.clone()));
        self
    }

    /// Sets the message emitted when a pointer releases.
    pub fn on_release(mut self, message: Message) -> Self
    where
        Message: Clone + 'a,
    {
        self.on_release = Some(Box::new(move || message.clone()));
        self
    }

    /// Sets the message emitted on a leftward swipe.
    pub fn on_swipe_left(mut self, message: Message) -> Self {
        self.on_swipe_left = Some(message);
        self
    }

    /// Sets the message emitted on a rightward swipe.
    pub fn on_swipe_right(mut self, message: Message) -> Self {
        self.on_swipe_right = Some(message);
        self
    }

    /// Sets the message emitted on an upward swipe.
    pub fn on_swipe_up(mut self, message: Message) -> Self {
        self.on_swipe_up = Some(message);
        self
    }

    /// Sets the message emitted on a downward swipe.
    pub fn on_swipe_down(mut self, message: Message) -> Self {
        self.on_swipe_down = Some(message);
        self
    }

    /// Sets a callback for any directional swipe.
    pub fn on_swipe(
        mut self,
        on_swipe: impl Fn(SwipeDirection) -> Message + 'a,
    ) -> Self {
        self.on_swipe = Some(Box::new(on_swipe));
        self
    }

    /// Sets the callback for pan start.
    pub fn on_pan_start(
        mut self,
        on_pan_start: impl Fn(PanStartDetails) -> Message + 'a,
    ) -> Self {
        self.on_pan_start = Some(Box::new(on_pan_start));
        self
    }

    /// Sets the callback for continuous pan updates.
    pub fn on_pan_update(
        mut self,
        on_pan_update: impl Fn(PanUpdateDetails) -> Message + 'a,
    ) -> Self {
        self.on_pan_update = Some(Box::new(on_pan_update));
        self
    }

    /// Sets the callback for pan release/end.
    pub fn on_pan_end(
        mut self,
        on_pan_end: impl Fn(PanEndDetails) -> Message + 'a,
    ) -> Self {
        self.on_pan_end = Some(Box::new(on_pan_end));
        self
    }

    /// Sets the message emitted when a pan is cancelled.
    pub fn on_pan_cancel(mut self, message: Message) -> Self {
        self.on_pan_cancel = Some(message);
        self
    }

    /// Sets the touch slop threshold in pixels (default 10.0 px).
    pub fn touch_slop(mut self, slop: f32) -> Self {
        self.touch_slop = slop.max(0.0);
        self
    }

    /// Sets the swipe displacement threshold in pixels (default 40.0 px).
    pub fn swipe_threshold(mut self, threshold: f32) -> Self {
        self.swipe_threshold = threshold.max(0.0);
        self
    }

    /// Sets whether gesture drag intercepts child events and consumes them.
    pub fn intercept_events(mut self, intercept: bool) -> Self {
        self.intercept_events = intercept;
        self
    }
}

/// Convenience builder function for [`GestureDetector`].
pub fn gesture_detector<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> GestureDetector<'a, Message, Theme, Renderer> {
    GestureDetector::new(content)
}

/// Internal state of the gesture detector.
#[derive(Debug, Default)]
struct State {
    pointer_down: bool,
    start_pos: Point,
    last_pos: Point,
    start_time: Option<Instant>,
    last_time: Option<Instant>,
    is_dragging: bool,
    has_dragged: bool,
    last_tap_time: Option<Instant>,
    last_tap_pos: Option<Point>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for GestureDetector<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            limits,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let mut just_started_dragging = false;

        let touch_pos = match event {
            Event::Touch(touch::Event::FingerPressed { position, .. })
            | Event::Touch(touch::Event::FingerMoved { position, .. })
            | Event::Touch(touch::Event::FingerLifted { position, .. }) => Some(*position),
            _ => cursor.position(),
        };

        // Process pointer events for gesture detection
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    let now = Instant::now();
                    state.pointer_down = true;
                    state.start_pos = pos;
                    state.last_pos = pos;
                    state.start_time = Some(now);
                    state.last_time = Some(now);
                    state.is_dragging = false;
                    state.has_dragged = false;

                    if let Some(on_press) = &self.on_press {
                        shell.publish(on_press(pos));
                    }
                }
            }
            Event::Touch(touch::Event::FingerPressed { position, .. }) => {
                if bounds.contains(*position) {
                    let now = Instant::now();
                    state.pointer_down = true;
                    state.start_pos = *position;
                    state.last_pos = *position;
                    state.start_time = Some(now);
                    state.last_time = Some(now);
                    state.is_dragging = false;
                    state.has_dragged = false;

                    if let Some(on_press) = &self.on_press {
                        shell.publish(on_press(*position));
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.pointer_down {
                    let pos = *position;
                    let delta_x = pos.x - state.start_pos.x;
                    let delta_y = pos.y - state.start_pos.y;
                    let dist = (delta_x * delta_x + delta_y * delta_y).sqrt();

                    let now = Instant::now();
                    let frame_delta = Vector::new(
                        pos.x - state.last_pos.x,
                        pos.y - state.last_pos.y,
                    );

                    if !state.is_dragging && dist >= self.touch_slop {
                        state.is_dragging = true;
                        state.has_dragged = true;
                        just_started_dragging = true;
                        if let Some(on_pan_start) = &self.on_pan_start {
                            shell.publish(on_pan_start(PanStartDetails {
                                point: state.start_pos,
                            }));
                        }
                    }

                    if state.is_dragging {
                        if let Some(on_pan_update) = &self.on_pan_update {
                            shell.publish(on_pan_update(PanUpdateDetails {
                                point: pos,
                                delta: frame_delta,
                                total_delta: Vector::new(delta_x, delta_y),
                            }));
                        }
                    }

                    state.last_pos = pos;
                    state.last_time = Some(now);
                }
            }
            Event::Touch(touch::Event::FingerMoved { position, .. }) => {
                if state.pointer_down {
                    let pos = *position;
                    let delta_x = pos.x - state.start_pos.x;
                    let delta_y = pos.y - state.start_pos.y;
                    let dist = (delta_x * delta_x + delta_y * delta_y).sqrt();

                    let now = Instant::now();
                    let frame_delta = Vector::new(
                        pos.x - state.last_pos.x,
                        pos.y - state.last_pos.y,
                    );

                    if !state.is_dragging && dist >= self.touch_slop {
                        state.is_dragging = true;
                        state.has_dragged = true;
                        just_started_dragging = true;
                        if let Some(on_pan_start) = &self.on_pan_start {
                            shell.publish(on_pan_start(PanStartDetails {
                                point: state.start_pos,
                            }));
                        }
                    }

                    if state.is_dragging {
                        if let Some(on_pan_update) = &self.on_pan_update {
                            shell.publish(on_pan_update(PanUpdateDetails {
                                point: pos,
                                delta: frame_delta,
                                total_delta: Vector::new(delta_x, delta_y),
                            }));
                        }
                    }

                    state.last_pos = pos;
                    state.last_time = Some(now);
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                if state.pointer_down {
                    state.pointer_down = false;
                    let now = Instant::now();
                    let pos = touch_pos.unwrap_or(state.last_pos);
                    let total_delta = Vector::new(
                        pos.x - state.start_pos.x,
                        pos.y - state.start_pos.y,
                    );
                    let duration = now
                        .duration_since(state.start_time.unwrap_or(now))
                        .as_secs_f32()
                        .max(0.001);
                    let velocity = total_delta / duration;

                    if state.is_dragging {
                        state.is_dragging = false;
                        if let Some(on_pan_end) = &self.on_pan_end {
                            shell.publish(on_pan_end(PanEndDetails {
                                velocity,
                                total_delta,
                            }));
                        }

                        // Determine swipe
                        let is_swipe = total_delta.x.abs() >= self.swipe_threshold
                            || total_delta.y.abs() >= self.swipe_threshold
                            || velocity.x.abs() >= 250.0
                            || velocity.y.abs() >= 250.0;

                        if is_swipe {
                            if total_delta.x.abs() >= total_delta.y.abs() {
                                if total_delta.x < 0.0 {
                                    if let Some(msg) = &self.on_swipe_left {
                                        shell.publish(msg.clone());
                                    }
                                    if let Some(cb) = &self.on_swipe {
                                        shell.publish(cb(SwipeDirection::Left));
                                    }
                                } else {
                                    if let Some(msg) = &self.on_swipe_right {
                                        shell.publish(msg.clone());
                                    }
                                    if let Some(cb) = &self.on_swipe {
                                        shell.publish(cb(SwipeDirection::Right));
                                    }
                                }
                            } else {
                                if total_delta.y < 0.0 {
                                    if let Some(msg) = &self.on_swipe_up {
                                        shell.publish(msg.clone());
                                    }
                                    if let Some(cb) = &self.on_swipe {
                                        shell.publish(cb(SwipeDirection::Up));
                                    }
                                } else {
                                    if let Some(msg) = &self.on_swipe_down {
                                        shell.publish(msg.clone());
                                    }
                                    if let Some(cb) = &self.on_swipe {
                                        shell.publish(cb(SwipeDirection::Down));
                                    }
                                }
                            }
                        }
                    } else if bounds.contains(pos) {
                        // Pointer released within slop -> Tap!
                        let is_double_tap = if let (Some(last_t), Some(last_p)) =
                            (state.last_tap_time, state.last_tap_pos)
                        {
                            let tap_dist = ((pos.x - last_p.x).powi(2)
                                + (pos.y - last_p.y).powi(2))
                            .sqrt();
                            now.duration_since(last_t) <= self.double_tap_timeout
                                && tap_dist <= self.touch_slop * 2.0
                        } else {
                            false
                        };

                        if is_double_tap {
                            if let Some(on_double_tap) = &self.on_double_tap {
                                shell.publish(on_double_tap());
                            }
                            state.last_tap_time = None;
                            state.last_tap_pos = None;
                        } else {
                            if let Some(on_tap) = &self.on_tap {
                                shell.publish(on_tap());
                            }
                            if let Some(on_tap_at) = &self.on_tap_at {
                                shell.publish(on_tap_at(pos));
                            }
                            state.last_tap_time = Some(now);
                            state.last_tap_pos = Some(pos);
                        }
                    }

                    if let Some(on_release) = &self.on_release {
                        shell.publish(on_release());
                    }
                }
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                if state.pointer_down {
                    state.pointer_down = false;
                    state.has_dragged = false;
                    if state.is_dragging {
                        state.is_dragging = false;
                        if let Some(on_pan_cancel) = &self.on_pan_cancel {
                            shell.publish(on_pan_cancel.clone());
                        }
                    }
                }
            }
            _ => {}
        }

        let is_release = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(..))
                | Event::Touch(touch::Event::FingerLifted { .. })
        );

        if just_started_dragging {
            if self.intercept_events {
                let cancel = Event::Touch(touch::Event::FingerLost {
                    id: touch::Finger(0),
                    position: touch_pos.unwrap_or(state.last_pos),
                });
                self.content.as_widget_mut().update(
                    &mut tree.children[0],
                    &cancel,
                    layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                );
                shell.capture_event();
            }
        } else if state.is_dragging {
            if self.intercept_events {
                shell.capture_event();
            }
        } else if is_release {
            if state.has_dragged {
                if self.intercept_events {
                    let cancel = Event::Touch(touch::Event::FingerLost {
                        id: touch::Finger(0),
                        position: touch_pos.unwrap_or(state.last_pos),
                    });
                    self.content.as_widget_mut().update(
                        &mut tree.children[0],
                        &cancel,
                        layout,
                        cursor,
                        renderer,
                        clipboard,
                        shell,
                        viewport,
                    );
                    shell.capture_event();
                }
                state.has_dragged = false;
            } else {
                self.content.as_widget_mut().update(
                    &mut tree.children[0],
                    event,
                    layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                );
            }
        } else {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer>
    From<GestureDetector<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(
        detector: GestureDetector<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(detector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text;

    #[test]
    fn gesture_detector_builder_and_defaults() {
        let content: Element<'_, (), iced::Theme> = text("test").into();
        let detector = gesture_detector(content)
            .on_tap(())
            .on_double_tap(())
            .on_swipe_left(())
            .on_swipe_right(())
            .on_swipe_up(())
            .on_swipe_down(())
            .touch_slop(15.0)
            .swipe_threshold(50.0);

        assert_eq!(detector.touch_slop, 15.0);
        assert_eq!(detector.swipe_threshold, 50.0);
    }

    #[test]
    fn swipe_directions() {
        assert_eq!(SwipeDirection::Left, SwipeDirection::Left);
        assert_ne!(SwipeDirection::Left, SwipeDirection::Right);
    }
}

