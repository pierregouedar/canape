//! ROG Ally backend (Windows only), on top of the ATK ACPI device.
//!
//! Needs the service's LocalSystem rights to open `\\.\ATKACPI`. Armoury
//! one of them should run.

use std::ffi::c_void;

use canape_core::PerformanceMode;
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
use crate::{Device, HwError, Result, Telemetry};

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
        Some(Self {
            handle,
            model,
            adl: Adl::open(),
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
        let value = match mode {
            PerformanceMode::Silent => atk::mode::SILENT,
            PerformanceMode::Performance => atk::mode::BALANCED,
            PerformanceMode::Turbo => atk::mode::TURBO,
            PerformanceMode::Manual => atk::mode::MANUAL,
        };
        self.set(atk::PERFORMANCE_MODE, value)
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
        let mut power = SYSTEM_POWER_STATUS::default();
        let battery_percent = (unsafe { GetSystemPowerStatus(&mut power) } != 0
            && power.BatteryLifePercent <= 100)
            .then_some(power.BatteryLifePercent);
        Ok(Telemetry {
            cpu_temp_c,
            gpu_util: self.adl.as_ref().and_then(Adl::gpu_util),
            battery_percent,
        })
    }

    fn frame_rate(&mut self) -> Option<f32> {
        self.adl.as_ref().and_then(Adl::fps)
    }
}
