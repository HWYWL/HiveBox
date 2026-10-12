//! The readings this page is made of, gathered from the board in one pass.
//!
//! # Why the readings are a type of their own
//!
//! Because the page is drawn from them and not from the board. Everything in [`Readings`] is a plain
//! value — numbers, strings, and the two `VolumeInfo`s a slot was asked for — so the whole of the
//! drawing is a pure function of it and the whole of the *reading* is a pure function of a board. The
//! split is what makes either testable: [`Readings::of`] runs against a simulated board with no window
//! in sight, and the page runs against a `Readings` built by hand with no board in sight.
//!
//! # One pass, and the two slow questions left out
//!
//! [`Readings::of`] asks each backend once, takes the lock, reads, and lets it go — so a page drawn
//! while another app holds one of them is a page with a `—` in a row rather than a page that has
//! stopped. What it deliberately does *not* do is ask anything slow: whether there is a card in the
//! slot is an SDMMC transaction ([`StorageBackend::refresh`]), and whether the NAS is awake is a login
//! ([`NasBackend::refresh`]). Those are [`Readings::again`], which the app calls when the page opens
//! and when a finger taps the row, and neither returns an answer — the answers arrive at the next
//! [`Readings::of`], which is the start-and-poll shape every slow backend here uses.
//!
//! [`StorageBackend::refresh`]: pomelo_hal::StorageBackend::refresh
//! [`NasBackend::refresh`]: pomelo_hal::NasBackend::refresh

use std::time::Duration;

use pomelo_hal::{
    Board, MemoryInfo, NasReading, VolumeInfo, WifiStatus,
};

/// The battery, as one look at the gauge saw it.
///
/// `percent` and `charging` are not optional and the two below them are, which is the shape of the
/// question rather than a shortcut: a board that cannot read its gauge at all has no battery to draw
/// and [`Readings::battery`] is `None`, while a board that reads a level but has no way to measure the
/// cell's voltage *does* have a battery — and a `0.00 V` drawn for it would be a measurement this board
/// never made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    /// How full, `0..=100`.
    pub percent: u8,
    /// Whether the charger is in.
    pub charging: bool,
    /// The cell's voltage, when the board can measure it.
    pub voltage_mv: Option<u32>,
    /// The chip's temperature — not the cell's, and the only thermometer a board like this has.
    pub temperature_c: Option<f32>,
}

impl Battery {
    /// Read the gauge, or `None` if it cannot be read.
    ///
    /// One `None` for two failures, because they are one thing to a reader: a page that could not read
    /// the level has no battery to draw, and whether the charger was in as well is not a distinction
    /// anybody can act on.
    fn of(board: &Board) -> Option<Self> {
        let power = board.power();

        let percent = power.battery_percent().ok()?;
        let charging = power.is_charging().ok()?;

        Some(Self {
            percent,
            charging,
            voltage_mv: power.battery_voltage_mv().ok(),
            temperature_c: power.chip_temperature_c().ok(),
        })
    }
}

/// Everything on the page, from one pass over the board's backends.
///
/// `Default` is "nothing has been read yet", and it is what the page is drawn from between being
/// opened and the first tick: every number is a `—`, which is the same thing it draws for a board that
/// cannot answer, and both are things a page is allowed to say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings {
    /// The battery, or `None` for a board that has no gauge — this one, when it is on a desk.
    pub battery: Option<Battery>,
    /// The radio, however it is: connected, connecting, or off.
    pub wifi: WifiStatus,
    /// Every mounted filesystem. A card that is not in the slot is not in here — see
    /// [`Readings::card`], which is the question the page actually asks.
    pub volumes: Vec<VolumeInfo>,
    /// The internal heap.
    pub heap: Option<MemoryInfo>,
    /// The external pool, `None` on a board that has none.
    pub psram: Option<MemoryInfo>,
    /// What the last look at the NAS found.
    pub nas: NasReading,
    /// How long the board has been up.
    pub uptime: Duration,
}

impl Readings {
    /// Read the board once.
    ///
    /// Each backend is asked in a statement of its own and not inside one `Self { .. }`: a field's
    /// temporary `MutexGuard` lives until the whole struct literal is built, so `board.system()` read
    /// three times in one literal would take a lock that is still held and deadlock against itself.
    /// Binding each answer to a name first lets the guard go before the next backend is asked.
    pub fn of(board: &Board) -> Self {
        let battery = Battery::of(board);
        let wifi = board.wifi().status();
        let volumes = board.storage().volumes().unwrap_or_default();
        let heap = board.system().memory().ok();
        let psram = board.system().psram().ok().flatten();
        let nas = board.nas().reading();
        let uptime = board.system().uptime().unwrap_or_default();

        Self {
            battery,
            wifi,
            volumes,
            heap,
            psram,
            nas,
            uptime,
        }
    }

    /// Ask the two slow backends to look again.
    ///
    /// The answers do not come back here: a card is probed and a NAS is logged into on the backends'
    /// own time, and what this does is start both so that the next [`Readings::of`] has them. Both are
    /// safe to ask repeatedly — a look already in flight is not a second look, and a slot already
    /// probed is a slot that answers from what it found.
    ///
    /// Each of the two is *let go of* before the other is taken, because these are the two backends
    /// most likely to be held by another app at this moment, and this page has nothing to do but wait.
    pub fn again(board: &Board) {
        {
            let mut storage = board.storage();
            let _ = storage.refresh();
        }

        {
            let mut nas = board.nas();
            let _ = nas.refresh();
        }
    }

    /// The built-in filesystem, if it is mounted.
    pub fn internal(&self) -> Option<&VolumeInfo> {
        self.volumes.iter().find(|volume| {
            volume.kind == pomelo_hal::VolumeKind::Internal
        })
    }

    /// The card in the slot, if there is one.
    pub fn card(&self) -> Option<&VolumeInfo> {
        self.volumes.iter().find(|volume| {
            volume.kind == pomelo_hal::VolumeKind::Removable
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pomelo_hal::NasStatus;

    /// A simulated board answers every one of the questions this page asks, which is the whole reason
    /// the page can be run on a desktop at all.
    #[test]
    fn a_simulated_board_answers_every_question_on_the_page() {
        let board = Board::simulated();
        let readings = Readings::of(&board);

        let battery = readings.battery.expect("the simulator has a gauge");
        assert!(battery.percent <= 100, "a level is a share of a full cell");
        assert!(
            battery.voltage_mv.is_some(),
            "and a voltage, which is the reading that moves"
        );

        assert!(readings.heap.is_some(), "a board has a heap");
        assert!(readings.psram.is_some(), "and this one has a pool beside it");

        assert!(
            readings.internal().is_some(),
            "the built-in filesystem is always mounted"
        );

        assert_eq!(
            readings.nas.status,
            NasStatus::Unconfigured,
            "a board that has not been told about a NAS says so, and that is not a failure"
        );
    }

    /// The two filesystems are told apart by kind and not by mount point, which is what the slot being
    /// empty has to be drawn from.
    #[test]
    fn the_two_filesystems_are_told_apart_by_kind() {
        let board = Board::simulated();
        let readings = Readings::of(&board);

        let internal = readings.internal().expect("always there");
        let card = readings.card().expect("the simulator has one in the slot");

        assert_ne!(internal.mount_point, card.mount_point);
        assert_ne!(
            internal.kind, card.kind,
            "soldered on and taken out are not the same thing"
        );

        assert!(internal.used_percent() <= 100.0);
        assert!(card.used_percent() <= 100.0);
    }

    /// Nothing read yet is every number missing, which is what the page draws between being opened and
    /// its first tick — and is the same thing it draws for a board that cannot answer.
    #[test]
    fn before_the_first_read_everything_is_missing() {
        let readings = Readings::default();

        assert!(readings.battery.is_none());
        assert!(readings.heap.is_none());
        assert!(readings.volumes.is_empty());
        assert!(readings.internal().is_none());
        assert!(readings.card().is_none());
        assert_eq!(readings.uptime, Duration::ZERO);
    }

    /// Asking the slow backends to look again is a question and not an answer: it returns with nothing
    /// and the readings are what they were until the next pass.
    #[test]
    fn asking_again_starts_a_look_and_returns() {
        let board = Board::simulated();
        let before = Readings::of(&board);

        Readings::again(&board);

        let after = Readings::of(&board);
        assert_eq!(
            before.internal().map(|v| v.mount_point.clone()),
            after.internal().map(|v| v.mount_point.clone()),
            "a look in flight has not changed what is mounted"
        );
    }
}
