//! Power / battery interface.

use crate::error::HalError;

/// Battery and power-supply management (AXP2101 PMIC on the reference board).
pub trait PowerBackend: Send + Sync {
    /// Initialize the PMIC / ADC subsystem.
    fn init(&mut self) -> Result<(), HalError>;
    /// Battery charge percentage in `0..=100`.
    fn battery_percent(&self) -> Result<u8, HalError>;
    /// Whether the battery is currently charging.
    fn is_charging(&self) -> Result<bool, HalError>;
    /// Battery voltage in millivolts.
    fn battery_voltage_mv(&self) -> Result<u32, HalError>;

    /// The PMIC's own die temperature, in degrees Celsius.
    ///
    /// The die and not the cell. This board carries a two-pin battery, so there is no NTC on the
    /// TS pin and no pack temperature exists to read — the firmware turns that channel off rather
    /// than measure a floating pin. What the power path does have is the chip's own temperature,
    /// which is this, and which is the closest thing to "how warm is the power system" that the
    /// hardware can answer. A backend that cannot read one says so rather than inventing a number.
    fn chip_temperature_c(&self) -> Result<f32, HalError>;

    /// Restart the board.
    ///
    /// A reset of the SoC, not a power cycle: the rails stay up and whatever the chip keeps across a
    /// reset — the RTC, the wake reason — is still there afterwards, which is the difference between
    /// this and taking the battery out.
    ///
    /// On the device this call does not return, and the `Ok` it will never produce is the signature's
    /// price rather than a thing that happens. What *can* return is a failure, and a caller that
    /// asked to restart and is still running is a caller that should say so.
    ///
    /// A backend with nothing to reset — the desktop simulator, whose process is not the board —
    /// returns `Ok` without doing anything, so a run on a host can open the question and answer it.
    fn restart(&self) -> Result<(), HalError>;
}
