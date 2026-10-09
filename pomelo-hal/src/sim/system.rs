//! Simulated system readings.

use std::time::{Duration, Instant, SystemTime};

use crate::error::HalError;
use crate::traits::SystemBackend;
use crate::types::{ChipInfo, FirmwareInfo, MemoryInfo};

/// The board a desktop process pretends to be.
///
/// It answers like the reference board does — the same chip, the same heap — and every reading is
/// taken now rather than made up once, because the point of the simulator is to let a panel be
/// watched: an uptime that never grew and a heap that never moved would let a readout that never
/// re-reads either pass its tests.
pub struct SimSystem {
    start: Instant,
}

impl SimSystem {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SimSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemBackend for SimSystem {
    /// The chip this project is built for, named the way the chip names itself.
    fn chip(&self) -> Result<ChipInfo, HalError> {
        Ok(ChipInfo {
            model: String::from("ESP32-S3"),
            cores: 2,
            revision: 0,
        })
    }

    /// A name and a version that are plainly a simulator's: a desktop run is not an image, and a
    /// version number here that looked like the board's would be a reading a page could mistake for
    /// one off the hardware.
    fn firmware(&self) -> Result<FirmwareInfo, HalError> {
        Ok(FirmwareInfo {
            name: String::from("pomelo-os"),
            version: String::from("0.1.0 (simulator)"),
        })
    }

    fn uptime(&self) -> Result<Duration, HalError> {
        Ok(self.start.elapsed())
    }

    /// The bench board's heap, with a slow swing through it.
    ///
    /// The swing is the difference between a simulation and a stand-in: a number that never moved
    /// says nothing about whether the panel re-reads it, and re-reading is the one thing the tick
    /// from the platform exists for. The figures are the memory page's own bench values, so a host
    /// run and the notes about the real board agree about what this chip has.
    fn memory(&self) -> Result<MemoryInfo, HalError> {
        /// Every byte of heap: the memory page's "8,519,680 bytes (8.5 MB)".
        const TOTAL_BYTES: u64 = 8_519_680;
        /// What is handed out with nothing much running: its "1,852,416 bytes (1.85 MB)".
        const BASE_USED_BYTES: u64 = 1_852_416;
        /// How far the swing carries it, either way. Small: a settled system does not breathe much.
        const SWING_BYTES: u64 = 196_608;
        /// One whole swing, in seconds.
        const PERIOD_SECS: f32 = 30.0;

        let phase = self.start.elapsed().as_secs_f32() / PERIOD_SECS * std::f32::consts::TAU;
        let swing = SWING_BYTES as f32 * (0.5 + 0.5 * phase.sin());
        let used = BASE_USED_BYTES + swing as u64;

        Ok(MemoryInfo {
            total_bytes: TOTAL_BYTES,
            free_bytes: TOTAL_BYTES - used,
        })
    }

    /// The host's clock, which is the honest thing for a desktop run to show.
    ///
    /// `Some` and never `None`: a process with a wall clock in front of it is not a board that has
    /// never been told the time, and a simulator that answered `None` would leave the one branch of
    /// the panel that handles it untestable by running the app.
    fn clock(&self) -> Result<Option<SystemTime>, HalError> {
        Ok(Some(SystemTime::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every reading the simulator promises is one a caller can act on: a heap that divides, an
    /// uptime that starts near zero, and a clock.
    #[test]
    fn the_simulated_board_answers_like_a_board() {
        let system = SimSystem::new();

        let memory = system.memory().expect("the simulator always answers");
        assert!(memory.free_bytes <= memory.total_bytes, "{memory:?}");
        assert!(memory.used_bytes() > 0, "{memory:?}");

        let used = memory.used_percent();
        assert!((0.0..100.0).contains(&used), "{used}% is not a heap in use");

        assert!(system.uptime().unwrap() < Duration::from_secs(60));
        assert!(system.clock().unwrap().is_some(), "a desktop has a clock");

        let chip = system.chip().unwrap();
        assert_eq!(chip.cores, 2, "the board this pretends to be has two cores");
    }

    /// The two readings that exist so that a page can be caught *not* re-reading them.
    #[test]
    fn the_readings_that_move_on_their_own_do_move() {
        let system = SimSystem::new();
        let first = system.memory().unwrap().free_bytes;
        let booted = system.uptime().unwrap();

        std::thread::sleep(Duration::from_millis(30));

        assert!(system.uptime().unwrap() > booted);
        assert_ne!(
            system.memory().unwrap().free_bytes,
            first,
            "a heap that never moved would hide a panel that never re-read one"
        );
    }
}
