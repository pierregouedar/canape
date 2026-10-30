//! The AMD driver's 3D settings that games can ask for: Radeon Super
//! Resolution (upscaling, with its sharpness), Fluid Motion Frames (frame
//! generation), Anti-Lag, Chill and Image Sharpening. Windows only, through the driver's `amdadlx64.dll`, whose
//! objects are called through their vtables so that no binding crate is
//! needed. Nothing here writes anything the user did not choose, and
//! [`Radeon::snapshot`] / [`Radeon::restore`] put the driver back as it was.
//!
//! Called from the user session (the shell): the driver refuses 3D settings
//! to a service in session 0.
//!
//! Tried on the device: RSR and AFMF switch on and the driver is given back.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::{HwError, Result};

const DLL: &str = "amdadlx64.dll";
/// The interface version the layouts below were written for (2.0.0, build
/// 125): `major << 48 | minor << 32 | release << 16 | build`.
const VERSION: u64 = (2 << 48) | 125;

/// `ADLX_OK`, `ADLX_ALREADY_ENABLED` and `ADLX_ALREADY_INITIALIZED`.
fn succeeded(result: i32) -> bool {
    matches!(result, 0..=2)
}

/// An interface pointer; every ADLX interface is a pointer to a pointer to
/// its table of functions.
#[derive(Clone, Copy)]
struct Interface(*mut c_void);

// Slots of the tables, counted from the first function. Interfaces other
// than the system start with `Acquire`, `Release` and `QueryInterface`.
const RELEASE: usize = 1;
const QUERY_INTERFACE: usize = 2;
/// `IADLXSystem` has no `Acquire`: `GetHybridGraphicsType`, `GetGPUs`,
/// `QueryInterface`, `GetDisplaysServices`, `GetDesktopsServices`,
/// `GetGPUsChangedHandling`, `EnableLog`, then:
const SYSTEM_GET_GPUS: usize = 1;
const SYSTEM_GET_3D_SETTINGS_SERVICES: usize = 7;
/// `IADLXGPUList`: `Acquire`, `Release`, `QueryInterface`, `Size`, `Empty`,
/// `Begin`, `End`, `At`, `Clear`, `Remove_Back`, `Add_Back`, then:
const GPU_LIST_AT: usize = 11;
/// `IADLX3DSettingsServices`: after the three common functions, `GetAntiLag`,
/// `GetChill`, `GetBoost`, `GetImageSharpening`, `GetEnhancedSync`,
/// `GetWaitForVerticalRefresh`, `GetFrameRateTargetControl`,
/// `GetAntiAliasing`, `GetMorphologicalAntiAliasing`,
/// `GetAnisotropicFiltering`, `GetTessellation`, then:
const SERVICES_GET_ANTI_LAG: usize = 3;
const SERVICES_GET_CHILL: usize = 4;
const SERVICES_GET_IMAGE_SHARPENING: usize = 6;
const SERVICES_GET_RADEON_SUPER_RESOLUTION: usize = 14;
/// `IADLX3DSettingsServices1` adds one function after the last of the
/// previous interface (`GetResetShaderCache`,
/// `Get3DSettingsChangedHandling` follow `GetRadeonSuperResolution`).
const SERVICES1_GET_FLUID_MOTION_FRAMES: usize = 17;
/// The three features share the first slots after the common ones.
const IS_SUPPORTED: usize = 3;
const IS_ENABLED: usize = 4;
const SET_ENABLED: usize = 5;
/// Radeon Super Resolution only.
const GET_SHARPNESS_RANGE: usize = 6;
const GET_SHARPNESS: usize = 7;
const SET_SHARPNESS: usize = 8;
/// `IADLX3DImageSharpening`: `IsSupported`, `IsEnabled`, `GetSharpnessRange`,
/// `GetSharpness`, `SetEnabled`, `SetSharpness`.
const SHARPENING_GET_RANGE: usize = 5;
const SHARPENING_GET: usize = 6;
const SHARPENING_SET_ENABLED: usize = 7;
const SHARPENING_SET: usize = 8;
/// `IADLX3DChill`: `IsSupported`, `IsEnabled`, `GetFPSRange`, `GetMinFPS`,
/// `GetMaxFPS`, `SetEnabled`, `SetMinFPS`, `SetMaxFPS`.
const CHILL_GET_RANGE: usize = 5;
const CHILL_GET_MIN: usize = 6;
const CHILL_GET_MAX: usize = 7;
const CHILL_SET_ENABLED: usize = 8;
const CHILL_SET_MIN: usize = 9;
const CHILL_SET_MAX: usize = 10;

type Call0 = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
type CallBool = unsafe extern "system" fn(*mut c_void, *mut u8) -> i32;
type CallSetBool = unsafe extern "system" fn(*mut c_void, u8) -> i32;
type CallInt = unsafe extern "system" fn(*mut c_void, *mut i32) -> i32;
type CallSetInt = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type CallRange = unsafe extern "system" fn(*mut c_void, *mut [i32; 3]) -> i32;
type CallAt = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;
type CallGpu = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> i32;
type CallQuery = unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> i32;
type CallRelease = unsafe extern "system" fn(*mut c_void) -> i32;

impl Interface {
    /// The function in slot `index`, typed by the caller.
    ///
    /// # Safety
    /// `F` must be the slot's exact signature.
    unsafe fn method<F: Copy>(self, index: usize) -> F {
        unsafe {
            let table = *(self.0 as *const *const *const c_void);
            std::mem::transmute_copy::<*const c_void, F>(&*table.add(index))
        }
    }

    /// An interface returned through an out parameter by slot `index`.
    fn get(self, index: usize, what: &str) -> Result<Interface> {
        let mut out = std::ptr::null_mut();
        let result = unsafe { self.method::<Call0>(index)(self.0, &mut out) };
        check(result, what)?;
        if out.is_null() {
            return Err(HwError::Unsupported("AMD driver setting"));
        }
        Ok(Interface(out))
    }

    /// An interface of a service that needs the GPU, by slot `index`.
    fn get_for(self, index: usize, gpu: Interface, what: &str) -> Result<Interface> {
        let mut out = std::ptr::null_mut();
        let result = unsafe { self.method::<CallGpu>(index)(self.0, gpu.0, &mut out) };
        check(result, what)?;
        if out.is_null() {
            return Err(HwError::Unsupported("AMD driver setting"));
        }
        Ok(Interface(out))
    }

    fn int(self, index: usize, what: &str) -> Result<i32> {
        let mut out = 0i32;
        check(
            unsafe { self.method::<CallInt>(index)(self.0, &mut out) },
            what,
        )?;
        Ok(out)
    }

    fn set_int(self, index: usize, value: i32, what: &str) -> Result<()> {
        check(
            unsafe { self.method::<CallSetInt>(index)(self.0, value) },
            what,
        )
    }

    fn range(self, index: usize, what: &str) -> Result<[i32; 3]> {
        let mut out = [0i32; 3];
        check(
            unsafe { self.method::<CallRange>(index)(self.0, &mut out) },
            what,
        )?;
        Ok(out)
    }

    fn query(self, name: &str) -> Result<Interface> {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut out = std::ptr::null_mut();
        let result =
            unsafe { self.method::<CallQuery>(QUERY_INTERFACE)(self.0, wide.as_ptr(), &mut out) };
        check(result, name)?;
        if out.is_null() {
            return Err(HwError::Unsupported("AMD driver setting"));
        }
        Ok(Interface(out))
    }

    fn flag(self, index: usize, what: &str) -> Result<bool> {
        let mut out = 0u8;
        check(
            unsafe { self.method::<CallBool>(index)(self.0, &mut out) },
            what,
        )?;
        Ok(out != 0)
    }

    fn set_flag(self, index: usize, on: bool, what: &str) -> Result<()> {
        check(
            unsafe { self.method::<CallSetBool>(index)(self.0, on as u8) },
            what,
        )
    }

    fn release(self) {
        unsafe { self.method::<CallRelease>(RELEASE)(self.0) };
    }
}

fn check(result: i32, what: &str) -> Result<()> {
    if succeeded(result) {
        Ok(())
    } else {
        Err(HwError::Io(std::io::Error::other(format!(
            "{what} returned {result}"
        ))))
    }
}

/// What a feature was set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub enabled: bool,
    /// Radeon Super Resolution only.
    pub sharpness: Option<i32>,
}

/// Chill as it is, or as it is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChillSetting {
    pub enabled: bool,
    pub min: i32,
    pub max: i32,
}

/// The features as the driver had them; one that cannot be read is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub rsr: Option<Setting>,
    pub afmf: Option<Setting>,
    pub anti_lag: Option<bool>,
    pub chill: Option<ChillSetting>,
    pub sharpening: Option<Setting>,
}

/// The driver's 3D settings.
pub struct Radeon {
    module: HMODULE,
    terminate: unsafe extern "system" fn() -> i32,
    system: Interface,
    /// The GPU list and its first GPU, the one 3D settings are asked for;
    /// `None` when the driver gives none (then only Super Resolution and
    /// Fluid Motion Frames, which need none, are available).
    gpus: Option<(Interface, Interface)>,
    services: Interface,
    /// `None` on a driver older than the interface that added frame
    /// generation.
    services1: Option<Interface>,
}

// The objects are the driver's and are used from one thread at a time.
unsafe impl Send for Radeon {}

impl Radeon {
    /// Loads the driver's module and opens its 3D settings. An error when
    /// there is no AMD driver or it is too old.
    pub fn open() -> Result<Self> {
        let wide: Vec<u16> = DLL.encode_utf16().chain(Some(0)).collect();
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(HwError::Unsupported("AMD driver (amdadlx64.dll)"));
        }
        macro_rules! symbol {
            ($name:literal, $type:ty) => {
                unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) }
                    .map(|f| unsafe { std::mem::transmute::<_, $type>(f) })
                    .ok_or(HwError::Unsupported(concat!("AMD driver: ", $name)))?
            };
        }
        let initialize = symbol!(
            "ADLXInitialize",
            unsafe extern "system" fn(u64, *mut *mut c_void) -> i32
        );
        let terminate = symbol!("ADLXTerminate", unsafe extern "system" fn() -> i32);

        let mut system = std::ptr::null_mut();
        check(
            unsafe { initialize(VERSION, &mut system) },
            "ADLXInitialize",
        )?;
        if system.is_null() {
            return Err(HwError::Unsupported("AMD driver"));
        }
        let system = Interface(system);
        let services = system.get(SYSTEM_GET_3D_SETTINGS_SERVICES, "Get3DSettingsServices")?;
        let services1 = services.query("IADLX3DSettingsServices1").ok();
        let gpus = system
            .get(SYSTEM_GET_GPUS, "GetGPUs")
            .ok()
            .and_then(|list| {
                let mut gpu = std::ptr::null_mut();
                let result = unsafe { list.method::<CallAt>(GPU_LIST_AT)(list.0, 0, &mut gpu) };
                if succeeded(result) && !gpu.is_null() {
                    Some((list, Interface(gpu)))
                } else {
                    list.release();
                    None
                }
            });
        Ok(Self {
            module,
            terminate,
            system,
            gpus,
            services,
            services1,
        })
    }

    fn rsr_interface(&self) -> Result<Interface> {
        let rsr = self.services.get(
            SERVICES_GET_RADEON_SUPER_RESOLUTION,
            "GetRadeonSuperResolution",
        )?;
        if !rsr.flag(IS_SUPPORTED, "RSR IsSupported").unwrap_or(false) {
            rsr.release();
            return Err(HwError::Unsupported("Radeon Super Resolution"));
        }
        Ok(rsr)
    }

    fn afmf_interface(&self) -> Result<Interface> {
        let services1 = self.services1.ok_or(HwError::Unsupported(
            "AMD Fluid Motion Frames (driver too old)",
        ))?;
        let afmf = services1.get(SERVICES1_GET_FLUID_MOTION_FRAMES, "GetAMDFluidMotionFrames")?;
        if !afmf.flag(IS_SUPPORTED, "AFMF IsSupported").unwrap_or(false) {
            afmf.release();
            return Err(HwError::Unsupported("AMD Fluid Motion Frames"));
        }
        Ok(afmf)
    }

    /// Radeon Super Resolution as it is now.
    pub fn rsr(&self) -> Result<Setting> {
        let rsr = self.rsr_interface()?;
        let result = (|| {
            let enabled = rsr.flag(IS_ENABLED, "RSR IsEnabled")?;
            let mut sharpness = 0i32;
            let got = unsafe { rsr.method::<CallInt>(GET_SHARPNESS)(rsr.0, &mut sharpness) };
            Ok(Setting {
                enabled,
                sharpness: succeeded(got).then_some(sharpness),
            })
        })();
        rsr.release();
        result
    }

    /// Turns Radeon Super Resolution on or off; when it is on and
    /// `sharpness` is given, sets it within the range the driver accepts.
    pub fn set_rsr(&self, enabled: bool, sharpness: Option<i32>) -> Result<()> {
        let rsr = self.rsr_interface()?;
        let result = (|| {
            // Turned on first: the driver refuses the values of a feature
            // that is off.
            rsr.set_flag(SET_ENABLED, enabled, "RSR SetEnabled")?;
            if enabled && let Some(wanted) = sharpness {
                let mut range = [0i32; 3];
                let got =
                    unsafe { rsr.method::<CallRange>(GET_SHARPNESS_RANGE)(rsr.0, &mut range) };
                let wanted = if succeeded(got) && range[0] <= range[1] {
                    wanted.clamp(range[0], range[1])
                } else {
                    wanted
                };
                check(
                    unsafe { rsr.method::<CallSetInt>(SET_SHARPNESS)(rsr.0, wanted) },
                    "RSR SetSharpness",
                )?;
            }
            Ok(())
        })();
        rsr.release();
        result
    }

    /// Fluid Motion Frames as it is now.
    pub fn afmf(&self) -> Result<Setting> {
        let afmf = self.afmf_interface()?;
        let result = afmf
            .flag(IS_ENABLED, "AFMF IsEnabled")
            .map(|enabled| Setting {
                enabled,
                sharpness: None,
            });
        afmf.release();
        result
    }

    pub fn set_afmf(&self, enabled: bool) -> Result<()> {
        let afmf = self.afmf_interface()?;
        let result = afmf.set_flag(SET_ENABLED, enabled, "AFMF SetEnabled");
        afmf.release();
        result
    }

    fn supported(&self, feature: Interface, what: &str) -> Result<Interface> {
        if !feature.flag(IS_SUPPORTED, what).unwrap_or(false) {
            feature.release();
            return Err(HwError::Unsupported("AMD driver setting"));
        }
        Ok(feature)
    }

    fn gpu(&self) -> Result<Interface> {
        self.gpus
            .map(|(_, gpu)| gpu)
            .ok_or(HwError::Unsupported("AMD GPU"))
    }

    fn anti_lag_interface(&self) -> Result<Interface> {
        let feature = self
            .services
            .get_for(SERVICES_GET_ANTI_LAG, self.gpu()?, "GetAntiLag")?;
        self.supported(feature, "Anti-Lag IsSupported")
    }

    fn chill_interface(&self) -> Result<Interface> {
        let feature = self
            .services
            .get_for(SERVICES_GET_CHILL, self.gpu()?, "GetChill")?;
        self.supported(feature, "Chill IsSupported")
    }

    fn sharpening_interface(&self) -> Result<Interface> {
        let feature = self.services.get_for(
            SERVICES_GET_IMAGE_SHARPENING,
            self.gpu()?,
            "GetImageSharpening",
        )?;
        self.supported(feature, "Image Sharpening IsSupported")
    }

    pub fn anti_lag(&self) -> Result<bool> {
        let feature = self.anti_lag_interface()?;
        let result = feature.flag(IS_ENABLED, "Anti-Lag IsEnabled");
        feature.release();
        result
    }

    pub fn set_anti_lag(&self, enabled: bool) -> Result<()> {
        let feature = self.anti_lag_interface()?;
        let result = feature.set_flag(SET_ENABLED, enabled, "Anti-Lag SetEnabled");
        feature.release();
        result
    }

    pub fn chill(&self) -> Result<ChillSetting> {
        let feature = self.chill_interface()?;
        let result = (|| {
            Ok(ChillSetting {
                enabled: feature.flag(IS_ENABLED, "Chill IsEnabled")?,
                min: feature.int(CHILL_GET_MIN, "Chill GetMinFPS")?,
                max: feature.int(CHILL_GET_MAX, "Chill GetMaxFPS")?,
            })
        })();
        feature.release();
        result
    }

    /// Turns Chill on or off; when it is on and `range` is given, sets the
    /// frame rates it keeps between, within what the driver accepts.
    pub fn set_chill(&self, enabled: bool, range: Option<(i32, i32)>) -> Result<()> {
        let feature = self.chill_interface()?;
        let result = (|| {
            // Turned on first: the driver refuses the values of a feature
            // that is off.
            feature.set_flag(CHILL_SET_ENABLED, enabled, "Chill SetEnabled")?;
            if enabled && let Some((min, max)) = range {
                let limits = feature.range(CHILL_GET_RANGE, "Chill GetFPSRange")?;
                let (low, high) = (limits[0], limits[1]);
                let (min, max) = if low <= high {
                    let min = min.clamp(low, high);
                    (min, max.clamp(min, high))
                } else {
                    (min, max)
                };
                // The maximum may not drop under the minimum and the other
                // way round: one order or the other works.
                let first = feature
                    .set_int(CHILL_SET_MAX, max, "Chill SetMaxFPS")
                    .and_then(|()| feature.set_int(CHILL_SET_MIN, min, "Chill SetMinFPS"));
                if first.is_err() {
                    feature.set_int(CHILL_SET_MIN, min, "Chill SetMinFPS")?;
                    feature.set_int(CHILL_SET_MAX, max, "Chill SetMaxFPS")?;
                }
            }
            Ok(())
        })();
        feature.release();
        result
    }

    pub fn sharpening(&self) -> Result<Setting> {
        let feature = self.sharpening_interface()?;
        let result = (|| {
            Ok(Setting {
                enabled: feature.flag(IS_ENABLED, "Image Sharpening IsEnabled")?,
                sharpness: feature
                    .int(SHARPENING_GET, "Image Sharpening GetSharpness")
                    .ok(),
            })
        })();
        feature.release();
        result
    }

    pub fn set_sharpening(&self, enabled: bool, sharpness: Option<i32>) -> Result<()> {
        let feature = self.sharpening_interface()?;
        let result = (|| {
            // Turned on first: the driver refuses the values of a feature
            // that is off.
            feature.set_flag(
                SHARPENING_SET_ENABLED,
                enabled,
                "Image Sharpening SetEnabled",
            )?;
            if enabled && let Some(wanted) = sharpness {
                let limits = feature.range(SHARPENING_GET_RANGE, "Image Sharpening range");
                let wanted = match limits {
                    Ok(l) if l[0] <= l[1] => wanted.clamp(l[0], l[1]),
                    _ => wanted,
                };
                feature.set_int(SHARPENING_SET, wanted, "Image Sharpening SetSharpness")?;
            }
            Ok(())
        })();
        feature.release();
        result
    }

    /// Radeon Super Resolution as it was: its values written while it is on,
    /// then off again if it was off.
    pub fn put_back_rsr(&self, before: Setting) -> Result<()> {
        self.set_rsr(true, before.sharpness)?;
        if before.enabled {
            Ok(())
        } else {
            self.set_rsr(false, None)
        }
    }

    /// Chill as it was (see [`Self::put_back_rsr`]).
    pub fn put_back_chill(&self, before: ChillSetting) -> Result<()> {
        self.set_chill(true, Some((before.min, before.max)))?;
        if before.enabled {
            Ok(())
        } else {
            self.set_chill(false, None)
        }
    }

    /// Image sharpening as it was (see [`Self::put_back_rsr`]).
    pub fn put_back_sharpening(&self, before: Setting) -> Result<()> {
        self.set_sharpening(true, before.sharpness)?;
        if before.enabled {
            Ok(())
        } else {
            self.set_sharpening(false, None)
        }
    }

    /// Every feature as it is now; one that cannot be read is `None`.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            rsr: self.rsr().ok(),
            afmf: self.afmf().ok(),
            anti_lag: self.anti_lag().ok(),
            chill: self.chill().ok(),
            sharpening: self.sharpening().ok(),
        }
    }

    /// Puts both features back as `snapshot` had them, and returns what
    /// could not be.
    pub fn restore(&self, snapshot: &Snapshot) -> Vec<String> {
        let mut errors = Vec::new();
        if let Some(rsr) = snapshot.rsr
            && let Err(err) = self.put_back_rsr(rsr)
        {
            errors.push(format!("Radeon Super Resolution: {err}"));
        }
        if let Some(afmf) = snapshot.afmf
            && let Err(err) = self.set_afmf(afmf.enabled)
        {
            errors.push(format!("Fluid Motion Frames: {err}"));
        }
        if let Some(on) = snapshot.anti_lag
            && let Err(err) = self.set_anti_lag(on)
        {
            errors.push(format!("Anti-Lag: {err}"));
        }
        if let Some(chill) = snapshot.chill
            && let Err(err) = self.put_back_chill(chill)
        {
            errors.push(format!("Chill: {err}"));
        }
        if let Some(sharpening) = snapshot.sharpening
            && let Err(err) = self.put_back_sharpening(sharpening)
        {
            errors.push(format!("Image Sharpening: {err}"));
        }
        errors
    }
}

impl Drop for Radeon {
    fn drop(&mut self) {
        if let Some(services1) = self.services1 {
            services1.release();
        }
        if let Some((list, gpu)) = self.gpus {
            gpu.release();
            list.release();
        }
        self.services.release();
        // The system object is not reference counted: terminating ADLX
        // ends it.
        let _ = self.system;
        unsafe { (self.terminate)() };
        let _ = self.module;
    }
}
