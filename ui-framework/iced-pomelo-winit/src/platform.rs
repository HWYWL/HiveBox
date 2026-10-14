//! Embedded platform host and event loop for the Pomelo OS panel.
//!
//! Directly interfaces with the hardware display (DMA flush) and touch interrupt queue,
//! functioning as the embedded counterpart to `winit::event_loop` and `winit::window`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use iced_core::theme::Base;
use iced_core::time::Instant;
use iced_core::window;
use iced_core::{Point, Rectangle, Size};
use iced_graphics::compositor::Compositor as _;
use iced_graphics::{compositor, Shell, Viewport};
use iced_program::Program;

use crate::application::Tree;
use crate::conversion;
use crate::{Error, ProgramApp};

/// The panel width in physical pixels.
pub const PANEL_WIDTH: u32 = 480;

/// The panel height in physical pixels.
pub const PANEL_HEIGHT: u32 = 480;

/// Physical dimensions of the display panel.
pub const PANEL_SIZE: Size<u32> = Size::new(PANEL_WIDTH, PANEL_HEIGHT);

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalTouchEvType {
    None = 0,
    Down = 1,
    Move = 2,
    Up = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HalTouchEvent {
    pub ev_type: HalTouchEvType,
    pub x: i32,
    pub y: i32,
}

#[cfg(target_os = "espidf")]
extern "C" {
    fn hal_display_draw_bitmap(x1: i32, y1: i32, x2: i32, y2: i32, pixels: *const u16, stride: i32);
    fn hal_display_wait_vsync();
    fn hal_touch_get_point(out_x: *mut i32, out_y: *mut i32) -> bool;
    fn hal_touch_wait_event(out_ev: *mut HalTouchEvent, timeout_ms: u32) -> bool;
    fn vTaskDelay(ticks: u32);
}

#[cfg(not(target_os = "espidf"))]
#[allow(unused_variables)]
unsafe fn hal_display_draw_bitmap(x1: i32, y1: i32, x2: i32, y2: i32, pixels: *const u16, stride: i32) {}

#[cfg(not(target_os = "espidf"))]
unsafe fn hal_display_wait_vsync() {}

#[cfg(not(target_os = "espidf"))]
#[allow(unused_variables)]
unsafe fn hal_touch_get_point(out_x: *mut i32, out_y: *mut i32) -> bool {
    false
}

#[cfg(not(target_os = "espidf"))]
#[allow(unused_variables)]
unsafe fn hal_touch_wait_event(out_ev: *mut HalTouchEvent, timeout_ms: u32) -> bool {
    false
}

#[cfg(not(target_os = "espidf"))]
#[allow(unused_variables)]
unsafe fn vTaskDelay(ticks: u32) {}

/// Reads the current touch point from the atomic driver cache.
pub fn touch_point() -> Option<Point> {
    let (mut x, mut y) = (0i32, 0i32);
    if unsafe { hal_touch_get_point(&mut x, &mut y) } {
        let clamped_x = x.clamp(0, PANEL_WIDTH as i32 - 1);
        let clamped_y = y.clamp(0, PANEL_HEIGHT as i32 - 1);
        Some(Point::new(clamped_x as f32, clamped_y as f32))
    } else {
        None
    }
}

/// Waits for a touch event from the interrupt-driven FreeRTOS queue.
pub fn wait_touch_event(timeout_ms: u32) -> Option<iced_core::Event> {
    let mut ev = std::mem::MaybeUninit::uninit();
    if unsafe { hal_touch_wait_event(ev.as_mut_ptr(), timeout_ms) } {
        let ev = unsafe { ev.assume_init() };
        conversion::touch_event(ev)
    } else {
        None
    }
}

/// One damaged rectangle in the panel's own coordinates.
///
/// Already rounded out to the two-pixel boundary the CO5300's QSPI controller requires, and kept
/// integral, so the merge decision below weighs the pixel areas that will actually be transferred
/// rather than the float bounds they came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Region {
    /// The panel rectangle `rect` covers, or `None` if it covers nothing.
    ///
    /// The CO5300 only accepts even coordinates. Rounding outwards makes this a superset of the
    /// damaged rectangle, which is the safe direction: a pixel is repainted twice at worst.
    fn of(rect: Rectangle) -> Option<Self> {
        let x1 = (rect.x.floor() as i32).max(0) / 2 * 2;
        let y1 = (rect.y.floor() as i32).max(0) / 2 * 2;
        let x2 = (((rect.x + rect.width).ceil() as i32 + 1) / 2 * 2).min(PANEL_WIDTH as i32);
        let y2 = (((rect.y + rect.height).ceil() as i32 + 1) / 2 * 2).min(PANEL_HEIGHT as i32);

        (x2 > x1 && y2 > y1).then_some(Self { x1, y1, x2, y2 })
    }

    fn area(&self) -> i64 {
        i64::from(self.x2 - self.x1) * i64::from(self.y2 - self.y1)
    }

    fn union(&self, other: &Self) -> Self {
        Self {
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
            x2: self.x2.max(other.x2),
            y2: self.y2.max(other.y2),
        }
    }
}

/// How much extra, unchanged area one merge may cost, as a fraction of the area already being sent.
///
/// Two separate regions cost two address-window command blocks (`CASET`, `RASET`, `RAMWR`) and a
/// drained ping-pong pipeline between them; one merged region costs whatever unchanged pixels lie
/// in the gap. It is the gap that has to be bounded: a status-bar repaint and an animation elsewhere
/// on the screen are two cheap transfers apart and nearly a full frame together.
const MERGE_SLACK: f64 = 0.25;

/// Groups damaged rectangles into the fewest transfers that do not waste much.
///
/// Largest first, so a big region absorbs the ones around it rather than a small one growing a
/// bounding box across the screen. Each region joins the first group it nearly fits in; the rest go
/// out as they are. A drag across the launcher's grid is damaged as several touching tiles and those
/// still become one transfer — they have no gap to pay for — while damage in two opposite corners
/// now costs two small transfers instead of one covering everything between them.
fn plan(mut regions: Vec<Region>) -> Vec<Region> {
    regions.sort_by(|a, b| b.area().cmp(&a.area()));

    let mut planned: Vec<Region> = Vec::with_capacity(regions.len());

    for region in regions {
        let mut join = None;

        for (index, group) in planned.iter().enumerate() {
            let merged = group.union(&region);
            let gap = merged.area() - group.area() - region.area();
            let sent = group.area() + region.area();

            if gap as f64 <= sent as f64 * MERGE_SLACK {
                join = Some((index, merged));
                break;
            }
        }

        match join {
            Some((index, merged)) => planned[index] = merged,
            None => planned.push(region),
        }
    }

    planned
}

/// What one [`flush_damage`] handed to the panel.
///
/// Counted after [`plan`] has merged what it could, so this describes the transfers actually made
/// and not the rectangles the compositor reported as damaged. The two differ by every unchanged
/// pixel a merge absorbed, which is the gap [`MERGE_SLACK`] bounds — and on this hardware that gap
/// is worth counting, because a merge trades pixels for transfers and the caller should be able to
/// weigh what it bought. It is also what makes `[flush]`'s rate meaningful: a number reported
/// against the unmerged area would be a rate for pixels that were never sent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
    /// How many `hal_display_draw_bitmap` calls the damage became.
    pub regions: usize,
    /// The pixels those calls carried.
    pub pixels: u64,
}

/// Hands damaged rectangular regions of the frame buffer to the panel via DMA.
///
/// Each planned region is one `hal_display_draw_bitmap`, which the driver slices into vertical
/// strips and drains before returning. Almost every pixel of a planned region was going to be sent
/// anyway; the alternative this replaced — the bounding box of *every* region — sent the whole
/// screen to repaint a clock ticking in the status bar.
///
/// Returns what was sent, for the caller that has to time it.
pub fn flush_damage(damage: &[Rectangle], pixels: &[u16]) -> Sent {
    let regions: Vec<Region> = damage.iter().filter_map(|rect| Region::of(*rect)).collect();
    let planned = plan(regions);

    let sent = Sent {
        regions: planned.len(),
        pixels: planned.iter().map(|region| region.area()).sum::<i64>() as u64,
    };

    for region in planned {
        unsafe {
            hal_display_draw_bitmap(
                region.x1,
                region.y1,
                region.x2,
                region.y2,
                pixels.as_ptr(),
                PANEL_WIDTH as i32,
            );
        }
    }

    sent
}

/// Yields task execution to FreeRTOS.
pub fn delay_ticks(ticks: u32) {
    unsafe {
        vTaskDelay(ticks);
    }
}

/// The panel and the finger: the hardware the loop reads and writes.
///
/// This is the embedded counterpart of a window. `winit` *creates* its window, so `iced_winit`
/// never has to be told what to draw on; here the hardware exists before the software, so it is
/// handed over instead — [`Host::new`] takes one, and a test substitutes its own. It is the one
/// seam the two platforms cannot share, and keeping it explicit is what makes the loop testable
/// without a panel.
pub trait Board {
    /// The panel size, in physical pixels.
    fn size(&self) -> (u32, u32);

    /// Where the finger is now, if it is down. Polled once a step.
    fn touch(&self) -> Option<Point> {
        None
    }

    /// Waits for an input event, up to `timeout_ms`. `None` on timeout.
    ///
    /// The default times out at once, which is what a test wants: the loop then runs as fast as
    /// the caller steps it.
    fn wait_touch(&self, _timeout_ms: u32) -> Option<iced_core::Event> {
        None
    }

    /// Hands the damaged rectangles to the panel, with the frame in its pixels.
    fn flush(&mut self, damage: &[Rectangle], pixels: &[u16]);

    /// Nothing was drawn: the panel's chance to yield or sleep.
    ///
    /// The default does nothing. This is where the firmware's loop lets FreeRTOS run, and it is
    /// what an idle screen costs.
    fn idle(&mut self) {}
}

/// The real panel: the CO5300 over the C side's `hal_display_*`, and the touch controller behind
/// `hal_touch_*`. This is the [`Board`] the firmware gets, through [`crate::run`].
pub struct Panel;

impl Board for Panel {
    fn size(&self) -> (u32, u32) {
        (PANEL_WIDTH, PANEL_HEIGHT)
    }

    fn touch(&self) -> Option<Point> {
        touch_point()
    }

    fn wait_touch(&self, timeout_ms: u32) -> Option<iced_core::Event> {
        wait_touch_event(timeout_ms)
    }

    /// Waits for the v-blank, then hands the damage to the panel, timed as two numbers.
    ///
    /// The wait and the transfer are split because they are the two halves a fix could act on and
    /// they are not interchangeable: the wait is the renderer standing idle against a display that
    /// is already pacing it, while the transfer is work that has to happen — a PSRAM copy into the
    /// shadow frame, a byte swap, and the QSPI strips, none of which the renderer's own phase
    /// split can see. See `iced_pomelo_gfx::profile::panel_write`; the guard is inert unless the
    /// crate is built with `profile`, so this reads the same in both builds.
    fn flush(&mut self, damage: &[Rectangle], pixels: &[u16]) {
        let mut write = iced_pomelo_gfx::profile::panel_write();

        unsafe {
            hal_display_wait_vsync();
        }

        write.waited();

        let sent = flush_damage(damage, pixels);

        write.sent(sent.regions, sent.pixels);
    }

    fn idle(&mut self) {
        // One frame's worth of ticks, so a loop with nothing to do does not spin.
        delay_ticks(1);
    }
}

/// The application host and event loop: drives touch input, frame scheduling, and presentation.
pub struct Host<P>
where
    P: Program,
{
    program: ProgramApp<P>,
    tree: Tree,
    messages: Vec<P::Message>,
    dirty: bool,
    redraw_requested: bool,
    compositor: <P::Renderer as compositor::Default>::Compositor,
    renderer: P::Renderer,
    surface: <<P::Renderer as compositor::Default>::Compositor as compositor::Compositor>::Surface,
    viewport: Viewport,
    flushed: Rc<Cell<bool>>,
    /// The hardware. Shared with the compositor's present callback, which is installed as a
    /// global and outlives any borrow of `self`.
    board: Rc<RefCell<Box<dyn Board>>>,
    touching: bool,
    pending_size: Option<Size>,
}

impl<P> Host<P>
where
    P: Program + 'static,
{
    /// Takes `program` and the hardware it draws on and reads.
    ///
    /// The board is what `winit` would have created: [`crate::run`] hands over the panel's, and a
    /// test hands over its own.
    pub fn new(program: P, board: Box<dyn Board>) -> Result<Self, Error> {
        crate::fonts::install_default();

        let (width, height) = board.size();
        let board = Rc::new(RefCell::new(board));

        let flushed = Rc::new(Cell::new(false));
        {
            let flushed = Rc::clone(&flushed);
            let board = Rc::clone(&board);
            iced_pomelo_gfx::panel::set(move |pixels, damage| {
                flushed.set(true);
                board.borrow_mut().flush(damage, pixels);
            });
        }

        let settings = program.settings();
        let graphics: iced_graphics::Settings = settings.clone().into();

        let mut compositor = block_on(
            <<P::Renderer as compositor::Default>::Compositor as compositor::Compositor>::new(
                graphics,
                iced_pomelo_gfx::Panel,
                iced_pomelo_gfx::Panel,
                Shell::headless(),
            ),
        )
        .map_err(Error::GraphicsCreationFailed)?;

        for font in settings.fonts {
            compositor.load_font(font);
        }

        let renderer = compositor.create_renderer();
        let surface = compositor.create_surface(iced_pomelo_gfx::Panel, width, height);

        Ok(Self {
            program: ProgramApp::new(program),
            tree: Tree::new(),
            messages: Vec::new(),
            dirty: true,
            redraw_requested: false,
            compositor,
            renderer,
            surface,
            viewport: Viewport::with_physical_size(Size::new(width, height), 1.0),
            flushed,
            board,
            touching: false,
            pending_size: Some(Size::new(width as f32, height as f32)),
        })
    }

    pub fn app(&self) -> &ProgramApp<P> {
        &self.program
    }

    /// Queues an event, the way a window would deliver one. For tests, and for a backend whose
    /// inputs outnumber a touchscreen's.
    pub fn push_event(&mut self, event: iced_core::Event) {
        self.tree.push_event(event);
        self.dirty = true;
    }

    /// A finger touched the panel at `point`.
    pub fn touch_down(&mut self, point: Point) {
        self.tree.touch_down(point);
        self.dirty = true;
    }

    /// A finger moved while touching the panel.
    pub fn touch_move(&mut self, point: Point) {
        self.tree.touch_move(point);
        self.dirty = true;
    }

    /// The finger left the panel.
    pub fn touch_up(&mut self) {
        self.tree.touch_up();
        self.dirty = true;
    }

    pub fn update(&mut self, message: P::Message) {
        self.program.update(message);
        self.dirty = true;
    }

    pub fn step(&mut self) -> bool {
        let point = self.board.borrow().touch();

        match (point, self.touching) {
            (Some(point), false) => {
                self.touching = true;
                self.tree.touch_down(point);
            }
            (Some(point), true) => self.tree.touch_move(point),
            (None, true) => {
                self.touching = false;
                self.tree.touch_up();
            }
            (None, false) => {}
        }

        self.flushed.set(false);
        self.frame();
        let painted = self.flushed.get();

        if !painted {
            let event = self.board.borrow().wait_touch(16);

            if let Some(event) = event {
                self.program.broadcast_event(event);
                self.dirty = true;
            } else {
                // Nothing to draw and no input: the board's chance to sleep. This is what an
                // idle screen costs, and the only place the loop yields.
                self.board.borrow_mut().idle();
            }
        }

        painted
    }

    pub fn run(&mut self) -> Result<(), Error> {
        loop {
            self.step();
        }
    }

    fn frame(&mut self) {
        if let Some(size) = self.pending_size.take() {
            self.program.broadcast(window::Event::Resized(size));
        }

        self.program
            .broadcast(window::Event::RedrawRequested(Instant::now()));

        self.messages.extend(self.program.poll());

        // What a task asked the tree to do. A widget operation needs the `UserInterface`, which
        // `Tree::draw` builds, so it is handed to the tree rather than applied here; and a frame
        // with one pending is not one to skip, or the operation would wait for an input that may
        // never come.
        let operations = self.program.take_operations();
        self.tree.queue_operations(operations);

        if !self.dirty
            && !self.redraw_requested
            && self.tree.is_idle()
            && self.messages.is_empty()
            && !self.tree.has_operations()
        {
            return;
        }

        let theme = self.program.theme();
        let base = theme.base();
        let bounds = self.viewport.logical_size();

        self.redraw_requested = self.tree.draw(
            &mut self.renderer,
            self.program.view(),
            &mut self.messages,
            &theme,
            bounds,
            base.text_color,
        );

        let _ = self.compositor.present(
            &mut self.renderer,
            &mut self.surface,
            &self.viewport,
            base.background_color,
            || {},
        );

        self.dirty = !self.messages.is_empty();

        for message in self.messages.drain(..) {
            self.program.update(message);
        }

        for message in self.program.poll() {
            self.dirty = true;
            self.program.update(message);
        }
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());

    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rectangle at integer coordinates, phrased the way damage arrives.
    fn at(x: f32, y: f32, side: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(side, side))
    }

    fn planned(damage: &[Rectangle]) -> Vec<Region> {
        let regions = damage.iter().filter_map(|rect| Region::of(*rect)).collect();

        plan(regions)
    }

    #[test]
    fn a_region_rounds_out_to_the_two_pixel_grid() {
        // The controller takes even coordinates only, so an odd start comes back one pixel early and
        // the far edge one pixel late. Rounding out, never in: a superset repaints a pixel twice.
        let region = Region::of(at(3.0, 5.0, 10.0)).expect("a region");

        assert_eq!((region.x1, region.y1, region.x2, region.y2), (2, 4, 14, 16));
    }

    #[test]
    fn damage_that_covers_nothing_is_dropped() {
        // A zero-area rectangle, and one entirely off the left edge. Neither is a transfer, and
        // neither may become a `hal_display_draw_bitmap` call.
        assert!(Region::of(Rectangle::new(Point::new(10.0, 10.0), Size::default())).is_none());
        assert!(Region::of(at(-50.0, -50.0, 10.0)).is_none());
    }

    #[test]
    fn damage_in_two_corners_stays_two_transfers() {
        // The point of the whole change: a clock in the status bar and something else at the far
        // corner must not turn into one transfer spanning everything between them.
        let damage = [at(4.0, 4.0, 8.0), at(460.0, 460.0, 8.0)];

        assert_eq!(
            planned(&damage),
            vec![
                Region {
                    x1: 4,
                    y1: 4,
                    x2: 12,
                    y2: 12
                },
                Region {
                    x1: 460,
                    y1: 460,
                    x2: 468,
                    y2: 468
                },
            ]
        );
    }

    #[test]
    fn touching_tiles_become_one_transfer() {
        // A drag across the launcher's grid damages several tiles that share an edge. They have no
        // gap to pay for, so they still go out as a single transfer -- which is what the bounding
        // box this replaced was for, kept for the case that earned it.
        let damage = [at(0.0, 0.0, 20.0), at(20.0, 0.0, 20.0), at(40.0, 0.0, 20.0)];

        assert_eq!(
            planned(&damage),
            vec![Region {
                x1: 0,
                y1: 0,
                x2: 60,
                y2: 20
            }]
        );
    }

    #[test]
    fn a_merge_is_refused_when_the_gap_costs_more_than_the_transfer() {
        // One small tile and one at the far side: the merge would send a screen to move two tiles.
        assert_eq!(planned(&[at(0.0, 0.0, 16.0), at(464.0, 464.0, 16.0)]).len(), 2);

        // The same two tiles pulled together, with a gap under the slack: one transfer is cheaper
        // than two address-window blocks, so they merge.
        assert_eq!(planned(&[at(0.0, 0.0, 16.0), at(20.0, 0.0, 16.0)]).len(), 1);
    }

    #[test]
    fn overlapping_regions_never_accumulate() {
        // The same rectangle twice -- which a diff can produce -- is one transfer of that rectangle,
        // not two and not a larger one.
        let damage = [at(100.0, 100.0, 32.0), at(100.0, 100.0, 32.0)];

        assert_eq!(
            planned(&damage),
            vec![Region {
                x1: 100,
                y1: 100,
                x2: 132,
                y2: 132
            }]
        );
    }

    #[test]
    fn what_a_flush_sent_is_counted_after_the_merge() {
        // `[flush]`'s pixel count and its rate are the transfer's and not the damage's: two touching
        // tiles go out as one region carrying both their areas, two far corners as two regions
        // carrying only their own. Measured on the unmerged damage the rate would be a rate for
        // pixels that were never sent, which is the number that would make a merge look free.
        //
        // The draw itself is the stub off the device, so the pixels handed in are never read.
        let sent = flush_damage(&[at(0.0, 0.0, 20.0), at(20.0, 0.0, 20.0)], &[]);

        assert_eq!(sent.regions, 1, "touching tiles are one transfer");
        assert_eq!(sent.pixels, 40 * 20, "and it carries both their areas");

        let sent = flush_damage(&[at(0.0, 0.0, 16.0), at(464.0, 464.0, 16.0)], &[]);

        assert_eq!(sent.regions, 2, "two corners stay two transfers");
        assert_eq!(sent.pixels, 2 * 16 * 16, "and each carries only its own");
    }
}
