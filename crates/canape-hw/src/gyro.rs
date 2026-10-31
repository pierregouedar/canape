//! The gyroscope through the Windows sensor stack (Windows only):
//! `Windows.Devices.Sensors.Gyrometer`, called through its WinRT vtables so
//! that no projection crate is needed. Usable from a desktop process at
//! medium integrity, without package capabilities.
//!
//! Validated on the device: mouse aiming in a game, steady at rest; Windows
//! caps the reported rate at 125 degrees per second.

use std::ffi::c_void;

use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows_sys::core::{GUID, HRESULT, PCWSTR};

use crate::motion::{Acceleration, Accelerometer, AngularVelocity, Gyro};
use crate::{HwError, Result};

type Hstring = *mut c_void;

#[link(name = "combase.dll", kind = "raw-dylib", modifiers = "+verbatim")]
unsafe extern "system" {
    fn RoGetActivationFactory(
        class_id: Hstring,
        iid: *const GUID,
        factory: *mut *mut c_void,
    ) -> HRESULT;
    fn WindowsCreateString(source: PCWSTR, length: u32, string: *mut Hstring) -> HRESULT;
    fn WindowsDeleteString(string: Hstring) -> HRESULT;
}

const CLASS: &str = "Windows.Devices.Sensors.Gyrometer";
const IID_GYROMETER_STATICS: GUID = GUID::from_u128(0x83b6e7c9_e49d_4b39_86e6_cd554be4c5c1);
/// The accelerometer's class and statics have the same layouts as the
/// gyrometer's, down to the slots of the reading (`AccelerationX` follows
/// the timestamp like `AngularVelocityX` does).
const ACCEL_CLASS: &str = "Windows.Devices.Sensors.Accelerometer";
const IID_ACCELEROMETER_STATICS: GUID = GUID::from_u128(0xa5e28b74_5a87_4a2d_becc_0f906ea061dd);

// Vtable slots after IInspectable's six (QueryInterface, AddRef, Release,
// GetIids, GetRuntimeClassName, GetTrustLevel).
const RELEASE: usize = 2;
/// `IGyrometerStatics::GetDefault`.
const GET_DEFAULT: usize = 6;
/// `IGyrometer`.
const GET_CURRENT_READING: usize = 6;
const MINIMUM_REPORT_INTERVAL: usize = 7;
const SET_REPORT_INTERVAL: usize = 8;
/// `IGyrometerReading::AngularVelocityX`, then Y and Z.
const ANGULAR_VELOCITY_X: usize = 7;

/// Report interval asked for while aiming, in milliseconds, unless the
/// sensor's minimum is higher.
const ACTIVE_INTERVAL_MS: u32 = 10;

type GetObject = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
type GetU32 = unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT;
type SetU32 = unsafe extern "system" fn(*mut c_void, u32) -> HRESULT;
type GetF64 = unsafe extern "system" fn(*mut c_void, *mut f64) -> HRESULT;
type Release = unsafe extern "system" fn(*mut c_void) -> u32;

/// An owned WinRT interface pointer, released on drop.
struct Object(*mut c_void);

impl Object {
    /// The function in vtable slot `index`, typed by the caller.
    ///
    /// # Safety
    /// `F` must be the slot's exact signature.
    unsafe fn method<F: Copy>(&self, index: usize) -> F {
        unsafe {
            let vtable = *(self.0 as *const *const *const c_void);
            std::mem::transmute_copy::<*const c_void, F>(&*vtable.add(index))
        }
    }

    /// Calls a getter returning another object; `None` for a null one.
    fn get_object(&self, index: usize, what: &str) -> Result<Option<Object>> {
        let mut out = std::ptr::null_mut();
        let hr = unsafe { self.method::<GetObject>(index)(self.0, &mut out) };
        check(hr, what)?;
        Ok((!out.is_null()).then_some(Object(out)))
    }

    fn get_u32(&self, index: usize, what: &str) -> Result<u32> {
        let mut out = 0;
        check(
            unsafe { self.method::<GetU32>(index)(self.0, &mut out) },
            what,
        )?;
        Ok(out)
    }

    fn set_u32(&self, index: usize, value: u32, what: &str) -> Result<()> {
        check(unsafe { self.method::<SetU32>(index)(self.0, value) }, what)
    }

    fn get_f64(&self, index: usize, what: &str) -> Result<f64> {
        let mut out = 0.0;
        check(
            unsafe { self.method::<GetF64>(index)(self.0, &mut out) },
            what,
        )?;
        Ok(out)
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        unsafe { self.method::<Release>(RELEASE)(self.0) };
    }
}

fn check(hr: HRESULT, what: &str) -> Result<()> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(HwError::Io(std::io::Error::other(format!(
            "{what} returned 0x{:08X}",
            hr as u32
        ))))
    }
}

/// The default gyrometer of the machine.
pub struct WindowsGyro {
    sensor: Object,
    minimum_ms: u32,
}

// The sensor objects are agile (free-threaded) WinRT objects, created in
// the multithreaded apartment.
unsafe impl Send for WindowsGyro {}

impl WindowsGyro {
    /// Joins the multithreaded apartment on the calling thread (if it has
    /// none yet) and opens the default gyrometer.
    pub fn open() -> Result<Self> {
        // S_FALSE (already joined) and RPC_E_CHANGED_MODE (a single-threaded
        // apartment) both leave a usable apartment.
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let statics = activation_factory(CLASS, &IID_GYROMETER_STATICS)?;
        let sensor = statics
            .get_object(GET_DEFAULT, "Gyrometer.GetDefault")?
            .ok_or(HwError::Unsupported("no gyroscope in the Windows sensors"))?;
        let minimum_ms = sensor
            .get_u32(MINIMUM_REPORT_INTERVAL, "Gyrometer.MinimumReportInterval")
            .unwrap_or(0);
        Ok(Self { sensor, minimum_ms })
    }
}

impl Gyro for WindowsGyro {
    fn describe(&self) -> String {
        format!(
            "gyro: Windows gyrometer, minimum report interval {} ms",
            self.minimum_ms
        )
    }

    fn set_active(&mut self, active: bool) {
        // 0 asks for the sensor's default rate.
        let interval = if active {
            ACTIVE_INTERVAL_MS.max(self.minimum_ms)
        } else {
            0
        };
        let _ = self
            .sensor
            .set_u32(SET_REPORT_INTERVAL, interval, "Gyrometer.ReportInterval");
    }

    fn read(&mut self) -> Result<Option<AngularVelocity>> {
        let Some(reading) = self
            .sensor
            .get_object(GET_CURRENT_READING, "Gyrometer.GetCurrentReading")?
        else {
            return Ok(None);
        };
        let axis = |i: usize| {
            reading
                .get_f64(ANGULAR_VELOCITY_X + i, "GyrometerReading.AngularVelocity")
                .map(|v| v as f32)
        };
        Ok(Some(AngularVelocity {
            x: axis(0)?,
            y: axis(1)?,
            z: axis(2)?,
        }))
    }
}

/// The default accelerometer of the machine, in g (gravity included).
pub struct WindowsAccelerometer {
    sensor: Object,
    minimum_ms: u32,
}

unsafe impl Send for WindowsAccelerometer {}

impl WindowsAccelerometer {
    pub fn open() -> Result<Self> {
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        let statics = activation_factory(ACCEL_CLASS, &IID_ACCELEROMETER_STATICS)?;
        let sensor = statics
            .get_object(GET_DEFAULT, "Accelerometer.GetDefault")?
            .ok_or(HwError::Unsupported(
                "no accelerometer in the Windows sensors",
            ))?;
        let minimum_ms = sensor
            .get_u32(
                MINIMUM_REPORT_INTERVAL,
                "Accelerometer.MinimumReportInterval",
            )
            .unwrap_or(0);
        Ok(Self { sensor, minimum_ms })
    }
}

impl Accelerometer for WindowsAccelerometer {
    fn describe(&self) -> String {
        format!(
            "motion: Windows accelerometer, minimum report interval {} ms",
            self.minimum_ms
        )
    }

    fn set_active(&mut self, active: bool) {
        let interval = if active {
            ACTIVE_INTERVAL_MS.max(self.minimum_ms)
        } else {
            0
        };
        let _ = self.sensor.set_u32(
            SET_REPORT_INTERVAL,
            interval,
            "Accelerometer.ReportInterval",
        );
    }

    fn read(&mut self) -> Result<Option<Acceleration>> {
        let Some(reading) = self
            .sensor
            .get_object(GET_CURRENT_READING, "Accelerometer.GetCurrentReading")?
        else {
            return Ok(None);
        };
        let axis = |i: usize| {
            reading
                .get_f64(ANGULAR_VELOCITY_X + i, "AccelerometerReading.Acceleration")
                .map(|v| v as f32)
        };
        Ok(Some(Acceleration {
            x: axis(0)?,
            y: axis(1)?,
            z: axis(2)?,
        }))
    }
}

fn activation_factory(class: &str, iid: &GUID) -> Result<Object> {
    let wide: Vec<u16> = class.encode_utf16().collect();
    let mut name = std::ptr::null_mut();
    check(
        unsafe { WindowsCreateString(wide.as_ptr(), wide.len() as u32, &mut name) },
        "WindowsCreateString",
    )?;
    let mut factory = std::ptr::null_mut();
    let hr = unsafe { RoGetActivationFactory(name, iid, &mut factory) };
    unsafe { WindowsDeleteString(name) };
    check(hr, "RoGetActivationFactory")?;
    if factory.is_null() {
        return Err(HwError::Unsupported("Windows sensors"));
    }
    Ok(Object(factory))
}
