//! AMD Display Library (`atiadlxx.dll`, shipped with the GPU driver): frame
//! rate and GPU activity of the integrated GPU. Loaded at runtime: no
//! driver, no telemetry.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Memory::{LMEM_FIXED, LocalAlloc};

const ADL_OK: i32 = 0;
const AMD_VENDOR_ID: i32 = 1002;
const ASIC_INTEGRATED: i32 = 1 << 1;
const MAX_PATH: usize = 256;
const PMLOG_MAX_SENSORS: usize = 256;
const PMLOG_INFO_ACTIVITY_GFX: usize = 19;
/// Whole-APU power draw, in watts.
const PMLOG_ASIC_POWER: usize = 23;

/// `AdapterInfo` from the ADL SDK (ANSI strings).
#[repr(C)]
#[derive(Clone, Copy)]
struct AdapterInfo {
    size: i32,
    adapter_index: i32,
    udid: [u8; MAX_PATH],
    bus_number: i32,
    driver_number: i32,
    function_number: i32,
    vendor_id: i32,
    adapter_name: [u8; MAX_PATH],
    display_name: [u8; MAX_PATH],
    present: i32,
    exist: i32,
    driver_path: [u8; MAX_PATH],
    driver_path_ext: [u8; MAX_PATH],
    pnp_string: [u8; MAX_PATH],
    os_display_index: i32,
}

// Sizes from the ADL SDK headers: a mismatch would corrupt memory.
const _: () = assert!(size_of::<AdapterInfo>() == 1572);
const _: () = assert!(size_of::<PmLogDataOutput>() == 4 + 8 * PMLOG_MAX_SENSORS);
const _: () = assert!(size_of::<FpsSettingsOutput>() == 36);
const _: () = assert!(size_of::<FpsSettingsInput>() == 40);

/// Written to turn the limiter "off" when the driver does not report its
/// range: the top of the range the Ally's driver reports (15..=1000), which
/// is also the value it ships with.
pub const FPS_LIMIT_OFF: u32 = 1000;

/// `ADLFPSSettingsOutput` from the ADL SDK.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FpsSettingsOutput {
    size: i32,
    ac_enabled: i32,
    dc_enabled: i32,
    ac_current: i32,
    dc_current: i32,
    ac_maximum: i32,
    ac_minimum: i32,
    dc_maximum: i32,
    dc_minimum: i32,
}

/// `ADLFPSSettingsInput` from the ADL SDK.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FpsSettingsInput {
    size: i32,
    global_settings: i32,
    ac_current: i32,
    dc_current: i32,
    reserved: [i32; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SensorData {
    supported: i32,
    value: i32,
}

#[repr(C)]
struct PmLogDataOutput {
    size: i32,
    sensors: [SensorData; PMLOG_MAX_SENSORS],
}

type Context = *mut c_void;
type MallocCallback = unsafe extern "system" fn(i32) -> *mut c_void;

type ControlCreate = unsafe extern "C" fn(MallocCallback, i32, *mut Context) -> i32;
type AdaptersCount = unsafe extern "C" fn(Context, *mut i32) -> i32;
type AdapterInfoGet = unsafe extern "C" fn(Context, *mut AdapterInfo, i32) -> i32;
type AsicFamily = unsafe extern "C" fn(Context, i32, *mut i32, *mut i32) -> i32;
type FrameMetricsStart = unsafe extern "C" fn(Context, i32, i32) -> i32;
type FrameMetricsGet = unsafe extern "C" fn(Context, i32, i32, *mut f32) -> i32;
type PmLogGet = unsafe extern "C" fn(Context, i32, *mut PmLogDataOutput) -> i32;
type FpsSettingsGet = unsafe extern "C" fn(Context, i32, *mut FpsSettingsOutput) -> i32;
// The ADL SDK passes the input struct by value; on x64 that is a pointer
// to a copy, which Rust's C ABI produces the same way.
type FpsSettingsSet = unsafe extern "C" fn(Context, i32, FpsSettingsInput) -> i32;

/// The ADL entry points Canape uses.
struct Api {
    control_create: ControlCreate,
    adapters_count: AdaptersCount,
    adapter_info: AdapterInfoGet,
    asic_family: AsicFamily,
    frame_metrics_start: FrameMetricsStart,
    frame_metrics_get: FrameMetricsGet,
    pmlog_get: PmLogGet,
    /// Optional: an older driver without them still gives FPS and activity.
    fps_settings_get: Option<FpsSettingsGet>,
    fps_settings_set: Option<FpsSettingsSet>,
}

/// ADL asks the caller to allocate the buffers it returns.
unsafe extern "system" fn adl_malloc(size: i32) -> *mut c_void {
    unsafe { LocalAlloc(LMEM_FIXED, size.max(0) as usize) }
}

/// A NUL-terminated ANSI field of an ADL struct.
fn ansi(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn load_api() -> Option<Api> {
    let name: Vec<u16> = "atiadlxx.dll".encode_utf16().chain(Some(0)).collect();
    let module: HMODULE = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return None;
    }
    macro_rules! optional {
        ($name:literal as $ty:ty) => {
            // SAFETY: the signatures above match the ADL SDK headers.
            unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) }.map(|f| unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(f)
            })
        };
    }
    macro_rules! symbol {
        ($name:literal as $ty:ty) => {
            optional!($name as $ty)?
        };
    }
    Some(Api {
        control_create: symbol!("ADL2_Main_Control_Create" as ControlCreate),
        adapters_count: symbol!("ADL2_Adapter_NumberOfAdapters_Get" as AdaptersCount),
        adapter_info: symbol!("ADL2_Adapter_AdapterInfo_Get" as AdapterInfoGet),
        asic_family: symbol!("ADL2_Adapter_ASICFamilyType_Get" as AsicFamily),
        frame_metrics_start: symbol!("ADL2_Adapter_FrameMetrics_Start" as FrameMetricsStart),
        frame_metrics_get: symbol!("ADL2_Adapter_FrameMetrics_Get" as FrameMetricsGet),
        pmlog_get: symbol!("ADL2_New_QueryPMLogData_Get" as PmLogGet),
        fps_settings_get: optional!("ADL2_FPS_Settings_Get" as FpsSettingsGet),
        fps_settings_set: optional!("ADL2_FPS_Settings_Set" as FpsSettingsSet),
    })
}

pub struct Adl {
    api: Api,
    context: Context,
    adapter: i32,
    /// `ADL2_Adapter_FrameMetrics_Start` result; FPS only when `ADL_OK`.
    frame_metrics: i32,
}

// ADL contexts are thread-safe; the service also serializes every call.
unsafe impl Send for Adl {}

impl Adl {
    /// The integrated AMD GPU, with frame metrics started. The error names
    /// the step that failed, for the service log.
    pub fn open() -> Result<Self, String> {
        let api = load_api().ok_or("atiadlxx.dll or one of its entry points not found")?;
        let mut context: Context = std::ptr::null_mut();
        let code = unsafe { (api.control_create)(adl_malloc, 1, &mut context) };
        if code != ADL_OK {
            return Err(format!("ADL2_Main_Control_Create returned {code}"));
        }
        let mut count = 0;
        let code = unsafe { (api.adapters_count)(context, &mut count) };
        if code != ADL_OK || count <= 0 {
            return Err(format!(
                "ADL2_Adapter_NumberOfAdapters_Get returned {code}, count {count}"
            ));
        }
        let mut adapters: Vec<AdapterInfo> = vec![unsafe { std::mem::zeroed() }; count as usize];
        let bytes = (adapters.len() * size_of::<AdapterInfo>()) as i32;
        let code = unsafe { (api.adapter_info)(context, adapters.as_mut_ptr(), bytes) };
        if code != ADL_OK {
            return Err(format!("ADL2_Adapter_AdapterInfo_Get returned {code}"));
        }
        let integrated = |a: &&AdapterInfo| {
            let (mut family, mut valid) = (0, 0);
            let ok =
                unsafe { (api.asic_family)(context, a.adapter_index, &mut family, &mut valid) };
            a.vendor_id == AMD_VENDOR_ID
                && a.exist != 0
                && ok == ADL_OK
                && family & valid & ASIC_INTEGRATED != 0
        };
        // In session 0 (the service) no adapter is `present`: there is no
        // display there. Prefer a present one, else take the first that exists.
        let adapter = adapters
            .iter()
            .filter(integrated)
            .max_by_key(|a| (a.present != 0, std::cmp::Reverse(a.adapter_index)))
            .map(|a| a.adapter_index)
            .ok_or_else(|| {
                let seen: Vec<String> = adapters
                    .iter()
                    .map(|a| {
                        let (mut family, mut valid) = (0, 0);
                        let code = unsafe {
                            (api.asic_family)(context, a.adapter_index, &mut family, &mut valid)
                        };
                        format!(
                            "#{} {:?} vendor {} present {} exist {} asic {code}:{family:#x}/{valid:#x}",
                            a.adapter_index,
                            ansi(&a.adapter_name),
                            a.vendor_id,
                            a.present,
                            a.exist
                        )
                    })
                    .collect();
                format!(
                    "no present integrated AMD adapter among {count}: {}",
                    seen.join("; ")
                )
            })?;
        // Display source 0. Fails with -1 from
        // session 0 (the service): GPU activity still works there.
        let frame_metrics = unsafe { (api.frame_metrics_start)(context, adapter, 0) };
        Ok(Self {
            api,
            context,
            adapter,
            frame_metrics,
        })
    }

    /// One line for the service log.
    pub fn describe(&self) -> String {
        if self.frame_metrics == ADL_OK {
            format!("adl: frame metrics started on adapter {}", self.adapter)
        } else {
            format!(
                "adl: adapter {}, ADL2_Adapter_FrameMetrics_Start returned {} (no FPS)",
                self.adapter, self.frame_metrics
            )
        }
    }

    /// Reads even when our own `FrameMetrics_Start` failed: once a process
    /// in the user session has started frame metrics (the shell does at
    /// startup), the driver reports them to every caller (docs/m4-report.md).
    pub fn fps(&self) -> Option<f32> {
        let mut fps = 0.0;
        let ok = unsafe { (self.api.frame_metrics_get)(self.context, self.adapter, 0, &mut fps) };
        (ok == ADL_OK && fps.is_finite() && fps >= 0.0).then_some(fps)
    }

    /// GPU activity in `[0, 1]` and APU power draw in watts, from one
    /// PMLog read.
    pub fn sensors(&self) -> (Option<f32>, Option<f32>) {
        let mut log = PmLogDataOutput {
            size: size_of::<PmLogDataOutput>() as i32,
            sensors: [SensorData::default(); PMLOG_MAX_SENSORS],
        };
        if unsafe { (self.api.pmlog_get)(self.context, self.adapter, &mut log) } != ADL_OK {
            return (None, None);
        }
        let read = |index: usize| {
            let sensor = log.sensors[index];
            (sensor.supported != 0).then_some(sensor.value)
        };
        let gpu = read(PMLOG_INFO_ACTIVITY_GFX).map(|v| (v.clamp(0, 100) as f32) / 100.0);
        let power = read(PMLOG_ASIC_POWER).filter(|&w| w > 0).map(|w| w as f32);
        (gpu, power)
    }

    /// The driver's frame rate limiter, as one log line.
    pub fn describe_fps_limit(&self) -> String {
        let Some(get) = self.api.fps_settings_get else {
            return "adl: no ADL2_FPS_Settings_Get in this driver".into();
        };
        // Zeroed, `ulSize` included.
        let mut settings = FpsSettingsOutput::default();
        let code = unsafe { get(self.context, self.adapter, &mut settings) };
        if code != ADL_OK {
            return format!("adl: ADL2_FPS_Settings_Get returned {code}");
        }
        format!(
            "adl: FPS limit AC {} (enabled {}, range {}..={}), DC {} (enabled {})",
            settings.ac_current,
            settings.ac_enabled,
            settings.ac_minimum,
            settings.ac_maximum,
            settings.dc_current,
            settings.dc_enabled
        )
    }

    /// The top of the driver's limiter range: writing it lifts the cap.
    /// A lower "high" value such as 240 did not lift it on the Ally: the
    /// game stayed near 26 FPS until 1000 was written (docs/m5-report.md;
    /// the game also dropped on its own, so this is to be measured again).
    fn fps_limit_off(&self) -> i32 {
        let Some(get) = self.api.fps_settings_get else {
            return FPS_LIMIT_OFF as i32;
        };
        let mut settings = FpsSettingsOutput::default();
        let code = unsafe { get(self.context, self.adapter, &mut settings) };
        if code == ADL_OK && settings.ac_maximum > 0 {
            settings.ac_maximum
        } else {
            FPS_LIMIT_OFF as i32
        }
    }

    /// Caps the frame rate on AC and battery, for every game ("global"
    /// flag); `None` writes the top of the driver's range (see
    /// [`Self::fps_limit_off`]). `ulSize` and the reserved fields stay at 0,
    /// the form of the call known to work. Works from the service: on the
    /// Ally a cap of 40 written from session 0 held a game at 40.0 FPS.
    /// The error carries the ADL code for the log.
    pub fn set_fps_limit(&self, fps: Option<u32>) -> Result<(), String> {
        let set = self
            .api
            .fps_settings_set
            .ok_or("no ADL2_FPS_Settings_Set in this driver")?;
        let fps = fps.map_or_else(|| self.fps_limit_off(), |fps| fps as i32);
        let settings = FpsSettingsInput {
            global_settings: 1,
            ac_current: fps,
            dc_current: fps,
            ..FpsSettingsInput::default()
        };
        match unsafe { set(self.context, self.adapter, settings) } {
            ADL_OK => Ok(()),
            code => Err(format!(
                "ADL2_FPS_Settings_Set({fps}) on adapter {} returned {code} (session 0)",
                self.adapter
            )),
        }
    }
}
