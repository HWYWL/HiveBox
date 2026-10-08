//! Simulated battery (AXP2101 PMIC).

use std::time::Instant;

use crate::error::HalError;
use crate::traits::PowerBackend;

/// Simulated battery that slowly drains while the app is running.
pub struct SimPower {
    start: Instant,
    initial_percent: u8,
    secs_per_percent: f32,
    charging: bool,
}

impl SimPower {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            initial_percent: 88,
            secs_per_percent: 45.0,
            charging: true,
        }
    }

    fn percent(&self) -> u8 {
        let drained = (self.start.elapsed().as_secs_f32() / self.secs_per_percent) as u8;
        self.initial_percent.saturating_sub(drained).max(1)
    }
}

impl Default for SimPower {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerBackend for SimPower {
    fn init(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    fn battery_percent(&self) -> Result<u8, HalError> {
        Ok(self.percent())
    }

    fn is_charging(&self) -> Result<bool, HalError> {
        Ok(self.charging)
    }

    fn battery_voltage_mv(&self) -> Result<u32, HalError> {
        // Linear map 0%..100% -> 3300..4200 mV.
        Ok(3300 + self.percent() as u32 * 9)
    }

    /// The PMIC's die temperature, in the shape a real one has: a slow swing, and a degree or so
    /// above it while the charger is running.
    ///
    /// A constant would be a stand-in that could not show this reading working. The die temperature
    /// is the one power figure that moves on its own — the fuel gauge changes by the hour and the
    /// charger's state by the plug — so a stand-in that never moved would let a page that never
    /// refreshed it, and a platform that never pushed it, both pass their tests.
    fn chip_temperature_c(&self) -> Result<f32, HalError> {
        /// The bench board's idle reading — and the number the page used to draw as a literal.
        const AMBIENT_C: f32 = 31.4;
        /// How far the swing carries it, either side of idle.
        const SWING_C: f32 = 1.6;
        /// One whole swing, in seconds. Slow: a die is not a thermometer.
        const PERIOD_SECS: f32 = 45.0;
        /// What the charger adds while it is running.
        const CHARGING_RISE_C: f32 = 1.2;

        let phase = self.start.elapsed().as_secs_f32() / PERIOD_SECS * std::f32::consts::TAU;
        let rise = if self.charging { CHARGING_RISE_C } else { 0.0 };

        Ok(AMBIENT_C + SWING_C * phase.sin() + rise)
    }

    /// Nothing happens, and that is the honest answer.
    ///
    /// There is no board here to reset — this backend is a value inside a desktop process, and
    /// killing the process is not something a settings page should be able to do. It returns `Ok`
    /// so that the app can be exercised end to end on a host: the question opens, the answer is
    /// taken, and the run continues, which is the only way to see the dialog at all.
    fn restart(&self) -> Result<(), HalError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The simulated die temperature is a temperature: somewhere a chip can be, and not the same
    /// number whatever the charger is doing.
    #[test]
    fn the_simulated_die_temperature_is_in_range_and_follows_the_charger() {
        let charging = SimPower::new();

        let warm = charging
            .chip_temperature_c()
            .expect("the simulator always answers");

        assert!((-20.0..=85.0).contains(&warm), "{warm} °C is not a die temperature");

        // The same board with the charger off reads cooler — which is the difference the page has to
        // be able to show, and the reason this is not a constant.
        let unplugged = SimPower {
            charging: false,
            ..SimPower::new()
        };

        let cool = unplugged
            .chip_temperature_c()
            .expect("the simulator always answers");

        assert!(warm > cool, "{warm} °C with the charger, {cool} °C without");
    }
}
