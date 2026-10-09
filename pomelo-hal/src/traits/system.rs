//! System interface — what this machine *is*, and what it is doing.

use std::time::{Duration, SystemTime};

use crate::error::HalError;
use crate::types::{ChipInfo, FirmwareInfo, MemoryInfo};

/// The board's own account of itself: the chip, the firmware on it, and the three readings that
/// move while it runs.
///
/// # Readings, not a conversation
///
/// Every method here returns immediately and answers about *right now*. There is no start/poll pair
/// and no queue, which is the whole difference between this and the radio: a caller asks how long
/// the board has been up the way it asks what time it is.
///
/// That is also why nothing here is pushed as an event. A reading is taken by whoever wants it, and
/// the only thing a page showing one needs from the platform is a nudge that time has passed — which
/// is [`SystemEvent::Tick`](crate::types::SystemEvent::Tick)'s whole purpose.
///
/// # What is not here
///
/// The two temperatures this board can read. The PMIC's die temperature belongs to the power system
/// that measures it ([`PowerBackend::chip_temperature_c`](crate::traits::PowerBackend::chip_temperature_c))
/// and the IMU's to the IMU
/// ([`ImuBackend::temperature_c`](crate::traits::ImuBackend::temperature_c)). A readout that wants
/// to say "how warm is this" asks the part that knows, and the number it gets says which part that
/// was.
pub trait SystemBackend: Send + Sync {
    /// The chip: the part number it reports, its cores, and its revision.
    fn chip(&self) -> Result<ChipInfo, HalError>;

    /// The firmware: the project's name, and the version built into the image.
    fn firmware(&self) -> Result<FirmwareInfo, HalError>;

    /// How long the board has been up.
    ///
    /// A `Duration` and not a formatted string: "6 hours 32 minutes" is a language, and which
    /// language a machine speaks is the interface's business rather than the HAL's.
    fn uptime(&self) -> Result<Duration, HalError>;

    /// The heap.
    fn memory(&self) -> Result<MemoryInfo, HalError>;

    /// The clock, when the board has one and it has been set.
    ///
    /// `Ok(None)` is a board whose clock has never been set: a backed-up RTC that came up holding no
    /// time, or an image whose first NTP sync has not happened yet. That is the ordinary state of a
    /// board minutes after its first boot, and it is deliberately not the same answer as an epoch of
    /// zero — a caller drawing "1970-01-01" over a board that simply does not know the time yet
    /// would be inventing a fact about it.
    fn clock(&self) -> Result<Option<SystemTime>, HalError>;
}
