//! AXP2101 power backend (the board's PMIC).
//!
//! Raw `extern "C"` bindings for the `hal_power_*` C API live in the private
//! `ffi` module below; the safe trait impl is a thin wrapper over them.

use pomelo_hal::{HalError, PowerBackend};

mod ffi {
    extern "C" {
        /// `esp_err_t hal_power_init(void)` (0 = `ESP_OK`).
        pub fn hal_power_init() -> i32;
        /// `int32_t hal_power_get_battery_percent(void)`; negative = unavailable.
        pub fn hal_power_get_battery_percent() -> i32;
        /// `bool hal_power_is_charging(void)`.
        pub fn hal_power_is_charging() -> bool;
        /// `int32_t hal_power_get_battery_voltage_mv(void)`; 0 = unavailable.
        pub fn hal_power_get_battery_voltage_mv() -> i32;
        /// `esp_err_t hal_power_get_chip_temperature_dc(int32_t *out_dc)`; tenths of a degree.
        pub fn hal_power_get_chip_temperature_dc(out_dc: *mut i32) -> i32;
        /// `void hal_power_restart(void)`; does not return.
        pub fn hal_power_restart();
    }
}

pub struct EspPower;

impl EspPower {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for EspPower {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerBackend for EspPower {
    fn init(&mut self) -> Result<(), HalError> {
        // Idempotent on the C side; returns ESP_OK if the PMIC is already set up.
        HalError::from_code(unsafe { ffi::hal_power_init() })
    }

    #[inline]
    fn battery_percent(&self) -> Result<u8, HalError> {
        let raw = unsafe { ffi::hal_power_get_battery_percent() };
        if raw < 0 {
            Err(HalError::NotSupported) // battery absent / unknown
        } else {
            Ok(raw.min(100) as u8)
        }
    }

    #[inline]
    fn is_charging(&self) -> Result<bool, HalError> {
        Ok(unsafe { ffi::hal_power_is_charging() })
    }

    #[inline]
    fn battery_voltage_mv(&self) -> Result<u32, HalError> {
        let raw = unsafe { ffi::hal_power_get_battery_voltage_mv() };
        if raw <= 0 {
            Err(HalError::NotSupported)
        } else {
            Ok(raw as u32)
        }
    }

    /// The AXP2101's own die temperature, from its 14-bit temperature ADC.
    ///
    /// The C side converts in tenths of a degree and says so in its return code; the tenths are kept
    /// as an integer across the boundary and divided once, here, because a `float` in an FFI
    /// signature is one more thing to agree on and this is the only place the number is a float.
    #[inline]
    fn chip_temperature_c(&self) -> Result<f32, HalError> {
        let mut dc = 0i32;
        let code = unsafe { ffi::hal_power_get_chip_temperature_dc(&mut dc) };

        if code != 0 {
            return Err(HalError::Internal(code));
        }

        Ok(dc as f32 / 10.0)
    }

    /// Reset the SoC. The call below does not return; the `Ok` is what the signature needs and not
    /// something this can produce.
    fn restart(&self) -> Result<(), HalError> {
        unsafe { ffi::hal_power_restart() };
        Ok(())
    }
}
