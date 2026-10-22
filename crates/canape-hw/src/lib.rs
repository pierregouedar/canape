//! Hardware abstraction. The service only talks to [`Device`]; the Ally
//! backend lives in `windows` (on top of [`atk`]) and a [`mock::MockDevice`]
//! serves development and tests on any OS.

use canape_core::{ControllerMode, Fan, FanCurve, PerformanceMode};

#[cfg(windows)]
mod adl;
pub mod atk;
pub mod display;
#[cfg(windows)]
mod hid;
pub mod mock;
pub mod pad;
#[cfg(windows)]
pub mod windows;

#[derive(Debug, thiserror::Error)]
pub enum HwError {
    #[error("not supported on this device: {0}")]
    Unsupported(&'static str),
    #[error("value out of range: {0}")]
    OutOfRange(String),
    #[error("firmware refused: {0}")]
    Firmware(String),
    #[error("GPU driver refused: {0}")]
    Driver(String),
    #[error("device I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, HwError>;

/// Sensor readings, all optional: not every device reports every value.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Telemetry {
    pub cpu_temp_c: Option<f32>,
    pub gpu_util: Option<f32>,
    /// What the APU actually draws, in watts (the TDP is only its limit).
    pub power_watts: Option<f32>,
    pub battery_percent: Option<u8>,
    pub on_battery: Option<bool>,
    pub cpu_fan_rpm: Option<u32>,
    pub gpu_fan_rpm: Option<u32>,
}

pub trait Device: Send {
    fn name(&self) -> &str;
    /// Backend details worth a line in the service log.
    fn diagnostics(&self) -> Vec<String> {
        Vec::new()
    }
    /// Supported sustained power range, in watts.
    fn tdp_range(&self) -> (u32, u32);
    fn set_tdp(&mut self, watts: u32) -> Result<()>;
    fn set_performance_mode(&mut self, mode: PerformanceMode) -> Result<()>;
    fn set_battery_charge_limit(&mut self, percent: u8) -> Result<()>;
    fn telemetry(&mut self) -> Result<Telemetry>;
    /// Frame rate of what is on screen, when the GPU driver reports it.
    fn frame_rate(&mut self) -> Option<f32> {
        None
    }
    /// Writes a custom curve. Callers validate it first
    /// ([`FanCurve::validate`]); backends check again. A performance mode
    /// write resets every curve to the mode's own: callers write them again.
    fn set_fan_curve(&mut self, _fan: Fan, _curve: &FanCurve) -> Result<()> {
        Err(HwError::Unsupported("fan curves"))
    }
    /// Every fan back to the current performance mode's curve. May also
    /// reset the power limits: callers apply a manual TDP again.
    fn reset_fan_curves(&mut self) -> Result<()> {
        Err(HwError::Unsupported("fan curves"))
    }
    /// The firmware's own curve for `mode`, when it reports one.
    fn default_fan_curve(&mut self, _fan: Fan, _mode: PerformanceMode) -> Result<Option<FanCurve>> {
        Ok(None)
    }
    /// GPU driver frame rate cap; `None` removes it. Callers check the range
    /// ([`canape_core::settings::FPS_LIMIT_RANGE`]).
    fn set_fps_limit(&mut self, _fps: Option<u32>) -> Result<()> {
        Err(HwError::Unsupported("frame rate limit"))
    }
    fn set_controller_mode(&mut self, _mode: ControllerMode) -> Result<()> {
        Err(HwError::Unsupported("controller mode"))
    }
}

/// Starts the AMD driver's frame metrics. Must run in the user session (the
/// shell): from the service's session 0 the driver refuses, but once started
/// here it reports FPS to the service too. Returns a line for the log.
#[cfg(windows)]
pub fn arm_frame_metrics() -> String {
    match adl::Adl::open() {
        Ok(adl) => adl.describe(),
        Err(err) => format!("adl: {err}"),
    }
}

/// Reads the Ally's own buttons (Command Center, Armoury, back paddles)
/// from the user session; see [`hid::listen`]. Reading needs no rights and
/// writes nothing but the interface's wake-up report.
#[cfg(windows)]
pub fn listen_buttons(on_button: impl FnMut(pad::Button)) -> Result<()> {
    hid::listen(on_button)
}

/// Picks the backend for the machine we run on.
pub fn detect() -> Box<dyn Device> {
    #[cfg(windows)]
    if let Some(device) = windows::AllyDevice::open() {
        return Box::new(device);
    }
    // Development: `CANAPE_MOCK_FPS=60` simulates a running game.
    let fps = std::env::var("CANAPE_MOCK_FPS")
        .ok()
        .and_then(|v| v.parse().ok());
    Box::new(mock::MockDevice {
        fps,
        ..mock::MockDevice::default()
    })
}
