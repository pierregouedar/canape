//! Feature reports to the Ally's controller over HID (Windows only), with
//! SetupAPI and `hid.dll`, and the input reports of its own buttons. Which
//! interface and which bytes: see `pad.rs`.
//!
//! UNTESTED on the device: checked only by `cargo check` for Windows.

use std::ffi::c_void;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS,
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
    HidD_SetFeature, HidP_Feature, HidP_GetButtonCaps, HidP_GetCaps, HidP_GetValueCaps,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows_sys::core::GUID;

use crate::pad;
use crate::{HwError, Result};

/// An open HID interface, closed on drop.
struct Interface {
    handle: HANDLE,
    /// Feature report length, report ID included.
    feature_len: usize,
    /// Input report length, report ID included.
    input_len: usize,
}

impl Drop for Interface {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

impl Interface {
    /// Opens `path` when it is the Ally controller's configuration
    /// interface: ASUS vendor, an Ally product ID, and a feature report
    /// with ID 0x5A in its descriptor.
    fn open(path: &[u16]) -> Option<Self> {
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
        let mut interface = Self {
            handle,
            feature_len: 0,
            input_len: 0,
        };
        let mut attributes: HIDD_ATTRIBUTES = unsafe { std::mem::zeroed() };
        attributes.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
        if !unsafe { HidD_GetAttributes(handle, &mut attributes) }
            || attributes.VendorID != pad::VENDOR_ID
            || !pad::PRODUCT_IDS.contains(&attributes.ProductID)
        {
            return None;
        }
        (interface.feature_len, interface.input_len) = report_lens(handle, pad::REPORT_ID)?;
        Some(interface)
    }

    fn set_feature(&self, report: &[u8]) -> Result<()> {
        let ok = unsafe {
            HidD_SetFeature(
                self.handle,
                report.as_ptr() as *const c_void,
                report.len() as u32,
            )
        };
        if ok {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }
}

/// The interface's feature and input report lengths when its descriptor
/// declares a feature report with `report_id` (value or button
/// capabilities).
fn report_lens(handle: HANDLE, report_id: u8) -> Option<(usize, usize)> {
    let mut data = 0;
    if !unsafe { HidD_GetPreparsedData(handle, &mut data) } {
        return None;
    }
    let found = (|| {
        let mut caps = HIDP_CAPS::default();
        if unsafe { HidP_GetCaps(data, &mut caps) } != HIDP_STATUS_SUCCESS
            || caps.FeatureReportByteLength == 0
        {
            return None;
        }
        let mut values = vec![HIDP_VALUE_CAPS::default(); caps.NumberFeatureValueCaps.into()];
        let mut count = values.len() as u16;
        let values_ok = count > 0
            && unsafe { HidP_GetValueCaps(HidP_Feature, values.as_mut_ptr(), &mut count, data) }
                == HIDP_STATUS_SUCCESS;
        let in_values = values_ok
            && values[..count as usize]
                .iter()
                .any(|v| v.ReportID == report_id);
        let mut buttons = vec![HIDP_BUTTON_CAPS::default(); caps.NumberFeatureButtonCaps.into()];
        let mut count = buttons.len() as u16;
        let buttons_ok = count > 0
            && unsafe { HidP_GetButtonCaps(HidP_Feature, buttons.as_mut_ptr(), &mut count, data) }
                == HIDP_STATUS_SUCCESS;
        let in_buttons = buttons_ok
            && buttons[..count as usize]
                .iter()
                .any(|b| b.ReportID == report_id);
        (in_values || in_buttons).then_some((
            caps.FeatureReportByteLength.into(),
            caps.InputReportByteLength.into(),
        ))
    })();
    unsafe { HidD_FreePreparsedData(data) };
    found
}

/// Device paths of every present HID interface.
fn interface_paths() -> Vec<Vec<u16>> {
    let mut guid: GUID = unsafe { std::mem::zeroed() };
    unsafe { HidD_GetHidGuid(&mut guid) };
    let set = unsafe {
        SetupDiGetClassDevsW(
            &guid,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if set == INVALID_HANDLE_VALUE as isize {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for index in 0.. {
        let mut data = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, index, &mut data) }
            == 0
        {
            break;
        }
        let mut size = 0u32;
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &data,
                std::ptr::null_mut(),
                0,
                &mut size,
                std::ptr::null_mut(),
            )
        };
        if (size as usize) < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() {
            continue;
        }
        // u32-aligned buffer for the variable-length detail struct.
        let mut buffer = vec![0u32; (size as usize).div_ceil(4)];
        let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        unsafe { (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
        let ok = unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &data,
                detail,
                size,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            continue;
        }
        // DevicePath starts after cbSize and runs to a NUL.
        let start = unsafe { std::ptr::addr_of!((*detail).DevicePath) as *const u16 };
        let max = (size as usize - 4) / 2;
        let chars = unsafe { std::slice::from_raw_parts(start, max) };
        let len = chars.iter().position(|&c| c == 0).unwrap_or(max);
        let mut path = chars[..len].to_vec();
        path.push(0);
        paths.push(path);
    }
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    paths
}

/// Sends `reports` (built for a given length by the closure) to every
/// matching controller interface. Returns how many interfaces took them.
pub fn write_input(reports: impl Fn(usize) -> Vec<Vec<u8>>) -> Result<usize> {
    let mut written = 0;
    let mut last_error = None;
    for path in interface_paths() {
        let Some(interface) = Interface::open(&path) else {
            continue;
        };
        let result = reports(interface.feature_len)
            .iter()
            .try_for_each(|report| interface.set_feature(report));
        match result {
            Ok(()) => written += 1,
            Err(err) => last_error = Some(err),
        }
    }
    match (written, last_error) {
        (0, Some(err)) => Err(err),
        (0, None) => Err(HwError::Unsupported(
            "no Ally controller HID interface (VID 0x0B05, PID 0x1ABE / 0x1B4C, report 0x5A)",
        )),
        (n, _) => Ok(n),
    }
}

/// Reads the Ally's own buttons until the device goes away or a read
/// fails, calling `on_button` for each press. Blocks: run it on its own
/// thread, and call it again after an error (the device comes back after
/// sleep).
pub fn listen(mut on_button: impl FnMut(pad::Button)) -> Result<()> {
    let interface = interface_paths()
        .iter()
        .find_map(|path| Interface::open(path))
        .filter(|i| i.input_len > 0)
        .ok_or(HwError::Unsupported(
            "no Ally controller HID interface with input reports",
        ))?;
    // The buttons only report once the interface has been woken.
    interface.set_feature(&pad::init_report(interface.feature_len))?;
    let mut report = vec![0u8; interface.input_len];
    loop {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                interface.handle,
                report.as_mut_ptr(),
                report.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if let Some(button) = pad::parse_input(&report[..read as usize]) {
            on_button(button);
        }
    }
}
