use canape_core::{
    BackButtons, ControllerMode, Fan, FanCurve, FanCurves, FanPreset, LightingEffect,
    LightingSpeed, PerformanceMode,
};

use crate::{BACK_BUTTONS_IN_WASD, Device, HwError, Result, Telemetry};

/// Fan speed at 100 % duty, for the simulated RPM.
const MAX_RPM: u32 = 6000;

/// In-memory device used off the Ally and in tests.
#[derive(Debug)]
pub struct MockDevice {
    pub tdp_watts: u32,
    pub mode: PerformanceMode,
    pub charge_limit: u8,
    pub fps: Option<f32>,
    pub gpu_util: Option<f32>,
    pub cpu_temp_c: f32,
    pub on_battery: bool,
    /// Custom curves the "firmware" holds; a mode write clears them, as on
    /// the Ally.
    pub fan_curves: FanCurves,
    pub fps_limit: Option<u32>,
    pub controller_mode: Option<ControllerMode>,
    pub back_buttons: BackButtons,
    /// Level and colour of the stick rings, once written.
    pub lighting: Option<(u8, [u8; 3], LightingEffect, LightingSpeed)>,
}

impl Default for MockDevice {
    fn default() -> Self {
        Self {
            tdp_watts: 15,
            mode: PerformanceMode::Performance,
            charge_limit: 100,
            fps: None,
            gpu_util: Some(0.7),
            cpu_temp_c: 55.0,
            on_battery: false,
            fan_curves: FanCurves::default(),
            fps_limit: None,
            controller_mode: None,
            back_buttons: BackButtons::default(),
            lighting: None,
        }
    }
}

impl MockDevice {
    /// The curve the fan follows: the custom one, else the mode's default.
    fn curve(&self, fan: Fan) -> FanCurve {
        self.fan_curves
            .get(fan)
            .copied()
            .unwrap_or_else(|| mode_curve(self.mode))
    }

    fn rpm(&self, fan: Fan) -> u32 {
        let duty = self.curve(fan).duty_at(self.cpu_temp_c as u8);
        MAX_RPM * u32::from(duty) / 100
    }
}

/// Stand-ins for the firmware's per-mode curves.
fn mode_curve(mode: PerformanceMode) -> FanCurve {
    match mode {
        PerformanceMode::Turbo => FanPreset::MaxCooling.curve(),
        PerformanceMode::Silent | PerformanceMode::Performance | PerformanceMode::Manual => {
            FanPreset::Balanced.curve()
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
        self.fan_curves = FanCurves::default();
        Ok(())
    }

    fn set_battery_charge_limit(&mut self, percent: u8) -> Result<()> {
        if !(40..=100).contains(&percent) {
            return Err(HwError::OutOfRange(format!(
                "{percent} % not in 40..=100 %"
            )));
        }
        self.charge_limit = percent;
        Ok(())
    }

    fn telemetry(&mut self) -> Result<Telemetry> {
        Ok(Telemetry {
            cpu_temp_c: Some(self.cpu_temp_c),
            gpu_util: self.gpu_util,
            // A game rarely draws its whole limit.
            power_watts: Some(self.tdp_watts as f32 * 0.9),
            battery_percent: Some(80),
            on_battery: Some(self.on_battery),
            cpu_fan_rpm: Some(self.rpm(Fan::Cpu)),
            gpu_fan_rpm: Some(self.rpm(Fan::Gpu)),
        })
    }

    fn frame_rate(&mut self) -> Option<f32> {
        // A cap shows in the frame rate, as with the AMD driver.
        match (self.fps, self.fps_limit) {
            (Some(fps), Some(limit)) => Some(fps.min(limit as f32)),
            (fps, _) => fps,
        }
    }

    fn set_fan_curve(&mut self, fan: Fan, curve: &FanCurve) -> Result<()> {
        if fan == Fan::Mid {
            return Err(HwError::Unsupported("no mid fan"));
        }
        curve.validate().map_err(HwError::OutOfRange)?;
        self.fan_curves.set(fan, Some(*curve));
        Ok(())
    }

    fn reset_fan_curves(&mut self) -> Result<()> {
        self.set_performance_mode(self.mode)
    }

    fn default_fan_curve(&mut self, fan: Fan, mode: PerformanceMode) -> Result<Option<FanCurve>> {
        Ok((fan != Fan::Mid).then(|| mode_curve(mode)))
    }

    fn set_fps_limit(&mut self, fps: Option<u32>) -> Result<()> {
        self.fps_limit = fps;
        Ok(())
    }

    fn set_controller_mode(&mut self, mode: ControllerMode) -> Result<()> {
        self.controller_mode = Some(mode);
        Ok(())
    }

    fn set_back_buttons(&mut self, buttons: &BackButtons, mode: ControllerMode) -> Result<()> {
        if mode == ControllerMode::Wasd {
            return Err(HwError::Unsupported(BACK_BUTTONS_IN_WASD));
        }
        self.controller_mode = Some(mode);
        self.back_buttons = *buttons;
        Ok(())
    }

    fn set_lighting(
        &mut self,
        level: u8,
        rgb: [u8; 3],
        effect: LightingEffect,
        speed: LightingSpeed,
    ) -> Result<()> {
        if level > canape_core::settings::LIGHTING_MAX_LEVEL {
            return Err(HwError::OutOfRange(format!("lighting level {level}")));
        }
        self.lighting = Some((level, rgb, effect, speed));
        Ok(())
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

    #[test]
    fn a_mode_write_drops_custom_curves() {
        let mut device = MockDevice::default();
        let curve = FanPreset::MaxCooling.curve();
        device.set_fan_curve(Fan::Cpu, &curve).unwrap();
        let fast = device.telemetry().unwrap().cpu_fan_rpm.unwrap();
        device
            .set_performance_mode(PerformanceMode::Silent)
            .unwrap();
        assert!(device.fan_curves.is_empty());
        let slow = device.telemetry().unwrap().cpu_fan_rpm.unwrap();
        assert!(slow < fast, "{slow} < {fast}");
        assert!(device.set_fan_curve(Fan::Mid, &curve).is_err());
        let mut bad = curve;
        bad.0[7].1 = 0;
        assert!(device.set_fan_curve(Fan::Gpu, &bad).is_err());
    }
}
