//! Simulated system readings.

use std::time::{Duration, Instant, SystemTime};

use crate::error::HalError;
use crate::traits::SystemBackend;
use crate::types::{ChipInfo, FirmwareInfo, MemoryInfo, TaskInfo, TaskReading, TaskState};

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

    /// A name, a version and a build time that are plainly a simulator's: a desktop run is not an
    /// image, and a version number here that looked like the board's would be a reading a page could
    /// mistake for one off the hardware.
    ///
    /// The build time is a fixed instant and not `SystemTime::now()`, which is the one thing it must
    /// not be: a host run that read the clock here would draw a firmware built a moment ago, every
    /// moment, and a test asserting the row says anything at all would be asserting nothing.
    fn firmware(&self) -> Result<FirmwareInfo, HalError> {
        Ok(FirmwareInfo {
            name: String::from("pomelo-os"),
            version: String::from("0.1.0 (simulator)"),
            built: Some(String::from("2026-01-01 00:00:00")),
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

    /// A board's worth of tasks.
    ///
    /// The names are the ones this board's firmware really runs — the two idle tasks, the two
    /// inter-processor ones, the timer service — because a list of invented names would teach whoever
    /// was reading it nothing about what they were looking at. The numbers wobble, and the states move
    /// in and out of "waiting", for the same reason the heap above swings: a task list that never
    /// changed would let a page that never re-read one pass its tests.
    ///
    /// (A real high-water mark only ever shrinks. Wobbling it is the simulator's business and not a
    /// claim about the board: see the note at the top of this file.)
    ///
    /// The run times are shares of this simulation's own uptime, and they grow because it does. On the
    /// board they come out of a counter that is counting the same clock the uptime read is, which is
    /// why the two agree here rather than being made up twice.
    fn tasks(&self) -> Result<TaskReading, HalError> {
        // What the board runs: `(name, priority, core, least stack free in bytes, waiting?, weight)`.
        //
        // The weight is the task's share of the counter, in hundredths, and the nine of them are meant
        // to add up to a whole one: a share is only a share of the sum, so a share that did not would
        // make this reading a different size than the uptime beside it. Which task gets which is the
        // simulator's business and not a claim about this firmware — the two idle ones are equal on
        // purpose, so that nothing here reads as a finding about where the board's time goes.
        const TASKS: [(&str, u8, u8, u32, bool, u32); 9] = [
            ("main", 1, 0, 2_048, false, 12),
            ("pomelo-ui", 5, 1, 1_024, false, 33),
            ("esp_timer", 22, 0, 3_072, true, 2),
            ("ipc0", 24, 0, 512, false, 1),
            ("ipc1", 24, 1, 512, false, 1),
            ("wifi", 23, 0, 4_096, true, 5),
            ("Tmr Svc", 1, 0, 1_536, false, 2),
            ("IDLE0", 0, 0, 256, true, 22),
            ("IDLE1", 0, 1, 256, true, 22),
        ];

        /// How far a stack reading swings, either way, and one whole swing, in seconds.
        const SWING_BYTES: u32 = 96;
        const PERIOD_SECS: f32 = 17.0;
        /// How long a task spends waiting, out of a cycle of its own.
        const WAIT_PERIOD_SECS: f32 = 4.0;

        let uptime = self.start.elapsed();
        let elapsed = uptime.as_secs_f32();
        let phase = elapsed / PERIOD_SECS * std::f32::consts::TAU;

        // The counter every row below is a share of. Summed from the table rather than written down
        // beside it: two numbers that have to agree eventually will not.
        let total = uptime.as_micros() as u64;
        let weights: u64 = TASKS.iter().map(|&(.., weight)| u64::from(weight)).sum();

        Ok(TaskReading {
            tasks: TASKS
                .iter()
                .enumerate()
                .map(|(index, &(name, priority, core, free, waits, weight))| {
                    // Each task swings out of step with the others and waits on a cycle of its own, so
                    // a page drawing one number — or one state — for all of them can be caught doing
                    // it.
                    let swing = SWING_BYTES as f32 * (0.5 + 0.5 * (phase + index as f32).sin());
                    let waiting = waits
                        && (elapsed / WAIT_PERIOD_SECS + index as f32).fract() < 0.5;

                    TaskInfo {
                        name: String::from(name),
                        state: if waiting {
                            TaskState::Blocked
                        } else {
                            TaskState::Running
                        },
                        priority,
                        stack_free_bytes: free + swing as u32,
                        core: Some(core),
                        // The one this reading was taken from: the task the panel is drawn by.
                        current: index == 1,
                        run_time: Some(Duration::from_micros(total * u64::from(weight) / weights)),
                    }
                })
                .collect(),
            total_run_time: Some(uptime),
        })
    }

    /// The 8 MB of PSRAM beside the internal heap, which is the pool the framebuffer and the log ring
    /// come out of. `Some` and never `None`: this board has one, and a simulator answering `None` would
    /// leave the branch that draws "no external memory" untestable by running the app.
    fn psram(&self) -> Result<Option<MemoryInfo>, HalError> {
        /// The board's external RAM, and what is handed out with nothing much running.
        const TOTAL_BYTES: u64 = 8 * 1024 * 1024;
        const BASE_USED_BYTES: u64 = 2 * 1024 * 1024;
        /// One whole swing, in seconds — slower than the heap's, because a framebuffer is not handed
        /// back and forth the way small allocations are.
        const PERIOD_SECS: f32 = 47.0;

        let phase = self.start.elapsed().as_secs_f32() / PERIOD_SECS * std::f32::consts::TAU;
        let used = BASE_USED_BYTES + (262_144.0 * (0.5 + 0.5 * phase.sin())) as u64;

        Ok(Some(MemoryInfo {
            total_bytes: TOTAL_BYTES,
            free_bytes: TOTAL_BYTES - used,
        }))
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
        let psram = system.psram().unwrap().expect("this board has PSRAM").free_bytes;
        let booted = system.uptime().unwrap();

        std::thread::sleep(Duration::from_millis(30));

        assert!(system.uptime().unwrap() > booted);
        assert_ne!(
            system.memory().unwrap().free_bytes,
            first,
            "a heap that never moved would hide a panel that never re-read one"
        );
        assert_ne!(
            system.psram().unwrap().unwrap().free_bytes,
            psram,
            "and an external pool with nothing happening in it is a pool nobody is drawing"
        );
    }

    /// The task list names the tasks a board actually runs, and says which one it was read from.
    ///
    /// The names and not the count, because the names are what make a list like this worth looking at:
    /// `IDLE0` and `ipc1` are the same ones that appear in the idf monitor's own output, and a
    /// simulator that invented them would teach a reader nothing.
    #[test]
    fn the_task_list_names_what_a_board_runs() {
        let system = SimSystem::new();
        let reading = system.tasks().expect("the simulator always answers");
        let tasks = &reading.tasks;

        assert!(tasks.len() >= 4, "{tasks:?}");
        assert!(
            tasks.iter().any(|task| task.name == "IDLE0"),
            "an idle task is the one task every scheduler has"
        );
        assert!(
            tasks.iter().all(|task| !task.name.is_empty() && task.core.is_some()),
            "every task has a name and, on this chip, a core"
        );
        assert_eq!(
            tasks.iter().filter(|task| task.current).count(),
            1,
            "the reading was taken from exactly one task"
        );
        assert!(
            tasks.iter().all(|task| task.stack_free_bytes > 0),
            "a task with no stack left would have faulted before it could be listed"
        );

        let psram = system.psram().unwrap().expect("this board has PSRAM");
        assert!(psram.free_bytes < psram.total_bytes, "{psram:?}");
        assert!(psram.used_percent() > 0.0);
    }

    /// Each task's run time is a share of a total the reading carries beside it, and the shares add up.
    ///
    /// The relation is what makes the numbers usable and what a panel cannot show: a percentage is only
    /// a percentage of the total it was taken against, so a list whose shares did not sum to the whole
    /// would be a list disagreeing with the clock it came from. Which task gets which share is the
    /// simulator's business; that they are shares of one counter is not.
    #[test]
    fn the_task_run_times_are_shares_of_the_total_beside_them() {
        let system = SimSystem::new();

        // Let the counter count. At the microseconds it takes to build the list every weight in the
        // table floors to zero, and a share of a total of zero is not a small share — it is no share
        // at all, which would make this test one that asserts nothing.
        std::thread::sleep(Duration::from_millis(10));

        let reading = system.tasks().expect("the simulator always answers");
        let total = reading.total_run_time.expect("the simulator counts run time");
        assert!(total >= Duration::from_millis(10), "{total:?}");

        let summed: Duration = reading.tasks.iter().filter_map(|task| task.run_time).sum();
        assert!(summed <= total, "{summed:?} out of {total:?}");

        let shares: f32 = reading
            .tasks
            .iter()
            .filter_map(|task| reading.share_of(task))
            .sum();
        assert!(
            (shares - 100.0).abs() <= 1.0,
            "the shares add up to {shares}, and a share that is not of the whole is not a share"
        );
    }
}
