//! ROG Ally backend (Windows only).
//!
//! - ASUS firmware through the `\\.\ATKACPI` device (DEVS/DSTS calls via
//!   `DeviceIoControl`): performance mode, fans, SPL/SPPT/FPPT power limits,
//!   battery charge limit;
//! - Ryzen SMU as a fallback for power limits;
//! - sensors (temperatures, GPU load) and the built-in controller HID.
//!
//! sources for each Ally model before any write to the firmware.

use canape_core::PerformanceMode;

use crate::{Device, HwError, Result, Telemetry};

pub struct AllyDevice {
    _private: (),
}

impl AllyDevice {
    /// Returns `None` when not running on a supported Ally.
    pub fn open() -> Option<Self> {
        None
    }
}

impl Device for AllyDevice {
    fn name(&self) -> &str {
        "ROG Ally"
    }

    fn tdp_range(&self) -> (u32, u32) {
        (7, 30)
    }

    fn set_tdp(&mut self, _watts: u32) -> Result<()> {
        Err(HwError::Unsupported("set_tdp"))
    }

    fn set_performance_mode(&mut self, _mode: PerformanceMode) -> Result<()> {
        Err(HwError::Unsupported("set_performance_mode"))
    }

    fn set_battery_charge_limit(&mut self, _percent: u8) -> Result<()> {
        Err(HwError::Unsupported("set_battery_charge_limit"))
    }

    fn telemetry(&mut self) -> Result<Telemetry> {
        Err(HwError::Unsupported("telemetry"))
    }
}
