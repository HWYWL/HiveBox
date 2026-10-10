//! Two screens on one panel, one behind the other, with the one in front on its way.
//!
//! What this exists for is the desktop's app layer: an app is drawn *over* the launcher, and the
//! moment it arrives or leaves is a moment the interface has to show rather than cut. iced has no
//! widget for that — `Stack` draws two children in one frame and says nothing about where either of
//! them is — so this is a widget that moves one of its two children, and asks for the frames to do
//! it in.
//!
//! # The pair, and which of them moves
//!
//! [`screen_transition`] takes a *background* and a *foreground*: the screen that is behind, and the
//! screen that is in front. On this board that pair is always the same pair — the launcher's grid
//! behind, the app that is coming or going in front — which is why the two are named for where they
//! *are* rather than for where they are going. A [`Motion`] decides that, and the same pair serves
//! all of them.
//!
//! ```text
//! 0.0                                 1.0
//! the app where it always was         the transition is over
//! ```
//!
//! At `0.0` the app is exactly where it has always been drawn, so a transition that is never seen
//! is indistinguishable from no transition at all. At `1.0` the motion has taken it wherever that
//! motion says: off the right for a back, off the top for the swipe home, and on the panel for an
//! open — which is done when the app has finished arriving rather than when it has gone.
//!
//! # Who advances it
//!
//! Itself. One step per `window::Event::RedrawRequested`, which the platform hands every widget on
//! every frame (`iced-pomelo-winit`'s `Tree::draw`), and a `request_redraw` while there is another
//! step to take. There is nothing for an app to subscribe to and nothing for it to call: a
//! transition is over when the widget publishes [`ScreenTransition::on_settled`], and until then it
//! is drawing its own frames.
//!
//! That is the same arrangement the pager uses for its settle, and for the same reason: a screen
//! that moves is a screen that needs frames, and the frame budget belongs to the thing that knows
//! how long it needs them.
//!
//! # What it does not do
//!
//! Cross-fade. Neither screen is drawn through the other, because this stack has no alpha for a
//! whole layer — only for a primitive — and a fade of two text-heavy screens per frame is not
//! something this panel's bus could carry anyway. What it does instead is what a phone does with a
//! *slide*: the screen in front travels, the screen behind stands still, and the panel's own edges
//! are the only clip either of them needs.

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::Operation;
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{window, Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::animation::{AnimationController, Curve};

/// Where a screen goes when a transition is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// The app leaves to the right: the swipe in from the left edge, and the back key that means
    /// the same thing. The desktop is what was behind it all along.
    Back,
    /// The app leaves upwards: the swipe up from the foot of the panel, which puts the app aside
    /// rather than closing it and shows the desktop it was covering.
    Home,
    /// The app arrives from the right, over the desktop it was opened from.
    Open,
    /// The screen in front comes down from above the panel: the task switcher, and the same gesture
    /// in reverse when a finger takes it away again.
    ///
    /// The one motion whose *end* is a layer being on screen rather than off it, which is what
    /// [`Motion::leaves`] answers no for. A drag of it works exactly like the others: the sheet is
    /// where the finger has put it, and where it ends up is decided when the finger leaves.
    Down,
}

impl Motion {
    /// Where the screen in front is, at `progress`, on a panel of `size`.
    ///
    /// `progress` is `0.0` at the start of the transition and `1.0` at the end of it, and this
    /// arithmetic is the whole of the motion: the curve that gets from one to the other belongs to
    /// the animator, not to this function.
    pub fn offset(self, progress: f32, size: Size) -> Vector {
        let progress = progress.clamp(0.0, 1.0);

        match self {
            // From where it was, off the right edge — one panel's width of travel.
            Self::Back => Vector::new(size.width * progress, 0.0),
            // Up and off the top, which is the direction the finger went.
            Self::Home => Vector::new(0.0, -size.height * progress),
            // In from the right: at 0.0 it is off the panel, at 1.0 it is where an app belongs.
            Self::Open => Vector::new(size.width * (1.0 - progress), 0.0),
            // Down from above: at 0.0 it is wholly over the top edge, at 1.0 it is where the sheet
            // belongs. `Open` turned a quarter turn, and the reason a sheet slides rather than fades.
            Self::Down => Vector::new(0.0, -size.height * (1.0 - progress)),
        }
    }

    /// Whether the screen in front has left the panel by the end of this motion.
    ///
    /// The one thing an app on the other side of a transition needs to know: an app that arrived is
    /// the screen, and an app that has gone leaves the desktop behind. [`Motion::Down`] is the other
    /// answer for the other reason — the sheet is on the panel when it has arrived, so there is
    /// nothing for the launcher to take away when it gets there.
    pub fn leaves(self) -> bool {
        matches!(self, Self::Back | Self::Home)
    }
}

/// Where the screen in front is, who is in charge of getting it there, and whether it is moving.
///
/// Two ways to hold a transition, and they are the two things a screen can be doing: following a
/// finger, or going somewhere on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    /// Held exactly here: no animation, because the clock is the finger. This *is* the position, as
    /// often as the caller wants to say it — a drag says it once a frame, and a still finger says the
    /// same number again.
    At(f32),
    /// On its way to here, under its own frames. Setting the same target again is not a new journey:
    /// a caller redrawing its view every frame hands this the same `To` every frame, and a transition
    /// that restarted on each of them would be one that never left.
    To(f32),
}

/// Two screens, one behind the other, with the one in front on its way somewhere.
pub struct ScreenTransition<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    background: Element<'a, Message, Theme, Renderer>,
    foreground: Element<'a, Message, Theme, Renderer>,
    motion: Motion,
    progress: Progress,
    width: Length,
    height: Length,
    duration: Duration,
    curve: Curve,
    on_settled: Option<Message>,
}

impl<'a, Message, Theme, Renderer> ScreenTransition<'a, Message, Theme, Renderer> {
    /// A transition between `background` (behind) and `foreground` (in front).
    ///
    /// A quarter of a second of `EaseOutCubic` by default, running to the end of the motion: the
    /// shape a page turn has, because it is the same gesture family and the same panel. See
    /// [`ScreenTransition::duration`] and [`ScreenTransition::progress`].
    pub fn new(
        background: Element<'a, Message, Theme, Renderer>,
        foreground: Element<'a, Message, Theme, Renderer>,
    ) -> Self {
        Self {
            background,
            foreground,
            motion: Motion::Back,
            progress: Progress::To(1.0),
            width: Length::Fill,
            height: Length::Fill,
            duration: Duration::from_millis(240),
            curve: Curve::EaseOutCubic,
            on_settled: None,
        }
    }

    /// Sets which way the screen in front is going.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Sets where the screen in front is, and who is taking it there.
    ///
    /// A transition that is driven — [`Progress::At`] — is one a gesture owns: the caller says where
    /// the screen is, as often as it has something new to say, and nothing here moves on its own. It
    /// is how a finger takes an app off the panel and puts it back, and it is the only way the finger
    /// and the animation cannot disagree about where the app is.
    pub fn progress(mut self, progress: Progress) -> Self {
        self.progress = progress;
        self
    }

    /// Sets the width of the transition.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Sets the height of the transition.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Sets how long the transition takes, and so how many frames it asks for.
    ///
    /// The whole panel is redrawn while a screen moves — the screen that is leaving is drawn again
    /// at every position, and the one behind it is drawn again underneath — so this number is a
    /// *budget* as much as a duration: at the 30 ms a full-screen write costs on this board,
    /// 240 ms is about eight frames of motion, which is as many as the panel can carry.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Sets the easing curve of the transition.
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Sets the message published once, when the transition is over.
    ///
    /// The message is the *end* of the transition rather than a step in it, and it arrives on the
    /// frame the screen has arrived: whoever owns the two screens swaps them here, and the swap
    /// cannot be seen because the screen in front is already exactly where the next one will be
    /// drawn.
    pub fn on_settled(mut self, message: Message) -> Self {
        self.on_settled = Some(message);
        self
    }
}

/// Convenience builder function for [`ScreenTransition`].
pub fn screen_transition<'a, Message, Theme, Renderer>(
    background: Element<'a, Message, Theme, Renderer>,
    foreground: Element<'a, Message, Theme, Renderer>,
) -> ScreenTransition<'a, Message, Theme, Renderer> {
    ScreenTransition::new(background, foreground)
}

/// Internal state of a [`ScreenTransition`].
#[derive(Debug, Default)]
struct State {
    /// Where the screen in front is, in `0.0..=1.0`.
    position: AnimationController,
    /// The place it is on its way to, if it is on its way anywhere: the last [`Progress::To`] this
    /// widget was handed. Kept so that being handed the same one again is not taken for a new
    /// journey — a caller redraws every frame, and would hand over the same target every frame.
    /// `None` while a finger holds it.
    aim: Option<f32>,
    /// Whether `on_settled` has been published for the journey in hand, so that it happens once.
    settled: bool,
}

/// One frame of a transition: hold it where it has been told to be or move it towards where it has
/// been sent, and report the frame it stops moving.
///
/// A free function rather than a method, because this is the part a test can drive by hand: the
/// instants it is handed *are* the animation, and a test that cannot choose them is testing the
/// machine's clock rather than the transition. Everything the step needs is passed in — nothing here
/// reads a widget, an element or a screen.
fn advance<Message: Clone>(
    progress: Progress,
    duration: Duration,
    curve: Curve,
    on_settled: Option<&Message>,
    state: &mut State,
    now: Instant,
    shell: &mut Shell<'_, Message>,
) {
    match progress {
        // A finger is holding it: the clock is the finger, and this is where it has put the screen.
        // Anything that was on its way stops *where it is*, because the hand that caught it decides
        // where it goes next and not the target it was sent to.
        Progress::At(position) => {
            if state.aim.take().is_some() {
                state.position.stop();
            }

            state.settled = false;
            state.position.set_value(position.clamp(0.0, 1.0));
        }
        // Or it is on its way, and only a *new* destination is a new journey.
        Progress::To(target) => {
            if state.aim != Some(target) {
                state.aim = Some(target);
                state.settled = false;
                state
                    .position
                    .animate_to(state.position.value(), target, duration, curve, now);
            }

            if state.position.is_animating() && state.position.update(now) {
                shell.request_redraw();
            }
        }
    }

    // The end of a journey, and the only message this widget ever publishes. Not while a finger holds
    // it — a held transition has not gone anywhere, it is being held — and not before it arrives, so
    // that the frame the screens are swapped on is one where the screen in front is already exactly
    // where the next one will be drawn.
    if matches!(progress, Progress::To(_)) && !state.position.is_animating() && !state.settled {
        state.settled = true;

        if let Some(message) = on_settled {
            shell.publish(message.clone());
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ScreenTransition<'_, Message, Theme, Renderer>
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
        vec![Tree::new(&self.background), Tree::new(&self.foreground)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.background, &self.foreground]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(self.height);
        let size = limits.resolve(self.width, self.height, Size::ZERO);
        let child_limits = layout::Limits::new(Size::ZERO, size);

        // Both screens fill the panel, and both are laid out *where they end up*: the offsets a
        // motion applies are a drawing matter, not a layout one. A screen laid out at its own place
        // and drawn somewhere else is what lets the two share one panel without either of them
        // knowing the other is there.
        let children = [&mut self.background, &mut self.foreground]
            .into_iter()
            .zip(&mut tree.children)
            .map(|(screen, child_tree)| {
                screen
                    .as_widget_mut()
                    .layout(child_tree, renderer, &child_limits)
                    .move_to(Point::ORIGIN)
            })
            .collect();

        layout::Node::with_children(size, children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        // Both of them, because an operation is about a screen rather than about the transition:
        // the settings app asks its own scrollable to jump to the top when its page changes, and
        // that request arrives whichever side of this pair the app is on.
        for (screen, (child_tree, child_layout)) in [&mut self.background, &mut self.foreground]
            .into_iter()
            .zip(tree.children.iter_mut().zip(layout.children()))
        {
            screen
                .as_widget_mut()
                .operate(child_tree, child_layout, renderer, operation);
        }
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
        let is_frame = matches!(event, Event::Window(window::Event::RedrawRequested(_)));

        let settled = {
            let state = tree.state.downcast_mut::<State>();

            // Everything about where the screen is happens on the frame, not on the messages that say
            // where it should be: a finger's next position and a gesture's decision both arrive as
            // messages, and iced runs the frame that follows them through this arm. One clock, and it
            // is the panel's.
            if is_frame {
                advance(
                    self.progress,
                    self.duration,
                    self.curve,
                    self.on_settled.as_ref(),
                    state,
                    Instant::now(),
                    shell,
                );
            }

            state.settled
        };

        // Which of the two screens a press belongs to. While the transition runs, neither: a screen
        // on its way out is not a screen a finger may press, and one that is arriving has not
        // arrived. Once it is over, the one that is still here — the app for an open, the desktop
        // for a back or a home, because by then the app has left the panel.
        let pressable = if !settled {
            None
        } else if self.motion.leaves() {
            Some(0)
        } else {
            Some(1)
        };

        for (index, ((screen, child_tree), child_layout)) in
            [&mut self.background, &mut self.foreground]
                .into_iter()
                .zip(tree.children.iter_mut())
                .zip(layout.children())
                .enumerate()
        {
            // Every frame reaches both screens, whatever else is dropped: a widget inside either of
            // them that keeps per-frame state — a settling pager, a blinking caret — is not frozen
            // by a transition it has nothing to do with.
            if !is_frame && pressable != Some(index) {
                continue;
            }

            screen.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
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
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let Some(visible_bounds) = bounds.intersection(viewport) else {
            return;
        };

        let offset = self.motion.offset(state.position.value(), bounds.size());
        let mut children = layout.children();

        // The screen behind, where it belongs: it is not moving, and it is not asked to.
        if let (Some(child_layout), Some(child_tree)) = (children.next(), tree.children.first()) {
            self.background.as_widget().draw(
                child_tree,
                renderer,
                theme,
                renderer_style,
                child_layout,
                cursor,
                viewport,
            );
        }

        // And the one in front, moved and clipped to the panel's own rectangle: a screen slides *out
        // of* the panel rather than into whatever is beside it, which is what the layer is for. It
        // is drawn with no cursor at all — nothing about a screen on its way is pressable — and with
        // the viewport shifted back into its own coordinates, so that its own culling is asked about
        // the part of it that is still on screen rather than the part it has left.
        let Some(child_layout) = children.next() else {
            return;
        };

        renderer.with_layer(visible_bounds, |renderer| {
            renderer.with_translation(offset, |renderer| {
                self.foreground.as_widget().draw(
                    &tree.children[1],
                    renderer,
                    theme,
                    renderer_style,
                    child_layout,
                    mouse::Cursor::Unavailable,
                    &Rectangle {
                        x: visible_bounds.x - offset.x,
                        y: visible_bounds.y - offset.y,
                        ..visible_bounds
                    },
                );
            });
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();

        // Nothing in a transition is under the finger's control while it moves, and a pointer that
        // changed shape mid-slide would be claiming otherwise.
        if !state.settled {
            return mouse::Interaction::None;
        }

        let Some(child_layout) = layout.children().nth(1) else {
            return mouse::Interaction::None;
        };

        self.foreground.as_widget().mouse_interaction(
            &tree.children[1],
            child_layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // The app's, when it has one: a sheet belongs to the screen that opened it, and the screen
        // that opens sheets here is the one in front.
        self.foreground.as_widget_mut().overlay(
            &mut tree.children[1],
            layout.children().nth(1)?,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<ScreenTransition<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(transition: ScreenTransition<'a, Message, Theme, Renderer>) -> Self {
        Self::new(transition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> Size {
        Size::new(480.0, 480.0)
    }

    /// A motion takes the screen in front from where it was to where that motion says, and nowhere
    /// else.
    #[test]
    fn a_motion_moves_the_screen_in_front_from_nothing_to_somewhere() {
        // A back leaves to the right: nothing at all at 0.0, one panel's width at 1.0.
        assert_eq!(Motion::Back.offset(0.0, panel()), Vector::new(0.0, 0.0));
        assert_eq!(Motion::Back.offset(1.0, panel()), Vector::new(480.0, 0.0));
        assert_eq!(Motion::Back.offset(0.5, panel()), Vector::new(240.0, 0.0));

        // Home leaves upwards, by the panel's height rather than its width.
        assert_eq!(Motion::Home.offset(0.0, panel()), Vector::new(0.0, 0.0));
        assert_eq!(Motion::Home.offset(1.0, panel()), Vector::new(0.0, -480.0));

        // An open arrives *from* the right, so the end of it is the origin — the app in the place
        // an app belongs, which is what makes the swap that follows invisible.
        assert_eq!(Motion::Open.offset(0.0, panel()), Vector::new(480.0, 0.0));
        assert_eq!(Motion::Open.offset(1.0, panel()), Vector::new(0.0, 0.0));

        // And a sheet comes down from above: wholly over the top edge at 0.0, in its place at 1.0 —
        // the same shape as an open, a quarter turn away from it.
        assert_eq!(Motion::Down.offset(0.0, panel()), Vector::new(0.0, -480.0));
        assert_eq!(Motion::Down.offset(0.25, panel()), Vector::new(0.0, -360.0));
        assert_eq!(Motion::Down.offset(1.0, panel()), Vector::new(0.0, 0.0));
    }

    /// A progress no finger could produce is clamped rather than extrapolated: a screen drawn a
    /// panel's width past the edge is not a thing that exists.
    #[test]
    fn a_progress_past_the_end_is_the_end() {
        assert_eq!(Motion::Back.offset(2.0, panel()), Vector::new(480.0, 0.0));
        assert_eq!(Motion::Back.offset(-1.0, panel()), Vector::new(0.0, 0.0));
        assert_eq!(Motion::Open.offset(-1.0, panel()), Vector::new(480.0, 0.0));
        assert_eq!(Motion::Down.offset(2.0, panel()), Vector::new(0.0, 0.0));
    }

    /// Which motions end with the screen in front off the panel — the question whoever owns the two
    /// screens asks when a transition reports itself over.
    ///
    /// The two arrivals are the interesting ones, and for opposite reasons: an app that has arrived
    /// *is* the screen, and a sheet that has arrived is a layer over it. Neither leaves the launcher
    /// anything to take away.
    #[test]
    fn only_the_departures_leave_the_panel() {
        assert!(Motion::Back.leaves());
        assert!(Motion::Home.leaves());
        assert!(!Motion::Open.leaves());
        assert!(!Motion::Down.leaves());
    }

    /// A transition publishes its settle once, on the frame it arrives, and asks for frames until
    /// then — the two halves of the contract an app relies on.
    #[test]
    fn a_transition_settles_once_and_only_at_the_end() {
        let mut state = State::default();
        let start = Instant::now();
        let (duration, curve) = (Duration::from_millis(100), Curve::Linear);
        let run = Progress::To(1.0);
        let settled: &str = "settled";
        let mut messages = Vec::new();

        // The first frame starts it, and it is not over.
        let mut shell = Shell::new(&mut messages);
        advance(run, duration, curve, Some(&settled), &mut state, start, &mut shell);

        assert!(!state.settled);
        assert!(messages.is_empty(), "nothing to report on the first frame");
        assert!(state.position.is_animating());

        // Halfway through: still nothing, and still running.
        let mut shell = Shell::new(&mut messages);
        advance(
            run,
            duration,
            curve,
            Some(&settled),
            &mut state,
            start + Duration::from_millis(50),
            &mut shell,
        );

        assert!(messages.is_empty());
        assert!(state.position.is_animating());
        assert_eq!(
            state.position.value(),
            0.5,
            "halfway is halfway on a linear curve"
        );

        // And the end: one message, and it is the last one there will ever be.
        let mut shell = Shell::new(&mut messages);
        advance(
            run,
            duration,
            curve,
            Some(&settled),
            &mut state,
            start + Duration::from_millis(100),
            &mut shell,
        );

        assert_eq!(messages, vec!["settled"]);
        assert_eq!(state.position.value(), 1.0);
        assert!(!state.position.is_animating());

        let mut shell = Shell::new(&mut messages);
        advance(
            run,
            duration,
            curve,
            Some(&settled),
            &mut state,
            start + Duration::from_millis(500),
            &mut shell,
        );

        assert_eq!(messages, vec!["settled"], "published once, not once a frame");
    }

    /// While a finger holds it the transition is the finger's: the position is the number the caller
    /// hands over, no frames are asked for, and nothing is ever reported over.
    #[test]
    fn a_held_transition_goes_where_the_finger_puts_it() {
        let mut state = State::default();
        let held: &str = "settled";
        let mut messages = Vec::new();

        // A drag, frame by frame: 0.1, 0.25, 0.25 again — a finger that has stopped is still a finger
        // — and 0.4.
        for (at, expected) in [(0.1, 0.1), (0.25, 0.25), (0.25, 0.25), (0.4, 0.4)] {
            let mut shell = Shell::new(&mut messages);
            advance(
                Progress::At(at),
                Duration::from_millis(100),
                Curve::Linear,
                Some(&held),
                &mut state,
                Instant::now(),
                &mut shell,
            );

            assert_eq!(state.position.value(), expected);
            assert!(!state.position.is_animating(), "the finger is the clock");
        }

        assert!(
            messages.is_empty(),
            "a transition being held has not gone anywhere, so there is nothing to report"
        );
    }

    /// A finger that catches a transition on its way takes it back: the animation stops where it was,
    /// the finger decides from there, and what happens next is the finger's business rather than the
    /// target nobody is heading for any more.
    #[test]
    fn a_finger_catches_a_transition_on_its_way() {
        let mut state = State::default();
        let start = Instant::now();
        let (duration, curve) = (Duration::from_millis(100), Curve::Linear);
        let mut messages = Vec::new();

        // On its way to the end, and a third of the way there.
        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::To(1.0),
            duration,
            curve,
            None::<&()>,
            &mut state,
            start,
            &mut shell,
        );

        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::To(1.0),
            duration,
            curve,
            None::<&()>,
            &mut state,
            start + Duration::from_millis(33),
            &mut shell,
        );

        assert!(state.position.is_animating());
        assert!((state.position.value() - 0.33).abs() < 0.01);

        // Caught, and put where the finger is.
        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::At(0.15),
            duration,
            curve,
            None::<&()>,
            &mut state,
            start + Duration::from_millis(40),
            &mut shell,
        );

        assert_eq!(state.position.value(), 0.15);
        assert!(!state.position.is_animating());

        // And let go of from there: it goes back to nothing.
        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::To(0.0),
            duration,
            curve,
            None::<&()>,
            &mut state,
            start + Duration::from_millis(50),
            &mut shell,
        );

        assert!(state.position.is_animating());

        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::To(0.0),
            duration,
            curve,
            None::<&()>,
            &mut state,
            start + Duration::from_millis(150),
            &mut shell,
        );

        assert_eq!(state.position.value(), 0.0);
        assert!(!state.position.is_animating());
    }

    /// The same target handed over again is the same journey and not a new one: a caller that redraws
    /// its view every frame says `To(1.0)` every frame, and a transition that restarted on each of
    /// them would stand still for a quarter of a second and then jump.
    #[test]
    fn the_same_target_every_frame_is_one_journey() {
        let mut state = State::default();
        let start = Instant::now();
        let (duration, curve) = (Duration::from_millis(100), Curve::Linear);
        let mut messages = Vec::new();

        for at in [0, 20, 50] {
            let mut shell = Shell::new(&mut messages);
            advance(
                Progress::To(1.0),
                duration,
                curve,
                None::<&()>,
                &mut state,
                start + Duration::from_millis(at),
                &mut shell,
            );
        }

        assert_eq!(
            state.position.value(),
            0.5,
            "measured from the first frame, not from the last one that said the same thing"
        );

        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::To(1.0),
            duration,
            curve,
            None::<&()>,
            &mut state,
            start + Duration::from_millis(100),
            &mut shell,
        );

        assert_eq!(state.position.value(), 1.0);
    }

    /// A transition with no time to take, or with nowhere to go, is over on its first frame and says
    /// so — and "nowhere to go" is a real case: a gesture that never moved the screen hands over a
    /// target the screen is already at.
    #[test]
    fn a_transition_with_nowhere_to_go_settles_on_its_first_frame() {
        let settled: &str = "settled";

        // No time to take.
        let mut state = State::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        advance(
            Progress::To(1.0),
            Duration::ZERO,
            Curve::EaseOutCubic,
            Some(&settled),
            &mut state,
            Instant::now(),
            &mut shell,
        );

        assert_eq!(messages, vec!["settled"]);
        assert_eq!(state.position.value(), 1.0);

        // And no distance to cover: sent back to where it already is.
        let mut state = State::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        advance(
            Progress::To(0.0),
            Duration::from_millis(240),
            Curve::EaseOutCubic,
            Some(&settled),
            &mut state,
            Instant::now(),
            &mut shell,
        );

        assert_eq!(messages, vec!["settled"]);
        assert_eq!(state.position.value(), 0.0);
    }

    /// A transition nobody is watching for still moves: with no `on_settled` there is no message, but
    /// the frames are asked for exactly the same.
    #[test]
    fn a_transition_with_nothing_to_report_still_animates() {
        let mut state = State::default();
        let start = Instant::now();
        let mut messages = Vec::new();

        let mut shell = Shell::new(&mut messages);
        advance(
            Progress::To(1.0),
            Duration::from_millis(100),
            Curve::Linear,
            None::<&()>,
            &mut state,
            start,
            &mut shell,
        );

        assert!(state.position.is_animating());
        assert!(messages.is_empty());
    }
}
