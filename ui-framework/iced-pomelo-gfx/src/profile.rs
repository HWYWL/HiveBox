//! What a frame spends, phase by phase.
//!
//! A screen change on the panel costs hundreds of milliseconds, and "the frame is slow" is not
//! actionable: this module is what turns it into "these are the milliseconds, in this order".
//! Every phase of a frame is timed with one `Instant` pair, the totals are collected in counters,
//! and the compositor prints them as one line when it has a frame to hand the panel.
//!
//! It is compiled only under the `profile` feature. Without it every function here is a no-op —
//! `start` hands back a guard that holds nothing and dropping it does nothing — so a call site is
//! the same plain statement in both builds and there is no `#[cfg]` in the middle of a render loop.
//! A build without the feature must not read the clock twice per glyph for numbers nobody reads.
//!
//! # What the split is for
//!
//! Measured on the launcher opening its settings screen (a whole-panel frame, x86 release): the
//! tree is ~12% of it, text ~60%, and the whole-screen `clear` ~7%. Of the text, most is
//! **rasterising a glyph that was not in the cache yet** — a screen pays that once (the same screen
//! a second time is 3× cheaper) and pays only the blits after that. Rasterising, the blit, the
//! `clear` and the panel write are all per-glyph or per-pixel costs, which is what scales with the
//! hardware; the raw "paint" number in the firmware's own log cannot say which of them it is.
//!
//! # The half the split cannot see
//!
//! A frame does not end where those phases do. The renderer hands its damage over and the host then
//! waits for a v-blank and pushes the pixels into the panel — a PSRAM copy into the shadow frame, a
//! byte swap into the DMA strips, and a QSPI transfer of each strip, all of it on the calling
//! thread. None of that is a [`Phase`]: the phases are the renderer's and end at the hand-off, which
//! is exactly why [`Frame::total`] stops there. [`panel_write`] is the other half, printed as a
//! `[flush]` line directly after the `[phase]` line of the same frame, so that "the phases are
//! cheap and the frame is not" becomes a number instead of a suspicion.

use std::time::Duration;

/// A phase of a frame, as deep as the split needs to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// `UserInterface::build`: the view function, layout, and the widget diff that follows.
    TreeBuild,
    /// `UserInterface::update`: the events, which is where a message lands.
    TreeUpdate,
    /// `UserInterface::draw`: the widgets recording their commands into the layers.
    TreeRecord,
    /// The damage bookkeeping: `diff`, `group`, and this crate's own fold of the result.
    Damage,
    /// Setting the clip and clearing the damaged rectangle back to the background.
    Clear,
    /// Replaying the recorded quads and canvas primitives.
    Quads,
    /// Replaying the recorded text.
    Text,
    /// Rasterising a glyph that was not in the cache yet.
    Rasterise,
    /// Blitting one glyph's cached mask.
    Blits,
}

impl Phase {
    /// How many phases a frame is split into.
    pub const COUNT: usize = 9;

    /// Every phase, in the order a frame goes through them.
    pub const ALL: [Phase; Self::COUNT] = [
        Phase::TreeBuild,
        Phase::TreeUpdate,
        Phase::TreeRecord,
        Phase::Damage,
        Phase::Clear,
        Phase::Quads,
        Phase::Text,
        Phase::Rasterise,
        Phase::Blits,
    ];

    /// The word this phase prints under.
    pub const fn name(self) -> &'static str {
        match self {
            Phase::TreeBuild => "build",
            Phase::TreeUpdate => "update",
            Phase::TreeRecord => "record",
            Phase::Damage => "damage",
            Phase::Clear => "clear",
            Phase::Quads => "quads",
            Phase::Text => "text",
            Phase::Rasterise => "raster",
            Phase::Blits => "blit",
        }
    }

    /// Whether this phase happens *inside* another one.
    ///
    /// Both of the text ones do: a blit is drawn within the text loop and a rasterise within a
    /// blit's mask lookup, which is the split's whole point — the frame's own cost is the outer
    /// phases, and a text-heavy screen spends most of its `text` in `raster`.
    pub const fn is_nested(self) -> bool {
        matches!(self, Phase::Rasterise | Phase::Blits)
    }
}

/// One frame's numbers: nanoseconds per phase, and the glyph counts that explain the text ones.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// The time each phase took, indexed by [`Phase`] as `usize`.
    pub phases: [u64; Phase::COUNT],
    /// Glyphs whose mask was blitted in this frame.
    pub glyphs: u64,
    /// How many of them had to be rasterised first — the part a screen pays only once.
    pub rasterised: u64,
}

impl Frame {
    /// The time one phase of the frame took.
    pub fn phase(&self, phase: Phase) -> Duration {
        Duration::from_nanos(self.phases[phase as usize])
    }

    /// Everything the split accounts for: the outermost phases, so the two that happen inside
    /// `text` are counted there and not again here.
    ///
    /// It is the frame's own cost minus whatever is around it — the panel write, most of all,
    /// which belongs to the host and not to the renderer, and this build's own printing.
    pub fn total(&self) -> Duration {
        Duration::from_nanos(
            self.phases
                .iter()
                .zip(Phase::ALL)
                .filter(|(_, phase)| !phase.is_nested())
                .map(|(nanos, _)| nanos)
                .sum(),
        )
    }

    /// Whether the frame did nothing at all, which is what an idle frame looks like.
    pub fn is_empty(&self) -> bool {
        self.phases.iter().all(|nanos| *nanos == 0) && self.glyphs == 0
    }
}
/// Times `phase` until the guard is dropped.
///
/// The guard is the whole interface on purpose: a phase that ends early — a `?`, a `return`, a
/// `continue` inside it — still gets counted, and a phase that is not taken (a layer with no text
/// in it) costs nothing beyond the drop.
#[must_use = "the guard is what records the phase; dropping it immediately records nothing"]
pub fn start(phase: Phase) -> Timer {
    Timer::new(phase)
}

/// One phase being timed, or nothing at all when the crate is built without `profile`.
#[derive(Debug)]
pub struct Timer {
    #[cfg(feature = "profile")]
    phase: Phase,
    #[cfg(feature = "profile")]
    started: std::time::Instant,
}

impl Timer {
    fn new(phase: Phase) -> Self {
        #[cfg(feature = "profile")]
        {
            Self {
                phase,
                started: std::time::Instant::now(),
            }
        }

        #[cfg(not(feature = "profile"))]
        {
            let _ = phase;

            Self {}
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        #[cfg(feature = "profile")]
        counters::add(self.phase, self.started.elapsed());
    }
}

/// Counts one blitted glyph, saying whether it had to be rasterised first.
pub fn glyph(rasterised: bool) {
    #[cfg(feature = "profile")]
    counters::glyph(rasterised);

    #[cfg(not(feature = "profile"))]
    let _ = rasterised;
}

/// Takes the counters of the frame that has just been drawn, leaving them at zero.
pub fn take() -> Frame {
    #[cfg(feature = "profile")]
    return counters::take();

    #[cfg(not(feature = "profile"))]
    Frame::default()
}

/// Prints the frame that has just been drawn, if it drew anything at all.
///
/// One line, because this goes to a serial console: the phases in milliseconds, the glyph counts,
/// and the sum of the phases so that a phase nothing measured would stand out as a gap against
/// the host's own frame time.
pub fn report() {
    #[cfg(feature = "profile")]
    {
        let frame = take();

        if frame.is_empty() {
            return;
        }

        let mut line = String::from("[phase]");

        for phase in Phase::ALL {
            line.push_str(&format!(
                " {} {:.2}",
                phase.name(),
                frame.phase(phase).as_secs_f64() * 1000.0
            ));
        }

        println!(
            "{line}  glyphs {}/{}  total {:.2}ms",
            frame.glyphs,
            frame.rasterised,
            frame.total().as_secs_f64() * 1000.0
        );
    }
}

/// Times the panel's half of the frame: the wait in front of it, and the transfer into it.
///
/// The two are kept apart because they are different problems. The wait is the frame being paced —
/// a v-blank the renderer could have spent drawing the next frame in, which is what overlapping
/// them would buy. The transfer is work that has to happen: every damaged pixel is copied into the
/// shadow frame, byte-swapped into the DMA strips and pushed over QSPI, and all of it is inside
/// `flush_damage`, where [`Frame::total`] cannot see it.
///
/// The caller marks each half as it passes:
///
/// ```ignore
/// let mut write = iced_pomelo_gfx::profile::panel_write();
///
/// unsafe { hal_display_wait_vsync() };
/// write.waited();
///
/// let sent = flush_damage(damage, pixels);
/// write.sent(sent.regions, sent.pixels);
/// ```
///
/// Dropping the guard prints the line, which is why there is no `report` to match [`report`]: a
/// frame's counters are taken by `[phase]` *before* the panel is touched, so these numbers do not
/// exist yet at that point and cannot join it. The pixel count and the rate beside it are what make
/// the transfer a number rather than a duration — a slow flush that is moving a whole screen and one
/// that is moving a status bar want opposite fixes, and only these tell them apart.
///
/// The rate covers the whole transfer and not the QSPI traffic alone, so read it as a floor on the
/// wire and not as the wire: the shadow copy and the byte swap happen inside that interval, and a
/// full-screen frame is 11.5 ms of transfer at the link's 40 MB/s, so a rate well under that is not
/// evidence of a slow panel. It is the evidence that the copy in front of it is worth splitting out.
#[must_use = "the guard is what times the write; dropping it immediately reports nothing"]
pub fn panel_write() -> PanelWrite {
    PanelWrite::new()
}

/// One panel write being timed, or nothing at all when the crate is built without `profile`.
#[derive(Debug)]
pub struct PanelWrite {
    #[cfg(feature = "profile")]
    started: std::time::Instant,
    /// When the v-blank came back: the end of the wait and the start of the transfer.
    #[cfg(feature = "profile")]
    marked: std::time::Instant,
    #[cfg(feature = "profile")]
    waited: Duration,
    #[cfg(feature = "profile")]
    transferred: Duration,
    /// The transfers sent and the pixels they carried, or `None` if the frame never went out.
    #[cfg(feature = "profile")]
    counts: Option<(usize, u64)>,
}

impl PanelWrite {
    fn new() -> Self {
        #[cfg(feature = "profile")]
        {
            let now = std::time::Instant::now();

            Self {
                started: now,
                marked: now,
                waited: Duration::ZERO,
                transferred: Duration::ZERO,
                counts: None,
            }
        }

        #[cfg(not(feature = "profile"))]
        {
            Self {}
        }
    }

    /// Marks the frame paced: the v-blank it was held for has come back.
    pub fn waited(&mut self) {
        #[cfg(feature = "profile")]
        {
            self.waited = self.started.elapsed();
            self.marked = std::time::Instant::now();
        }
    }

    /// Marks the frame handed over: how many transfers it became, and how many pixels they carried.
    ///
    /// Both are counted after the damage has been merged, so they describe the transfers actually
    /// made rather than the rectangles the compositor reported.
    pub fn sent(&mut self, regions: usize, pixels: u64) {
        #[cfg(feature = "profile")]
        {
            self.transferred = self.marked.elapsed();
            self.counts = Some((regions, pixels));
        }

        #[cfg(not(feature = "profile"))]
        let _ = (regions, pixels);
    }
}

impl Drop for PanelWrite {
    fn drop(&mut self) {
        #[cfg(feature = "profile")]
        {
            // No counts means the frame never reached the panel. That is not a slow frame but a
            // frame that did not happen, and a line of zeros would only be noise between the ones
            // that did.
            let Some((regions, pixels)) = self.counts else {
                return;
            };

            let millis = |spent: Duration| spent.as_secs_f64() * 1000.0;
            let seconds = self.transferred.as_secs_f64();
            let rate = if seconds > 0.0 {
                pixels as f64 * 2.0 / seconds / 1_000_000.0
            } else {
                0.0
            };

            println!(
                "[flush] wait {:.2}  send {:.2}  total {:.2}ms  px {}  regions {}  {rate:.1} MB/s",
                millis(self.waited),
                millis(self.transferred),
                millis(self.waited + self.transferred),
                pixels,
                regions
            );
        }
    }
}

/// The counters themselves: atomics, because a `canvas` widget can draw from a task and the
/// compositor reads them from the loop.
///
/// They are `u32` nanoseconds and not `u64`, because the target this exists for is 32-bit and has
/// no 64-bit atomics at all (the firmware will not compile without this). A phase that ran longer
/// than four seconds would not fit in one, which is why the addition saturates: a number that is
/// wrong in the direction of "small" would read as a fast frame.
#[cfg(feature = "profile")]
mod counters {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::{Frame, Phase};

    static PHASES: [AtomicU32; Phase::COUNT] = [const { AtomicU32::new(0) }; Phase::COUNT];
    static GLYPHS: AtomicU32 = AtomicU32::new(0);
    static RASTERISED: AtomicU32 = AtomicU32::new(0);

    pub(super) fn add(phase: Phase, spent: Duration) {
        let nanos = u32::try_from(spent.as_nanos()).unwrap_or(u32::MAX);

        let _ =
            PHASES[phase as usize].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                Some(total.saturating_add(nanos))
            });
    }

    pub(super) fn glyph(rasterised: bool) {
        GLYPHS.fetch_add(1, Ordering::Relaxed);

        if rasterised {
            RASTERISED.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn take() -> Frame {
        let mut phases = [0; Phase::COUNT];

        for (slot, counter) in phases.iter_mut().zip(&PHASES) {
            *slot = u64::from(counter.swap(0, Ordering::Relaxed));
        }

        Frame {
            phases,
            glyphs: u64::from(GLYPHS.swap(0, Ordering::Relaxed)),
            rasterised: u64::from(RASTERISED.swap(0, Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phase_has_a_name() {
        for phase in Phase::ALL {
            assert!(!phase.name().is_empty());
        }
    }

    #[test]
    fn an_idle_frame_is_empty_and_a_measured_one_is_not() {
        assert!(Frame::default().is_empty());

        let mut frame = Frame::default();
        frame.phases[Phase::Quads as usize] = 1_000;

        assert!(!frame.is_empty());
        assert_eq!(frame.phase(Phase::Quads), Duration::from_micros(1));
        assert_eq!(frame.total(), Duration::from_micros(1));
    }

    /// A blit and a rasterise are drawn *inside* the text loop, so a screen that spends 5 ms in
    /// text and 3 of them in rasterising has spent 5 ms on text and not 8. This is the arithmetic
    /// the device's own numbers depend on: the first version of this line added every phase up and
    /// reported a 618 ms frame inside a 412 ms one, which is the kind of number that sends someone
    /// looking for a bug in the wrong crate.
    #[test]
    fn the_nested_phases_are_not_counted_twice() {
        let mut frame = Frame::default();
        frame.phases[Phase::Text as usize] = 5_000;
        frame.phases[Phase::Rasterise as usize] = 3_000;
        frame.phases[Phase::Blits as usize] = 500;

        assert_eq!(frame.total(), Duration::from_micros(5));
        assert_eq!(frame.phase(Phase::Rasterise), Duration::from_micros(3));
    }

    /// Without the feature the whole module is inert, which is what keeps it out of a build that
    /// does not want it: nothing may be recorded, not even by accident.
    #[test]
    fn a_timer_records_nothing_without_the_feature() {
        let guard = start(Phase::Text);
        drop(guard);

        glyph(true);

        #[cfg(not(feature = "profile"))]
        assert!(take().is_empty());

        #[cfg(feature = "profile")]
        assert_eq!(take().glyphs, 1);
    }
}
