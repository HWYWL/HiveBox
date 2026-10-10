//! Generic gesture recognition widget for touch and mouse interactions.
//!
//! Provides a flexible, Flutter-inspired gesture recognizer supporting:
//! - Taps and double taps with slop filtering
//! - Continuous pan/drag gestures with delta and velocity tracking
//! - Directional swipe gestures (Left, Right, Up, Down)
//! - Edge-origin swipes: a direction may be required to start at a named edge of the detector
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

/// The edge of the detector a gesture has to start at.
///
/// A phone's back gesture begins at the side of the screen and its "home" gesture at the foot, and
/// which edge a drag started on is the whole of what tells that gesture apart from the same drag
/// made in the middle of a page — where the drag belongs to whatever is scrolling. See
/// [`GestureDetector::swipe_origin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    /// The foot of the panel, where the "home" gesture starts.
    Bottom,
    /// The head of it, which is where a phone puts its notification shade — and where this board puts
    /// the task switcher, for the same reason: it is the one edge nothing else reaches for.
    ///
    /// A downward drag is the direction most likely to *be* something else — a list scrolling under
    /// the finger — which is why the caller has to pin it to this edge before the detector will
    /// claim it at all. See [`GestureDetector::swipe_origin`].
    Top,
}

impl Edge {
    /// Whether `point` lies within `depth` pixels of this edge of `bounds`.
    pub fn contains(self, point: Point, bounds: Rectangle, depth: f32) -> bool {
        let depth = depth.max(0.0);

        match self {
            Self::Left => point.x <= bounds.x + depth,
            Self::Right => point.x >= bounds.x + bounds.width - depth,
            Self::Bottom => point.y >= bounds.y + bounds.height - depth,
            Self::Top => point.y <= bounds.y + depth,
        }
    }
}

impl SwipeDirection {
    /// The slot this direction's origin is kept in.
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Up => 2,
            Self::Down => 3,
        }
    }
}

/// Which way a drag of `delta` points: whichever axis it travelled furthest along.
fn swipe_direction(delta: Vector) -> SwipeDirection {
    if delta.x.abs() >= delta.y.abs() {
        if delta.x < 0.0 {
            SwipeDirection::Left
        } else {
            SwipeDirection::Right
        }
    } else if delta.y < 0.0 {
        SwipeDirection::Up
    } else {
        SwipeDirection::Down
    }
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
    /// Where a swipe in each direction has to *begin*, by [`SwipeDirection::index`].
    ///
    /// `None` — what every direction starts as — means anywhere in the detector's bounds, which is
    /// what a swipe meant before this existed. See [`GestureDetector::swipe_origin`].
    swipe_origins: [Option<(Edge, f32)>; 4],
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
            swipe_origins: [None; 4],
            double_tap_timeout: std::time::Duration::from_millis(300),
            intercept_events: true,
        }
    }

    /// Requires a swipe in `direction` to begin within `depth` pixels of `edge`.
    ///
    /// This is what lets one detector sit over a whole screen without being a layer that swallows
    /// every drag: a drag that starts anywhere else is left to the widgets underneath, which is what
    /// a page that scrolls is made of. The question is asked the moment a drag passes `touch_slop`
    /// — not when it ends — because by then the widget underneath has already had its drag taken
    /// away, and a scroll that was interrupted to no purpose is worse than no gesture layer at all.
    ///
    /// The edges are the *start*, not the direction: a phone's back gesture is a rightward drag that
    /// starts at the **left** edge, and a leftward one that starts at the right. Nothing here assumes
    /// the two are the same side.
    pub fn swipe_origin(mut self, direction: SwipeDirection, edge: Edge, depth: f32) -> Self {
        self.swipe_origins[direction.index()] = Some((edge, depth));
        self
    }

    /// Whether this detector has anything to say about a drag in `direction`.
    ///
    /// Two ways to have something to say, and they are the two things a caller can ask this widget
    /// for: a swipe *message* for that direction, or the continuous pan callbacks — for which the
    /// direction is only what the pin is asked about, because a pan is a drag with a place it started
    /// and not a direction.
    ///
    /// A pan callback with no pin is nobody's answer, and that is deliberate: "tell me about every
    /// drag on the panel" is a layer that eats scrolls, and a caller who wants a drag from anywhere
    /// can pin one direction to an edge of the whole panel rather than leave the pin off.
    fn handles(&self, direction: SwipeDirection) -> bool {
        let specific = match direction {
            SwipeDirection::Left => self.on_swipe_left.is_some(),
            SwipeDirection::Right => self.on_swipe_right.is_some(),
            SwipeDirection::Up => self.on_swipe_up.is_some(),
            SwipeDirection::Down => self.on_swipe_down.is_some(),
        };

        let pans = self.on_pan_start.is_some()
            || self.on_pan_update.is_some()
            || self.on_pan_end.is_some();

        specific
            || self.on_swipe.is_some()
            || (pans && self.swipe_origins[direction.index()].is_some())
    }

    /// Where a swipe in `direction` has to begin, as it was configured — `None` for anywhere.
    ///
    /// Exported for the tests of whoever wires a screen up. The pins are the whole of what separates
    /// a gesture layer from a layer that eats scrolls, and a wiring that lost one would still build,
    /// still look right, and still be wrong in the hand.
    pub fn origin(&self, direction: SwipeDirection) -> Option<(Edge, f32)> {
        self.swipe_origins[direction.index()]
    }

    /// Whether a drag in `direction` that started at `start` is this detector's to act on.
    ///
    /// Both halves matter. A direction with no handler is not this detector's, whoever's it is —
    /// taking it would mean cancelling a drag in order to do nothing with it. And a direction with
    /// an origin is only this detector's when the drag began there.
    fn claims(&self, direction: SwipeDirection, start: Point, bounds: Rectangle) -> bool {
        if !self.handles(direction) {
            return false;
        }

        match self.swipe_origins[direction.index()] {
            Some((edge, depth)) => edge.contains(start, bounds, depth),
            None => true,
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

    /// Sets the callback for pan start: the frame the drag passed the slop and became this
    /// detector's.
    ///
    /// The four pan callbacks are the detector's own drags and nothing else. What makes a drag this
    /// detector's is [`GestureDetector::swipe_origin`], and a pan callback with *no* pin claims
    /// nothing at all — so a caller who wants pans has to say where they begin. That is the whole
    /// difference between a gesture layer and a layer that eats scrolls: a page under it keeps
    /// scrolling wherever the pin does not reach.
    pub fn on_pan_start(
        mut self,
        on_pan_start: impl Fn(PanStartDetails) -> Message + 'a,
    ) -> Self {
        self.on_pan_start = Some(Box::new(on_pan_start));
        self
    }

    /// Sets the callback for continuous pan updates: one per frame the finger moves, with the
    /// distance since the last frame and the distance since the drag began.
    pub fn on_pan_update(
        mut self,
        on_pan_update: impl Fn(PanUpdateDetails) -> Message + 'a,
    ) -> Self {
        self.on_pan_update = Some(Box::new(on_pan_update));
        self
    }

    /// Sets the callback for pan release/end: where the drag stopped, and how fast.
    ///
    /// The velocity is the whole drag's — its displacement over its duration — which is what a flick
    /// is, and what a caller deciding between "this far" and "this fast" needs.
    pub fn on_pan_end(
        mut self,
        on_pan_end: impl Fn(PanEndDetails) -> Message + 'a,
    ) -> Self {
        self.on_pan_end = Some(Box::new(on_pan_end));
        self
    }

    /// Sets the message emitted when a pan is cancelled: the finger lost to the panel, or taken away
    /// from the drag.
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
    /// Whether this drag is the detector's to act on, decided the moment it became a drag.
    ///
    /// A drag that is not the detector's is left entirely to the widget underneath — its moves are
    /// forwarded and nothing is captured — which is what keeps a swipe filter from being a scroll
    /// filter. See [`GestureDetector::claims`].
    owns: bool,
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
                    state.owns = false;

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
                    state.owns = false;

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
                        // The drag becomes the detector's (or does not) here, where the origin and
                        // the direction are both known and the widget underneath has not yet been
                        // interrupted. See `claims`.
                        state.owns = self.claims(
                            swipe_direction(Vector::new(delta_x, delta_y)),
                            state.start_pos,
                            bounds,
                        );
                        // The pan callbacks are the detector's own drags and nothing else, for the
                        // same reason the swipe messages are: a pan this detector did not claim is a
                        // scroll of whatever is underneath, and a caller drawing a screen from it
                        // would be drawing one for a finger that is reading a list.
                        if state.owns {
                            if let Some(on_pan_start) = &self.on_pan_start {
                                shell.publish(on_pan_start(PanStartDetails {
                                    point: state.start_pos,
                                }));
                            }
                        }
                    }

                    if state.is_dragging && state.owns {
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
                        // The drag becomes the detector's (or does not) here, where the origin and
                        // the direction are both known and the widget underneath has not yet been
                        // interrupted. See `claims`.
                        state.owns = self.claims(
                            swipe_direction(Vector::new(delta_x, delta_y)),
                            state.start_pos,
                            bounds,
                        );
                        // The pan callbacks are the detector's own drags and nothing else, for the
                        // same reason the swipe messages are: a pan this detector did not claim is a
                        // scroll of whatever is underneath, and a caller drawing a screen from it
                        // would be drawing one for a finger that is reading a list.
                        if state.owns {
                            if let Some(on_pan_start) = &self.on_pan_start {
                                shell.publish(on_pan_start(PanStartDetails {
                                    point: state.start_pos,
                                }));
                            }
                        }
                    }

                    if state.is_dragging && state.owns {
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
                        // Asked with the answer from the moment the drag began and not with this
                        // moment's direction: a pan is one gesture from one place, and the release is
                        // its last frame rather than a new question about it.
                        if state.owns {
                            if let Some(on_pan_end) = &self.on_pan_end {
                                shell.publish(on_pan_end(PanEndDetails {
                                    velocity,
                                    total_delta,
                                }));
                            }
                        }

                        // The swipe, if this is one and if it is the detector's to report.
                        //
                        // Asked again here rather than trusted from the moment the drag began: a
                        // drag that begins downwards in a side strip can end up leftwards, and the
                        // direction it *ends* in is the message that would be sent. The origin is
                        // the same question as it was then, so a gesture that changed its mind
                        // about its direction gets nothing rather than something surprising.
                        let direction = swipe_direction(total_delta);
                        let is_swipe = total_delta.x.abs() >= self.swipe_threshold
                            || total_delta.y.abs() >= self.swipe_threshold
                            || velocity.x.abs() >= 250.0
                            || velocity.y.abs() >= 250.0;

                        if is_swipe && state.owns && self.claims(direction, state.start_pos, bounds)
                        {
                            let specific = match direction {
                                SwipeDirection::Left => &self.on_swipe_left,
                                SwipeDirection::Right => &self.on_swipe_right,
                                SwipeDirection::Up => &self.on_swipe_up,
                                SwipeDirection::Down => &self.on_swipe_down,
                            };

                            if let Some(message) = specific {
                                shell.publish(message.clone());
                            }

                            if let Some(on_swipe) = &self.on_swipe {
                                shell.publish(on_swipe(direction));
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
                        // Only a drag that was the detector's can be *cancelled* as one: one it never
                        // claimed was never announced, and a caller told that a gesture it never heard
                        // about has ended would be a caller told about somebody else's finger.
                        if state.owns {
                            if let Some(on_pan_cancel) = &self.on_pan_cancel {
                                shell.publish(on_pan_cancel.clone());
                            }
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

        let was_dragged = state.has_dragged;

        if is_release && was_dragged {
            state.has_dragged = false;
        }

        // Who the drag belongs to is what decides whether the widget underneath hears about it at
        // all, and the answer is the one `claims` gave when the drag began. A drag this detector
        // does not own is forwarded exactly as it always was — the filter is on the *gesture*, and
        // a page that scrolls has to keep scrolling wherever a corner of it does not claim.
        let consuming = state.owns && self.intercept_events;

        if consuming && (just_started_dragging || (is_release && was_dragged)) {
            // The finger is the detector's now, and the widget underneath is told so: a button
            // under it stops being pressed, a scrollable stops scrolling. Told once at each end of
            // the drag and not once a frame — the cancel walks the whole subtree.
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
        } else if consuming && state.is_dragging {
            shell.capture_event();
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

    /// An edge is a band along one side of the bounds, and nothing else.
    #[test]
    fn an_edge_is_a_band_along_one_side_of_the_bounds() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(480.0, 480.0));

        assert!(Edge::Left.contains(Point::new(0.0, 300.0), bounds, 24.0));
        assert!(Edge::Left.contains(Point::new(24.0, 300.0), bounds, 24.0));
        assert!(!Edge::Left.contains(Point::new(24.1, 300.0), bounds, 24.0));

        assert!(Edge::Right.contains(Point::new(456.0, 300.0), bounds, 24.0));
        assert!(!Edge::Right.contains(Point::new(455.9, 300.0), bounds, 24.0));

        // The bottom band is the foot of the panel and nothing above it, which is what keeps a
        // swipe up in the middle of a page from reading as a swipe up from its edge.
        assert!(Edge::Bottom.contains(Point::new(200.0, 470.0), bounds, 24.0));
        assert!(!Edge::Bottom.contains(Point::new(200.0, 450.0), bounds, 24.0));
    }

    /// The band is measured from the detector's own bounds, wherever they happen to be.
    #[test]
    fn an_edge_follows_the_bounds_it_is_measured_in() {
        let bounds = Rectangle::new(Point::new(100.0, 50.0), Size::new(200.0, 300.0));

        assert!(Edge::Left.contains(Point::new(110.0, 200.0), bounds, 24.0));
        assert!(!Edge::Left.contains(Point::new(130.0, 200.0), bounds, 24.0));

        assert!(Edge::Bottom.contains(Point::new(200.0, 340.0), bounds, 24.0));
        assert!(!Edge::Bottom.contains(Point::new(200.0, 320.0), bounds, 24.0));
    }

    /// A drag points along whichever axis it travelled furthest.
    #[test]
    fn a_drag_points_along_the_axis_it_travelled() {
        assert_eq!(swipe_direction(Vector::new(-60.0, 5.0)), SwipeDirection::Left);
        assert_eq!(swipe_direction(Vector::new(60.0, -5.0)), SwipeDirection::Right);
        assert_eq!(swipe_direction(Vector::new(5.0, -60.0)), SwipeDirection::Up);
        assert_eq!(swipe_direction(Vector::new(-5.0, 60.0)), SwipeDirection::Down);

        // Exactly diagonal is a tie, and the tie goes to the horizontal: a thumb crossing the panel
        // is a side gesture far more often than it is a vertical one.
        assert_eq!(
            swipe_direction(Vector::new(30.0, -30.0)),
            SwipeDirection::Right
        );
    }

    /// A swipe is claimed when the detector can answer it *and* the drag began where that direction
    /// says it has to.
    #[test]
    fn a_swipe_is_claimed_only_where_it_began() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(480.0, 480.0));
        let content: Element<'_, (), iced::Theme> = text("x").into();

        let detector = gesture_detector(content)
            .on_swipe_up(())
            .on_swipe_right(())
            .swipe_origin(SwipeDirection::Up, Edge::Bottom, 24.0)
            .swipe_origin(SwipeDirection::Right, Edge::Left, 24.0);

        // Claimed: up from the foot of the panel, right from its left side.
        assert!(detector.claims(SwipeDirection::Up, Point::new(200.0, 470.0), bounds));
        assert!(detector.claims(SwipeDirection::Right, Point::new(4.0, 200.0), bounds));

        // Not claimed: the same directions, started anywhere else. This is the half that keeps a
        // page scrolling — the drag is left to the widget underneath rather than taken and dropped.
        assert!(!detector.claims(SwipeDirection::Up, Point::new(200.0, 240.0), bounds));
        assert!(!detector.claims(SwipeDirection::Right, Point::new(240.0, 200.0), bounds));

        // Not claimed: directions with no handler at all, wherever they began. Intercepting a drag
        // in order to do nothing with it is the one thing this filter exists to prevent.
        assert!(!detector.claims(SwipeDirection::Down, Point::new(4.0, 470.0), bounds));
        assert!(!detector.claims(SwipeDirection::Left, Point::new(4.0, 200.0), bounds));
    }

    /// With no origin set, a direction is claimed anywhere — what a swipe meant before origins.
    #[test]
    fn a_swipe_with_no_origin_is_claimed_anywhere() {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(480.0, 480.0));
        let content: Element<'_, (), iced::Theme> = text("x").into();

        let detector = gesture_detector(content).on_swipe_left(());

        assert!(detector.claims(SwipeDirection::Left, Point::new(240.0, 240.0), bounds));
        assert!(detector.claims(SwipeDirection::Left, Point::new(2.0, 478.0), bounds));
    }
}

