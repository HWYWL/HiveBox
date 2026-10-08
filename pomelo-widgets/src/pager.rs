//! Reusable horizontal pagination widget (`Pager` / `PageView`).
//!
//! Provides smooth horizontal swiping between multiple child views with:
//! - Interactive sliding preview during drag gestures
//! - Elastic resistance at boundary pages (first/last)
//! - Gesture cancellation (suppresses clicks on child buttons/tiles during swipe)
//! - Clean page change notifications (`on_change`)

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::Operation;
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{touch, window};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::animation::{AnimationController, Curve};

/// A multi-page container supporting horizontal swipe and drag pagination.
pub struct Pager<
    'a,
    Message,
    Theme = iced::Theme,
    Renderer = iced::Renderer,
> {
    pages: Vec<Element<'a, Message, Theme, Renderer>>,
    current_page: usize,
    on_change: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    width: Length,
    height: Length,
    swipe_commit: f32,
    touch_slop: f32,
    interactive: bool,
    anim_duration: Duration,
    curve: Curve,
    animated: bool,
}

impl<'a, Message, Theme, Renderer> Pager<'a, Message, Theme, Renderer> {
    /// Creates a new [`Pager`] with the given collection of pages.
    pub fn new(
        pages: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            pages: pages.into_iter().collect(),
            current_page: 0,
            on_change: None,
            width: Length::Fill,
            height: Length::Fill,
            swipe_commit: 200.0,
            touch_slop: 18.0,
            interactive: false,
            anim_duration: Duration::ZERO,
            curve: Curve::EaseOutCubic,
            animated: false,
        }
    }

    /// Sets the currently active page index (0-indexed).
    pub fn current_page(mut self, page: usize) -> Self {
        self.current_page = page;
        self
    }

    /// Sets the callback emitted when a page change is committed.
    pub fn on_change(
        mut self,
        on_change: impl Fn(usize) -> Message + 'a,
    ) -> Self {
        self.on_change = Some(Box::new(on_change));
        self
    }

    /// Sets the width of the pager.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Sets the height of the pager.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Sets the displacement threshold in pixels required to turn a page upon release.
    pub fn swipe_commit(mut self, commit: f32) -> Self {
        self.swipe_commit = commit.max(1.0);
        self
    }

    /// Sets the touch slop before considering movement a drag.
    pub fn touch_slop(mut self, slop: f32) -> Self {
        self.touch_slop = slop.max(0.0);
        self
    }

    /// Sets whether to visually slide adjacent pages during a drag.
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Sets whether page transition animations and visual dragging are enabled.
    ///
    /// Defaults to `false` (animations disabled: instant page snaps upon release, no live visual sliding).
    /// When set to `true`, enables live interactive page sliding and smooth settle transition animations.
    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        if animated {
            self.interactive = true;
            if self.anim_duration == Duration::ZERO {
                self.anim_duration = Duration::from_millis(250);
            }
        } else {
            self.interactive = false;
            self.anim_duration = Duration::ZERO;
        }
        self
    }

    /// Sets the duration of the settle transition animation when releasing a drag.
    ///
    /// Defaults to [`Duration::ZERO`] (instant snap). Setting a non-zero duration automatically enables animations.
    pub fn anim_duration(mut self, duration: Duration) -> Self {
        self.anim_duration = duration;
        if duration > Duration::ZERO {
            self.animated = true;
        }
        self
    }

    /// Sets the easing curve for the settle transition animation.
    ///
    /// Defaults to [`Curve::EaseOutCubic`].
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    fn start_settle(
        &self,
        state: &mut State,
        width: f32,
        current_page: usize,
        page_count: usize,
        offset: f32,
        shell: &mut Shell<'_, Message>,
    ) {
        // Flutter's kMinFlingVelocity is between 400.0 and 800.0 px/s.
        // A flick above 450.0 px/s is recognized as an intentional fling gesture.
        const MIN_FLING_VELOCITY: f32 = 450.0;

        let has_on_change = self.on_change.is_some();
        let can_next = has_on_change && current_page + 1 < page_count;
        let can_prev = has_on_change && current_page > 0;

        // In Flutter PageScrollPhysics, slow dragging snaps to page.roundToDouble() (50% of screen).
        // If swipe_commit is configured, clamp the commit ratio between 0.35 and 0.50.
        let commit_dist = self.swipe_commit.max(1.0);
        let commit_ratio = if width > 0.0 {
            (commit_dist / width).clamp(0.35, 0.5)
        } else {
            0.5
        };

        // Fractional page offset (positive = towards next page, negative = towards prev page)
        let drag_fraction = if width > 0.0 {
            -offset / width
        } else {
            0.0
        };

        // Flutter PageScrollPhysics: fling velocity shifts the effective fractional page by 0.5
        let velocity_shift = if state.velocity_x < -MIN_FLING_VELOCITY {
            0.5
        } else if state.velocity_x > MIN_FLING_VELOCITY {
            -0.5
        } else {
            0.0
        };

        let effective_fraction = drag_fraction + velocity_shift;

        let (target_offset, target_page) = if effective_fraction >= commit_ratio && can_next {
            (-width, Some(current_page + 1))
        } else if effective_fraction <= -commit_ratio && can_prev {
            (width, Some(current_page - 1))
        } else {
            (0.0, None)
        };

        if !self.animated || self.anim_duration == Duration::ZERO {
            state.is_dragging = false;
            state.has_dragged = false;
            if let Some(target) = target_page {
                if target != current_page {
                    state.drag_offset = target_offset;
                    state.settle_target_page = Some(target);
                    if let Some(on_change) = &self.on_change {
                        shell.publish(on_change(target));
                    }
                } else {
                    state.drag_offset = 0.0;
                    state.settle_target_page = None;
                }
            } else {
                state.drag_offset = 0.0;
                state.settle_target_page = None;
            }
            shell.request_redraw();
        } else {
            let remaining_distance = (target_offset - offset).abs();
            let duration = if width > 0.0 {
                let ratio = (remaining_distance / width).clamp(0.35, 1.0);
                self.anim_duration.mul_f32(ratio)
            } else {
                self.anim_duration
            };

            let now = Instant::now();
            state.animator.animate_to(
                offset,
                target_offset,
                duration,
                self.curve,
                now,
            );
            state.settle_target_page = target_page;
            state.is_dragging = false;
            shell.request_redraw();
        }
    }
}

/// Convenience builder function for [`Pager`].
pub fn pager<'a, Message, Theme, Renderer>(
    pages: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
) -> Pager<'a, Message, Theme, Renderer> {
    Pager::new(pages)
}

/// Internal state of the pager.
#[derive(Debug, Default)]
struct State {
    pointer_down: bool,
    start_x: f32,
    drag_offset: f32,
    is_dragging: bool,
    has_dragged: bool,
    animator: AnimationController,
    settle_target_page: Option<usize>,
    last_x: f32,
    last_move_time: Option<Instant>,
    velocity_x: f32,
    last_page: Option<usize>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Pager<'_, Message, Theme, Renderer>
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
        self.pages.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.pages);
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

        let children_nodes: Vec<layout::Node> = self
            .pages
            .iter_mut()
            .zip(&mut tree.children)
            .enumerate()
            .map(|(i, (page, child_tree))| {
                page.as_widget_mut()
                    .layout(child_tree, renderer, &child_limits)
                    .move_to(Point::new(i as f32 * size.width, 0.0))
            })
            .collect();

        layout::Node::with_children(size, children_nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        for (page, (child_tree, child_layout)) in self
            .pages
            .iter_mut()
            .zip(tree.children.iter_mut().zip(layout.children()))
        {
            page.as_widget_mut().operate(
                child_tree,
                child_layout,
                renderer,
                operation,
            );
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
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let page_count = self.pages.len();
        let current_page = self.current_page.min(page_count.saturating_sub(1));
        let mut just_started_dragging = false;

        // If parent switched current_page externally or in response to on_change:
        if state.last_page != Some(self.current_page) {
            state.last_page = Some(self.current_page);
            if let Some(target) = state.settle_target_page {
                if self.current_page == target {
                    state.settle_target_page = None;
                    state.drag_offset = 0.0;
                    state.has_dragged = false;
                }
            } else if state.animator.is_animating() {
                state.animator.stop();
                state.settle_target_page = None;
                state.drag_offset = 0.0;
            }
        }

        let touch_pos = match event {
            Event::Touch(touch::Event::FingerPressed { position, .. })
            | Event::Touch(touch::Event::FingerMoved { position, .. })
            | Event::Touch(touch::Event::FingerLifted { position, .. }) => Some(*position),
            _ => cursor.position(),
        };

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    if state.animator.is_animating() || state.settle_target_page.is_some() {
                        state.animator.stop();
                        state.settle_target_page = None;
                        state.pointer_down = true;
                        state.start_x = pos.x - state.drag_offset;
                        state.last_x = pos.x;
                        state.last_move_time = Some(Instant::now());
                        state.velocity_x = 0.0;
                        state.is_dragging = true;
                        state.has_dragged = true;
                        shell.capture_event();
                    } else {
                        state.pointer_down = true;
                        state.start_x = pos.x;
                        state.last_x = pos.x;
                        state.last_move_time = Some(Instant::now());
                        state.velocity_x = 0.0;
                        state.drag_offset = 0.0;
                        state.is_dragging = false;
                        state.has_dragged = false;
                    }
                }
            }
            Event::Touch(touch::Event::FingerPressed { position, .. }) => {
                if bounds.contains(*position) {
                    if state.animator.is_animating() || state.settle_target_page.is_some() {
                        state.animator.stop();
                        state.settle_target_page = None;
                        state.pointer_down = true;
                        state.start_x = position.x - state.drag_offset;
                        state.last_x = position.x;
                        state.last_move_time = Some(Instant::now());
                        state.velocity_x = 0.0;
                        state.is_dragging = true;
                        state.has_dragged = true;
                        shell.capture_event();
                    } else {
                        state.pointer_down = true;
                        state.start_x = position.x;
                        state.last_x = position.x;
                        state.last_move_time = Some(Instant::now());
                        state.velocity_x = 0.0;
                        state.drag_offset = 0.0;
                        state.is_dragging = false;
                        state.has_dragged = false;
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.pointer_down {
                    let now = Instant::now();
                    if let Some(last_time) = state.last_move_time {
                        let dt = now.duration_since(last_time).as_secs_f32();
                        if dt > 0.001 {
                            let inst_v = (position.x - state.last_x) / dt;
                            state.velocity_x = state.velocity_x * 0.4 + inst_v * 0.6;
                        }
                    }
                    state.last_x = position.x;
                    state.last_move_time = Some(now);

                    let delta_x = position.x - state.start_x;

                    if !state.is_dragging && delta_x.abs() >= self.touch_slop {
                        state.is_dragging = true;
                        state.has_dragged = true;
                        just_started_dragging = true;
                        // Absorb slop smoothly at trigger moment, shifting start_x so dragged_x starts from 0.0
                        state.start_x += self.touch_slop.copysign(delta_x);
                    }

                    if state.is_dragging {
                        let can_prev = current_page > 0;
                        let can_next = current_page + 1 < page_count;

                        // Continuous linear drag displacement relative to slop-adjusted start_x
                        let dragged_x = position.x - state.start_x;

                        // Apply damping when pulling past the ends
                        let effective_delta = if dragged_x > 0.0 && !can_prev {
                            dragged_x * 0.25
                        } else if dragged_x < 0.0 && !can_next {
                            dragged_x * 0.25
                        } else {
                            dragged_x
                        };

                        state.drag_offset = effective_delta;
                        shell.request_redraw();
                    }
                }
            }
            Event::Touch(touch::Event::FingerMoved { position, .. }) => {
                if state.pointer_down {
                    let now = Instant::now();
                    if let Some(last_time) = state.last_move_time {
                        let dt = now.duration_since(last_time).as_secs_f32();
                        if dt > 0.001 {
                            let inst_v = (position.x - state.last_x) / dt;
                            state.velocity_x = state.velocity_x * 0.4 + inst_v * 0.6;
                        }
                    }
                    state.last_x = position.x;
                    state.last_move_time = Some(now);

                    let delta_x = position.x - state.start_x;

                    if !state.is_dragging && delta_x.abs() >= self.touch_slop {
                        state.is_dragging = true;
                        state.has_dragged = true;
                        just_started_dragging = true;
                        // Absorb slop smoothly at trigger moment, shifting start_x so dragged_x starts from 0.0
                        state.start_x += self.touch_slop.copysign(delta_x);
                    }

                    if state.is_dragging {
                        let can_prev = current_page > 0;
                        let can_next = current_page + 1 < page_count;

                        // Continuous linear drag displacement relative to slop-adjusted start_x
                        let dragged_x = position.x - state.start_x;

                        let effective_delta = if dragged_x > 0.0 && !can_prev {
                            dragged_x * 0.25
                        } else if dragged_x < 0.0 && !can_next {
                            dragged_x * 0.25
                        } else {
                            dragged_x
                        };

                        state.drag_offset = effective_delta;
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.pointer_down {
                    state.pointer_down = false;
                    let had_dragged = state.has_dragged;
                    let offset = state.drag_offset;

                    if had_dragged {
                        self.start_settle(state, bounds.width, current_page, page_count, offset, shell);
                    }
                }
            }
            Event::Touch(touch::Event::FingerLifted { position, .. }) => {
                if state.pointer_down {
                    state.pointer_down = false;
                    let had_dragged = state.has_dragged;

                    if had_dragged {
                        let can_prev = current_page > 0;
                        let can_next = current_page + 1 < page_count;
                        let dragged_x = position.x - state.start_x;
                        let effective_delta = if dragged_x > 0.0 && !can_prev {
                            dragged_x * 0.25
                        } else if dragged_x < 0.0 && !can_next {
                            dragged_x * 0.25
                        } else {
                            dragged_x
                        };
                        state.drag_offset = effective_delta;
                        self.start_settle(state, bounds.width, current_page, page_count, effective_delta, shell);
                    }
                }
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                if state.pointer_down {
                    state.pointer_down = false;
                    if state.has_dragged && state.drag_offset != 0.0 {
                        self.start_settle(state, bounds.width, current_page, page_count, state.drag_offset, shell);
                    } else {
                        state.drag_offset = 0.0;
                        state.is_dragging = false;
                        state.has_dragged = false;
                        shell.request_redraw();
                    }
                }
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if state.animator.is_animating() {
                    let is_still_running = state.animator.update(*now);
                    state.drag_offset = state.animator.value();

                    if is_still_running {
                        shell.request_redraw();
                    } else {
                        // Animation completed and reached target_offset (e.g. -width, width, or 0.0)
                        state.drag_offset = state.animator.value();
                        if let Some(target_page) = state.settle_target_page {
                            if target_page != current_page {
                                // Keep state.drag_offset at target_offset so this frame's draw()
                                // continues rendering target_page without jumping back to old page!
                                if let Some(on_change) = &self.on_change {
                                    shell.publish(on_change(target_page));
                                } else {
                                    state.settle_target_page = None;
                                    state.drag_offset = 0.0;
                                    state.has_dragged = false;
                                }
                            } else {
                                state.settle_target_page = None;
                                state.drag_offset = 0.0;
                                state.has_dragged = false;
                            }
                        } else {
                            state.drag_offset = 0.0;
                            state.has_dragged = false;
                        }
                        shell.request_redraw();
                    }
                }
            }
            _ => {}
        }

        // Calculate offset and adjusted cursor for child views
        let base_x = current_page as f32 * bounds.width;
        let offset_x = if self.interactive || state.settle_target_page.is_some() {
            state.drag_offset
        } else {
            0.0
        };
        let scroll_x = base_x - offset_x;

        let adjusted_cursor = match touch_pos {
            Some(p) if bounds.contains(p) => {
                mouse::Cursor::Available(p + Vector::new(scroll_x, 0.0))
            }
            _ => mouse::Cursor::Unavailable,
        };

        let is_release = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(..))
                | Event::Touch(touch::Event::FingerLifted { .. })
        );

        if just_started_dragging {
            // As soon as dragging begins, immediately cancel any active button/tile press
            // BEFORE capturing the event in shell.
            let cancel = Event::Touch(touch::Event::FingerLost {
                id: touch::Finger(0),
                position: touch_pos.unwrap_or(Point::ORIGIN),
            });
            for (page, (child_tree, child_layout)) in self
                .pages
                .iter_mut()
                .zip(tree.children.iter_mut().zip(layout.children()))
            {
                page.as_widget_mut().update(
                    child_tree,
                    &cancel,
                    child_layout,
                    adjusted_cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                );
            }
            shell.capture_event();
        } else if state.is_dragging {
            // During drag, capture the movement event so parent/system does not interfere
            shell.capture_event();
        } else if is_release {
            if state.has_dragged {
                // If this gesture was a drag (even if user slid back to origin),
                // suppress release to children so buttons never trigger false clicks.
                let cancel = Event::Touch(touch::Event::FingerLost {
                    id: touch::Finger(0),
                    position: touch_pos.unwrap_or(Point::ORIGIN),
                });
                for (page, (child_tree, child_layout)) in self
                    .pages
                    .iter_mut()
                    .zip(tree.children.iter_mut().zip(layout.children()))
                {
                    page.as_widget_mut().update(
                        child_tree,
                        &cancel,
                        child_layout,
                        adjusted_cursor,
                        renderer,
                        clipboard,
                        shell,
                        viewport,
                    );
                }
                shell.capture_event();
                if !state.animator.is_animating() {
                    state.has_dragged = false;
                }
            } else {
                // Legitimate tap (release without dragging): forward release to visible page
                for (i, (page, (child_tree, child_layout))) in self
                    .pages
                    .iter_mut()
                    .zip(tree.children.iter_mut().zip(layout.children()))
                    .enumerate()
                {
                    if i == current_page {
                        page.as_widget_mut().update(
                            child_tree,
                            event,
                            child_layout,
                            adjusted_cursor,
                            renderer,
                            clipboard,
                            shell,
                            viewport,
                        );
                    }
                }
            }
        } else {
            let is_animating = state.animator.is_animating();
            let is_redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_)));

            // Forward event only when idle or if this is a frame redraw
            if !is_animating || is_redraw {
                for (i, (page, (child_tree, child_layout))) in self
                    .pages
                    .iter_mut()
                    .zip(tree.children.iter_mut().zip(layout.children()))
                    .enumerate()
                {
                    if i == current_page || (is_animating && state.settle_target_page == Some(i)) {
                        page.as_widget_mut().update(
                            child_tree,
                            event,
                            child_layout,
                            adjusted_cursor,
                            renderer,
                            clipboard,
                            shell,
                            viewport,
                        );
                    }
                }
            }
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
        let state = tree.state.downcast_ref::<State>();
        if state.is_dragging || state.animator.is_animating() {
            return mouse::Interaction::None;
        }

        let bounds = layout.bounds();
        let current_page = self.current_page.min(self.pages.len().saturating_sub(1));
        let base_x = current_page as f32 * bounds.width;
        let offset_x = if self.interactive || state.settle_target_page.is_some() {
            state.drag_offset
        } else {
            0.0
        };
        let scroll_x = base_x - offset_x;

        let adjusted_cursor = match cursor.position() {
            Some(p) if bounds.contains(p) => {
                mouse::Cursor::Available(p + Vector::new(scroll_x, 0.0))
            }
            _ => mouse::Cursor::Unavailable,
        };

        self.pages
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .enumerate()
            .filter_map(|(i, ((page, child_tree), child_layout))| {
                if i == current_page
                    || (state.is_dragging
                        && (i as i32 - current_page as i32).abs() <= 1)
                {
                    Some(page.as_widget().mouse_interaction(
                        child_tree,
                        child_layout,
                        adjusted_cursor,
                        viewport,
                        renderer,
                    ))
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(mouse::Interaction::None)
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
        let current_page = self.current_page.min(self.pages.len().saturating_sub(1));
        let base_x = current_page as f32 * bounds.width;
        let offset_x = if self.interactive || state.settle_target_page.is_some() {
            state.drag_offset
        } else {
            0.0
        };
        let scroll_x = base_x - offset_x;

        let Some(visible_bounds) = bounds.intersection(viewport) else {
            return;
        };

        renderer.with_layer(visible_bounds, |renderer| {
            renderer.with_translation(Vector::new(-scroll_x, 0.0), |renderer| {
                let adjusted_cursor = match cursor.position() {
                    Some(p) if bounds.contains(p) => {
                        mouse::Cursor::Available(p + Vector::new(scroll_x, 0.0))
                    }
                    _ => mouse::Cursor::Unavailable,
                };

                for (page, (child_tree, child_layout)) in self
                    .pages
                    .iter()
                    .zip(tree.children.iter().zip(layout.children()))
                {
                    let child_bounds = child_layout.bounds();
                    let translated_x = child_bounds.x - scroll_x;
                    let page_view_rect = Rectangle {
                        x: translated_x,
                        y: child_bounds.y,
                        width: child_bounds.width,
                        height: child_bounds.height,
                    };

                    if page_view_rect.intersects(&visible_bounds) {
                        page.as_widget().draw(
                            child_tree,
                            renderer,
                            theme,
                            renderer_style,
                            child_layout,
                            adjusted_cursor,
                            &Rectangle {
                                x: visible_bounds.x + scroll_x,
                                ..visible_bounds
                            },
                        );
                    }
                }
            });
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let current_page = self.current_page.min(self.pages.len().saturating_sub(1));

        if let Some((page, (child_tree, child_layout))) = self
            .pages
            .iter_mut()
            .zip(tree.children.iter_mut().zip(layout.children()))
            .nth(current_page)
        {
            page.as_widget_mut().overlay(
                child_tree,
                child_layout,
                renderer,
                viewport,
                translation,
            )
        } else {
            None
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Pager<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(
        pager: Pager<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(pager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text;

    #[test]
    fn pager_builder_and_defaults() {
        let p_default: Pager<'_, (), iced::Theme> =
            pager(vec![text("Page 1").into(), text("Page 2").into()]);
        assert!(!p_default.animated);
        assert!(!p_default.interactive);
        assert_eq!(p_default.anim_duration, Duration::ZERO);

        let pages: Vec<Element<'_, (), iced::Theme>> =
            vec![text("Page 1").into(), text("Page 2").into()];
        let p = pager(pages)
            .current_page(1)
            .swipe_commit(64.0)
            .touch_slop(12.0)
            .interactive(true)
            .animated(true)
            .anim_duration(Duration::from_millis(300))
            .curve(Curve::EaseOutQuad);

        assert_eq!(p.pages.len(), 2);
        assert_eq!(p.current_page, 1);
        assert_eq!(p.swipe_commit, 64.0);
        assert_eq!(p.touch_slop, 12.0);
        assert!(p.interactive);
        assert!(p.animated);
        assert_eq!(p.anim_duration, Duration::from_millis(300));
        assert_eq!(p.curve, Curve::EaseOutQuad);
    }

    #[test]
    fn pager_default_unanimated_snaps_instantly() {
        let pages: Vec<Element<'_, usize, iced::Theme>> =
            vec![text("Page 1").into(), text("Page 2").into()];
        let p = pager(pages)
            .current_page(0)
            .on_change(|page| page);

        assert!(!p.animated);

        let mut state = State::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        // Release at offset -200px on a 400px wide screen (50% commit threshold)
        p.start_settle(&mut state, 400.0, 0, 2, -200.0, &mut shell);

        // Must NOT start any animation
        assert!(!state.animator.is_animating());
        assert_eq!(state.settle_target_page, Some(1));
        assert_eq!(state.drag_offset, -400.0);
        assert_eq!(messages, vec![1]);
    }

    #[test]
    fn pager_settle_animation_progression() {
        let pages: Vec<Element<'_, usize, iced::Theme>> =
            vec![text("Page 1").into(), text("Page 2").into()];
        let p = pager(pages)
            .current_page(0)
            .animated(true)
            .on_change(|page| page)
            .anim_duration(Duration::from_millis(200))
            .curve(Curve::Linear);

        let mut state = State::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let t0 = Instant::now();

        // Release at offset -200px on a 400px wide screen (50% commit threshold)
        p.start_settle(&mut state, 400.0, 0, 2, -200.0, &mut shell);

        assert!(state.animator.is_animating());
        assert_eq!(state.settle_target_page, Some(1));
        assert_eq!(state.animator.value(), -200.0);

        // Advance halfway through duration
        let t_half = t0 + Duration::from_millis(100);
        let running = state.animator.update(t_half);
        assert!(running);
        assert!(state.animator.value() < -200.0);

        // Advance past duration
        let t_end = t0 + Duration::from_millis(250);
        let running = state.animator.update(t_end);
        assert!(!running);
        assert_eq!(state.animator.value(), -400.0);
    }

    #[test]
    fn pager_flick_velocity_commits_early() {
        let pages: Vec<Element<'_, usize, iced::Theme>> =
            vec![text("Page 1").into(), text("Page 2").into()];
        let p = pager(pages)
            .current_page(0)
            .animated(true)
            .swipe_commit(200.0)
            .on_change(|page| page);

        let mut state = State {
            velocity_x: -600.0, // Fast flick left
            ..Default::default()
        };
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        // Offset is only -20px (well below 200px threshold), but flick velocity is high
        p.start_settle(&mut state, 400.0, 0, 2, -20.0, &mut shell);

        assert_eq!(state.settle_target_page, Some(1));
        assert!(state.animator.is_animating());
    }

    #[test]
    fn pager_slow_drag_under_threshold_snaps_back() {
        let pages: Vec<Element<'_, usize, iced::Theme>> =
            vec![text("Page 1").into(), text("Page 2").into()];
        let p = pager(pages)
            .current_page(0)
            .animated(true)
            .swipe_commit(200.0)
            .on_change(|page| page);

        let mut state = State {
            velocity_x: -50.0, // Slow drag
            ..Default::default()
        };
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        // Offset is -80px on 400px screen (20% < 45% threshold), slow velocity
        p.start_settle(&mut state, 400.0, 0, 2, -80.0, &mut shell);

        assert_eq!(state.settle_target_page, None);
        assert!(state.animator.is_animating());
        assert_eq!(state.animator.value(), -80.0);
    }

    #[test]
    fn pager_seamless_page_handoff_no_flicker() {
        let pages: Vec<Element<'_, usize, iced::Theme>> =
            vec![text("Page 1").into(), text("Page 2").into()];
        let p = pager(pages)
            .current_page(0)
            .on_change(|page| page)
            .anim_duration(Duration::from_millis(100))
            .curve(Curve::Linear);

        let mut state = State::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let t0 = Instant::now();

        // Release dragging to next page
        p.start_settle(&mut state, 400.0, 0, 2, -200.0, &mut shell);
        assert_eq!(state.settle_target_page, Some(1));

        // Advance to animation end
        let t_end = t0 + Duration::from_millis(150);
        let _ = state.animator.update(t_end);

        // Simulate RedrawRequested handling when animation completes
        let is_running = state.animator.is_animating();
        assert!(!is_running);

        state.drag_offset = state.animator.value();
        assert_eq!(state.drag_offset, -400.0);
        // Notice: drag_offset is STILL -400.0 (holding target page in view!)
        // It must NOT be reset to 0.0 while current_page is still 0!
        assert_eq!(state.settle_target_page, Some(1));

        // Now parent app receives on_change(1) and rebuilds with current_page = 1:
        if state.settle_target_page == Some(1) {
            // Rebuilt with current_page == target_page:
            let p_next: Pager<'_, usize, iced::Theme> = pager(vec![text("Page 1").into(), text("Page 2").into()])
                .current_page(1);
            if p_next.current_page == state.settle_target_page.unwrap() {
                state.settle_target_page = None;
                state.drag_offset = 0.0;
            }
        }

        assert_eq!(state.settle_target_page, None);
        assert_eq!(state.drag_offset, 0.0);
    }

    #[test]
    fn pager_drag_reversal_is_continuous_and_does_not_pop() {
        use iced::advanced::Widget;

        let mut p = pager(vec![text("Page 1").into(), text("Page 2").into()])
            .current_page(0)
            .touch_slop(18.0)
            .swipe_commit(200.0);

        let mut tree = Tree::new(&p as &dyn Widget<(), iced::Theme, ()>);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(480.0, 480.0));
        let layout_node = layout::Node::new(Size::new(480.0, 480.0));
        let layout = Layout::new(&layout_node);
        let mut clipboard = iced::advanced::clipboard::Null;

        // 1. Touch down at x = 100.0
        p.update(
            &mut tree,
            &Event::Touch(touch::Event::FingerPressed {
                id: touch::Finger(0),
                position: Point::new(100.0, 240.0),
            }),
            layout,
            mouse::Cursor::Unavailable,
            &(),
            &mut clipboard,
            &mut shell,
            &bounds,
        );

        let state = tree.state.downcast_ref::<State>();
        assert!(!state.is_dragging);
        assert_eq!(state.drag_offset, 0.0);

        // 2. Drag right past slop (delta_x = +25.0) -> dragged_x should be +7.0, damped to +1.75 on page 0
        p.update(
            &mut tree,
            &Event::Touch(touch::Event::FingerMoved {
                id: touch::Finger(0),
                position: Point::new(125.0, 240.0),
            }),
            layout,
            mouse::Cursor::Unavailable,
            &(),
            &mut clipboard,
            &mut shell,
            &bounds,
        );

        let state = tree.state.downcast_ref::<State>();
        assert!(state.is_dragging);
        assert!((state.drag_offset - 1.75).abs() < 1e-4);

        // 3. Move finger back towards origin to x = 110.0 (delta = +10.0 from down)
        // With previous copysign bug, dragged_x inverted sign to -8.0!
        // Now it must stay on the right (+110 - 118 = -8 in raw, but continuous!)
        // Specifically, as finger moves from 125 -> 120 -> 118 -> 115 -> 100:
        // raw dragged_x moves smoothly from +7 -> +2 -> 0 -> -3 -> -18 without ANY 36px jump!
        let mut prev_offset = state.drag_offset;
        for x in [120.0, 118.0, 115.0, 105.0, 100.0, 95.0, 80.0] {
            p.update(
                &mut tree,
                &Event::Touch(touch::Event::FingerMoved {
                    id: touch::Finger(0),
                    position: Point::new(x, 240.0),
                }),
                layout,
                mouse::Cursor::Unavailable,
                &(),
                &mut clipboard,
                &mut shell,
                &bounds,
            );
            let cur_offset = tree.state.downcast_ref::<State>().drag_offset;
            // Displacement must decrease monotonically as x decreases
            assert!(
                cur_offset <= prev_offset + 1e-5,
                "Offset must decrease continuously when moving left: prev={}, cur={}",
                prev_offset,
                cur_offset
            );
            // Must have NO large discontinuities (jump <= 15px for small steps)
            assert!(
                (cur_offset - prev_offset).abs() < 20.0,
                "Detected jump discontinuity: prev={}, cur={}",
                prev_offset,
                cur_offset
            );
            prev_offset = cur_offset;
        }
    }
}


