//! ROG Ally backend (Windows only), on top of the ATK ACPI device.
//!
//! Needs the service's LocalSystem rights to open `\\.\ATKACPI`. Armoury
//! one of them should run.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use canape_core::{ControllerMode, Fan, FanCurve, PerformanceMode};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

use crate::adl::Adl;
use crate::atk::{self, Model};
use crate::{Device, HwError, Result, Telemetry, hid, pad};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// `HKLM\HARDWARE\DESCRIPTION\System\BIOS\SystemProductName`.
fn product_name() -> Option<String> {
    let key = wide(r"HARDWARE\DESCRIPTION\System\BIOS");
    let value = wide("SystemProductName");
    let mut buffer = [0u16; 128];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

pub struct AllyDevice {
    handle: HANDLE,
    model: Model,
    /// FPS and GPU load, when the AMD driver exposes them.
    adl: Option<Adl>,
    /// Why ADL is unavailable, for the service log.
    adl_error: Option<String>,
    /// Last [`atk::PERFORMANCE_MODE`] value written, and when: fan curves
    /// wait after it, and a curve reset writes it again.
    firmware_mode: Option<(u32, Instant)>,
    /// The init report goes once, before the first controller command.
    controller_ready: bool,
}

// The handle is only used behind the service's handler lock.
unsafe impl Send for AllyDevice {}

impl AllyDevice {
    /// Returns `None` when not running on a supported Ally, or when the ACPI
    /// device cannot be opened (not elevated).
    pub fn open() -> Option<Self> {
        let model = Model::detect(&product_name()?)?;
        let path = wide(atk::DEVICE_PATH);
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let (adl, adl_error) = match Adl::open() {
            Ok(adl) => (Some(adl), None),
            Err(err) => (None, Some(err)),
        };
        Some(Self {
            handle,
            model,
            adl,
            adl_error,
            firmware_mode: None,
            controller_ready: false,
        })
    }

    fn call(&self, input: &[u8]) -> Result<[u8; atk::OUTPUT_LEN]> {
        let mut output = [0u8; atk::OUTPUT_LEN];
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                atk::IOCTL,
                input.as_ptr() as *const c_void,
                input.len() as u32,
                output.as_mut_ptr() as *mut c_void,
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(output)
    }

    fn set(&self, device_id: u32, value: u32) -> Result<()> {
        let output = self.call(&atk::set_buffer(device_id, value))?;
        if atk::set_succeeded(&output) {
            Ok(())
        } else {
            Err(HwError::Firmware(format!(
                "DEVS 0x{device_id:08X} = {value} returned {}",
                atk::result(&output)
            )))
        }
    }

    fn get(&self, device_id: u32) -> Result<Option<u32>> {
        Ok(atk::get_value(&self.call(&atk::get_buffer(device_id))?))
    }

    fn write_mode(&mut self, value: u32) -> Result<()> {
        self.set(atk::PERFORMANCE_MODE, value)?;
        self.firmware_mode = Some((value, Instant::now()));
        Ok(())
    }

    /// The mode last written, else balanced (the firmware's default).
    fn current_mode(&self) -> u32 {
        self.firmware_mode
            .map_or(atk::mode::BALANCED, |(value, _)| value)
    }

    fn fan_rpm(&self, fan: Fan) -> Option<u32> {
        let output = self.call(&atk::get_buffer(atk::fan_speed_id(fan))).ok()?;
        atk::fan_rpm(&output)
    }
}

fn mode_value(mode: PerformanceMode) -> u32 {
    match mode {
        PerformanceMode::Silent => atk::mode::SILENT,
        PerformanceMode::Performance => atk::mode::BALANCED,
        PerformanceMode::Turbo => atk::mode::TURBO,
        PerformanceMode::Manual => atk::mode::MANUAL,
    }
}

impl Drop for AllyDevice {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

impl Device for AllyDevice {
    fn name(&self) -> &str {
        self.model.name()
    }

    fn diagnostics(&self) -> Vec<String> {
        let mut lines = vec![match (&self.adl, &self.adl_error) {
            (Some(adl), _) => adl.describe(),
            (None, Some(err)) => format!("adl unavailable: {err}"),
            (None, None) => "adl unavailable".into(),
        }];
        if let Some(adl) = &self.adl {
            lines.push(adl.describe_fps_limit());
        }
        let present = |id| matches!(self.get(id), Ok(Some(_)));
        lines.push(format!(
            "atk: fan curves cpu {} gpu {} mid {}, fan speeds {:?} / {:?} rpm",
            present(atk::FAN_CURVE_CPU),
            present(atk::FAN_CURVE_GPU),
            present(atk::FAN_MID),
            self.fan_rpm(Fan::Cpu),
            self.fan_rpm(Fan::Gpu),
        ));
        lines
    }

    fn tdp_range(&self) -> (u32, u32) {
        self.model.tdp_range()
    }

    /// sPPT, then SPL, then fPPT.
    fn set_tdp(&mut self, watts: u32) -> Result<()> {
        let (min, max) = self.tdp_range();
        if !(min..=max).contains(&watts) {
            return Err(HwError::OutOfRange(format!(
                "{watts} W not in {min}..={max} W"
            )));
        }
        self.set(atk::PPT_SPPT, watts)?;
        self.set(atk::PPT_SPL, watts)?;
        if self.get(atk::PPT_FPPT)?.is_some() {
            self.set(atk::PPT_FPPT, watts)?;
        }
        Ok(())
    }

    fn set_performance_mode(&mut self, mode: PerformanceMode) -> Result<()> {
        self.write_mode(mode_value(mode))
    }

    fn set_battery_charge_limit(&mut self, percent: u8) -> Result<()> {
        let (min, max) = atk::BATTERY_LIMIT_RANGE;
        if !(min..=max).contains(&percent) {
            return Err(HwError::OutOfRange(format!(
                "{percent} % not in {min}..={max} %"
            )));
        }
        self.set(atk::BATTERY_LIMIT, percent.into())
    }

    fn telemetry(&mut self) -> Result<Telemetry> {
        let cpu_temp_c = self
            .get(atk::TEMP_CPU)?
            .filter(|t| (1..125).contains(t))
            .map(|t| t as f32);
        let (gpu_util, power_watts) = self.adl.as_ref().map_or((None, None), Adl::sensors);
        let mut power = SYSTEM_POWER_STATUS::default();
        let known = unsafe { GetSystemPowerStatus(&mut power) } != 0;
        let battery_percent =
            (known && power.BatteryLifePercent <= 100).then_some(power.BatteryLifePercent);
        // ACLineStatus: 0 offline, 1 online, 255 unknown.
        let on_battery = (known && power.ACLineStatus <= 1).then_some(power.ACLineStatus == 0);
        Ok(Telemetry {
            cpu_temp_c,
            gpu_util,
            power_watts,
            battery_percent,
            on_battery,
            cpu_fan_rpm: self.fan_rpm(Fan::Cpu),
            gpu_fan_rpm: self.fan_rpm(Fan::Gpu),
        })
    }

    fn frame_rate(&mut self) -> Option<f32> {
        self.adl.as_ref().and_then(Adl::fps)
    }

    /// back to the "fan range" IDs, and if that fails too it writes the mode
    /// range fallback is for laptops without curves; on the Ally we go
    /// straight to the reset.
    fn set_fan_curve(&mut self, fan: Fan, curve: &FanCurve) -> Result<()> {
        curve.validate().map_err(HwError::OutOfRange)?;
        if fan == Fan::Mid && self.get(atk::FAN_MID)?.is_none() {
            return Err(HwError::Unsupported("no mid fan on this model"));
        }
        if let Some((_, written)) = self.firmware_mode {
            let delay = Duration::from_millis(atk::FAN_CURVE_DELAY_MS);
            if let Some(wait) = delay.checked_sub(written.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        let output = self.call(&atk::fan_curve_buffer(fan, curve))?;
        if atk::set_succeeded(&output) {
            return Ok(());
        }
        let reset = self.write_mode(self.current_mode());
        Err(HwError::Firmware(format!(
            "DEVS 0x{:08X} (fan curve {curve:?}) returned {}; mode written again: {reset:?}",
            atk::fan_curve_id(fan),
            atk::result(&output)
        )))
    }

    /// firmware then also applies the mode's power limits.
    fn reset_fan_curves(&mut self) -> Result<()> {
        self.write_mode(self.current_mode())
    }

    fn default_fan_curve(&mut self, fan: Fan, mode: PerformanceMode) -> Result<Option<FanCurve>> {
        let output = self.call(&atk::fan_curve_read_buffer(fan, mode_value(mode)))?;
        Ok(FanCurve::from_bytes(&output))
    }

    fn set_fps_limit(&mut self, fps: Option<u32>) -> Result<()> {
        let adl = self
            .adl
            .as_ref()
            .ok_or(HwError::Unsupported("no AMD driver (ADL)"))?;
        adl.set_fps_limit(fps).map_err(HwError::Driver)
    }

    /// UNTESTED on the device. The init report once, then the mode command
    /// writes its button bindings after the mode; Canape keeps the
    /// firmware's.
    fn set_controller_mode(&mut self, mode: ControllerMode) -> Result<()> {
        let init = !self.controller_ready;
        hid::write_input(|len| {
            let mut reports = Vec::new();
            if init {
                reports.push(pad::init_report(len));
            }
            reports.push(pad::mode_report(mode, len));
            reports
        })?;
        self.controller_ready = true;
        Ok(())
    }
}
