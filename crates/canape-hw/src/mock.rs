use canape_core::PerformanceMode;

use crate::{Device, HwError, Result, Telemetry};

/// In-memory device used off the Ally and in tests.
#[derive(Debug)]
pub struct MockDevice {
    pub tdp_watts: u32,
    pub mode: PerformanceMode,
    pub charge_limit: u8,
}

impl Default for MockDevice {
    fn default() -> Self {
        Self {
            tdp_watts: 15,
            mode: PerformanceMode::Performance,
            charge_limit: 100,
        }
    }
}

impl Device for MockDevice {
    fn name(&self) -> &str {
        "mock"
    }

    fn tdp_range(&self) -> (u32, u32) {
        (7, 30)
    }

    fn set_tdp(&mut self, watts: u32) -> Result<()> {
        let (min, max) = self.tdp_range();
        if !(min..=max).contains(&watts) {
            return Err(HwError::OutOfRange(format!(
                "{watts} W not in {min}..={max} W"
            )));
        }
        self.tdp_watts = watts;
        self.mode = PerformanceMode::Manual;
        Ok(())
    }

    fn set_performance_mode(&mut self, mode: PerformanceMode) -> Result<()> {
        self.mode = mode;
        Ok(())
    }

    fn set_battery_charge_limit(&mut self, percent: u8) -> Result<()> {
        if !(20..=100).contains(&percent) {
            return Err(HwError::OutOfRange(format!(
                "{percent} % not in 20..=100 %"
            )));
        }
        self.charge_limit = percent;
        Ok(())
    }

    fn telemetry(&mut self) -> Result<Telemetry> {
        Ok(Telemetry {
            cpu_temp_c: Some(55.0),
            gpu_util: Some(0.7),
            battery_percent: Some(80),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_tdp_out_of_range() {
        let mut device = MockDevice::default();
        assert!(device.set_tdp(50).is_err());
        assert!(device.set_tdp(12).is_ok());
        assert_eq!(device.tdp_watts, 12);
        assert_eq!(device.mode, PerformanceMode::Manual);
    }
}
