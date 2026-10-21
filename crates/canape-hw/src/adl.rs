//! AMD Display Library (`atiadlxx.dll`, shipped with the GPU driver): frame

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

/// The ADL entry points Canape uses.
struct Api {
    control_create: ControlCreate,
    adapters_count: AdaptersCount,
    adapter_info: AdapterInfoGet,
    asic_family: AsicFamily,
    frame_metrics_start: FrameMetricsStart,
    frame_metrics_get: FrameMetricsGet,
    pmlog_get: PmLogGet,
}

/// ADL asks the caller to allocate the buffers it returns.
unsafe extern "system" fn adl_malloc(size: i32) -> *mut c_void {
    unsafe { LocalAlloc(LMEM_FIXED, size.max(0) as usize) }
}

fn load_api() -> Option<Api> {
    let name: Vec<u16> = "atiadlxx.dll".encode_utf16().chain(Some(0)).collect();
    let module: HMODULE = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return None;
    }
    macro_rules! symbol {
        ($name:literal as $ty:ty) => {
            // SAFETY: the signatures above match the ADL SDK headers.
            unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(GetProcAddress(
                    module,
                    concat!($name, "\0").as_ptr(),
                )?)
            }
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
    })
}

pub struct Adl {
    api: Api,
    context: Context,
    adapter: i32,
}

// ADL contexts are thread-safe; the service also serializes every call.
unsafe impl Send for Adl {}

impl Adl {
    /// The integrated AMD GPU, with frame metrics started; `None` without
    /// an AMD driver.
    pub fn open() -> Option<Self> {
        let api = load_api()?;
        let mut context: Context = std::ptr::null_mut();
        if unsafe { (api.control_create)(adl_malloc, 1, &mut context) } != ADL_OK {
            return None;
        }
        let mut count = 0;
        if unsafe { (api.adapters_count)(context, &mut count) } != ADL_OK || count <= 0 {
            return None;
        }
        let mut adapters: Vec<AdapterInfo> = vec![unsafe { std::mem::zeroed() }; count as usize];
        let bytes = (adapters.len() * size_of::<AdapterInfo>()) as i32;
        if unsafe { (api.adapter_info)(context, adapters.as_mut_ptr(), bytes) } != ADL_OK {
            return None;
        }
        let adapter = adapters
            .iter()
            .filter(|a| a.vendor_id == AMD_VENDOR_ID && a.present != 0 && a.exist != 0)
            .map(|a| a.adapter_index)
            .find(|&index| {
                let (mut family, mut valid) = (0, 0);
                let ok = unsafe { (api.asic_family)(context, index, &mut family, &mut valid) };
                ok == ADL_OK && family & valid & ASIC_INTEGRATED != 0
            })?;
        unsafe { (api.frame_metrics_start)(context, adapter, 0) };
        Some(Self {
            api,
            context,
            adapter,
        })
    }

    pub fn fps(&self) -> Option<f32> {
        let mut fps = 0.0;
        let ok = unsafe { (self.api.frame_metrics_get)(self.context, self.adapter, 0, &mut fps) };
        (ok == ADL_OK && fps.is_finite() && fps >= 0.0).then_some(fps)
    }

    /// GPU activity in `[0, 1]`.
    pub fn gpu_util(&self) -> Option<f32> {
        let mut log = PmLogDataOutput {
            size: size_of::<PmLogDataOutput>() as i32,
            sensors: [SensorData::default(); PMLOG_MAX_SENSORS],
        };
        if unsafe { (self.api.pmlog_get)(self.context, self.adapter, &mut log) } != ADL_OK {
            return None;
        }
        let sensor = log.sensors[PMLOG_INFO_ACTIVITY_GFX];
        (sensor.supported != 0).then(|| (sensor.value.clamp(0, 100) as f32) / 100.0)
    }
}
