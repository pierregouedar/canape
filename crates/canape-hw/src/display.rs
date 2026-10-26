//! Refresh rate and resolution of the built-in screen. Display modes belong to the user's
//! session, so the shell changes them itself; no rights are needed.
//!
//! The built-in panel is the active display path whose output technology
//! is "internal" or "embedded DisplayPort": a TV or monitor plugged in is
//! never touched.

/// One display mode: width, height, bits per pixel, refresh rate (Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub bits: u32,
    pub hz: u32,
}

/// The refresh rates offered at the current resolution and depth, lowest
/// first. Rates of 0 and 1 mean "hardware default" and are left out.
pub fn rates_for(modes: &[Mode], current: Mode) -> Vec<u32> {
    let mut rates: Vec<u32> = modes
        .iter()
        .filter(|m| {
            m.width == current.width
                && m.height == current.height
                && m.bits == current.bits
                && m.hz > 1
        })
        .map(|m| m.hz)
        .collect();
    rates.sort_unstable();
    rates.dedup();
    rates
}

/// The resolutions offered at the current colour depth, largest first,
/// as `(width, height)`. Only those with the panel's aspect ratio are
/// kept: the panel's own is that of the largest mode it reports, so a
/// 16:9 panel is never offered 4:3 modes that would stretch the image.
pub fn resolutions_for(modes: &[Mode], current: Mode) -> Vec<(u32, u32)> {
    let usable = || {
        modes
            .iter()
            .filter(|m| m.bits == current.bits && m.hz > 1 && m.width > 0 && m.height > 0)
    };
    let Some(native) = usable().max_by_key(|m| u64::from(m.width) * u64::from(m.height)) else {
        return Vec::new();
    };
    let (nw, nh) = (u64::from(native.width), u64::from(native.height));
    let mut sizes: Vec<(u32, u32)> = usable()
        .filter(|m| {
            // Equal ratios within 1 %: 1366x768 counts as 16:9.
            let (w, h) = (u64::from(m.width), u64::from(m.height));
            (w * nh).abs_diff(h * nw) * 100 <= h * nw
        })
        .map(|m| (m.width, m.height))
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    sizes
}

#[cfg(windows)]
pub use sys::*;

#[cfg(windows)]
mod sys {
    use super::{Mode, rates_for, resolutions_for};
    use crate::{HwError, Result};

    use windows_sys::Win32::Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_PATH_INFO,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
        QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    };
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::Graphics::Gdi::{
        CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_SUCCESSFUL,
        DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS,
        EnumDisplaySettingsExW,
    };

    /// GDI name of the built-in screen (`\\.\DISPLAY1`), NUL-terminated.
    pub struct Screen(Vec<u16>);

    impl Screen {
        /// The built-in screen, when it is on.
        pub fn internal() -> Option<Self> {
            let (mut paths, mut modes) = (0u32, 0u32);
            if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) }
                != ERROR_SUCCESS
            {
                return None;
            }
            let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
            let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
            let status = unsafe {
                QueryDisplayConfig(
                    QDC_ONLY_ACTIVE_PATHS,
                    &mut paths,
                    path_info.as_mut_ptr(),
                    &mut modes,
                    mode_info.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                return None;
            }
            path_info[..paths as usize]
                .iter()
                .filter(|p| {
                    let tech = p.targetInfo.outputTechnology;
                    tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                        || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                })
                .find_map(|p| {
                    let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                    name.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
                    name.header.adapterId = p.sourceInfo.adapterId;
                    name.header.id = p.sourceInfo.id;
                    if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
                        return None;
                    }
                    let len = name.viewGdiDeviceName.iter().position(|&c| c == 0)?;
                    let mut gdi = name.viewGdiDeviceName[..len].to_vec();
                    gdi.push(0);
                    Some(Self(gdi))
                })
        }

        fn mode(&self, index: u32) -> Option<(Mode, DEVMODEW)> {
            let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
            dm.dmSize = size_of::<DEVMODEW>() as u16;
            let ok = unsafe { EnumDisplaySettingsExW(self.0.as_ptr(), index, &mut dm, 0) } != 0;
            ok.then(|| {
                let mode = Mode {
                    width: dm.dmPelsWidth,
                    height: dm.dmPelsHeight,
                    bits: dm.dmBitsPerPel,
                    hz: dm.dmDisplayFrequency,
                };
                (mode, dm)
            })
        }

        /// The current refresh rate.
        pub fn refresh(&self) -> Option<u32> {
            self.mode(ENUM_CURRENT_SETTINGS).map(|(m, _)| m.hz)
        }

        /// Rates offered at the current resolution, lowest first.
        pub fn rates(&self) -> Vec<u32> {
            let Some((current, _)) = self.mode(ENUM_CURRENT_SETTINGS) else {
                return Vec::new();
            };
            let modes: Vec<Mode> = (0..).map_while(|i| self.mode(i)).map(|(m, _)| m).collect();
            rates_for(&modes, current)
        }

        /// The current resolution.
        pub fn resolution(&self) -> Option<(u32, u32)> {
            self.mode(ENUM_CURRENT_SETTINGS)
                .map(|(m, _)| (m.width, m.height))
        }

        /// Resolutions offered with the panel's aspect ratio, largest first.
        pub fn resolutions(&self) -> Vec<(u32, u32)> {
            let Some((current, _)) = self.mode(ENUM_CURRENT_SETTINGS) else {
                return Vec::new();
            };
            let modes: Vec<Mode> = (0..).map_while(|i| self.mode(i)).map(|(m, _)| m).collect();
            resolutions_for(&modes, current)
        }

        /// Switches to `width` x `height`, keeping the colour depth; Windows
        /// picks the refresh rate, so set it afterwards. Saved for the
        /// next sign-in like a change made in Windows settings.
        pub fn set_resolution(&self, width: u32, height: u32) -> Result<()> {
            let (current, mut dm) = self
                .mode(ENUM_CURRENT_SETTINGS)
                .ok_or(HwError::Unsupported("screen mode unreadable"))?;
            if (current.width, current.height) == (width, height) {
                return Ok(());
            }
            dm.dmPelsWidth = width;
            dm.dmPelsHeight = height;
            dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT;
            let result = unsafe {
                ChangeDisplaySettingsExW(
                    self.0.as_ptr(),
                    &dm,
                    std::ptr::null_mut(),
                    CDS_UPDATEREGISTRY,
                    std::ptr::null(),
                )
            };
            if result == DISP_CHANGE_SUCCESSFUL {
                Ok(())
            } else {
                Err(HwError::Driver(format!(
                    "ChangeDisplaySettingsEx({width}x{height}) returned {result}"
                )))
            }
        }

        /// Switches to `hz` at the current resolution, saved for the next
        /// sign-in like a change made in Windows settings.
        pub fn set_refresh(&self, hz: u32) -> Result<()> {
            let (current, mut dm) = self
                .mode(ENUM_CURRENT_SETTINGS)
                .ok_or(HwError::Unsupported("screen mode unreadable"))?;
            if current.hz == hz {
                return Ok(());
            }
            dm.dmDisplayFrequency = hz;
            dm.dmFields = DM_DISPLAYFREQUENCY;
            let result = unsafe {
                ChangeDisplaySettingsExW(
                    self.0.as_ptr(),
                    &dm,
                    std::ptr::null_mut(),
                    CDS_UPDATEREGISTRY,
                    std::ptr::null(),
                )
            };
            if result == DISP_CHANGE_SUCCESSFUL {
                Ok(())
            } else {
                Err(HwError::Driver(format!(
                    "ChangeDisplaySettingsEx({hz} Hz) returned {result}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: u32, height: u32, hz: u32) -> Mode {
        Mode {
            width,
            height,
            bits: 32,
            hz,
        }
    }

    #[test]
    fn rates_follow_the_current_resolution() {
        let modes = [
            mode(1920, 1080, 120),
            mode(1920, 1080, 60),
            mode(1920, 1080, 60),
            mode(1920, 1080, 1),
            mode(1280, 720, 48),
            Mode {
                bits: 16,
                ..mode(1920, 1080, 30)
            },
        ];
        assert_eq!(rates_for(&modes, mode(1920, 1080, 120)), [60, 120]);
        assert_eq!(rates_for(&modes, mode(1280, 720, 48)), [48]);
        assert!(rates_for(&[], mode(1920, 1080, 120)).is_empty());
    }

    #[test]
    fn resolutions_keep_the_panel_aspect_ratio() {
        let modes = [
            mode(1920, 1080, 120),
            mode(1920, 1080, 60),
            mode(1600, 900, 60),
            mode(1280, 720, 60),
            mode(1366, 768, 60),
            // 4:3 and 16:10, not the panel's shape.
            mode(1024, 768, 60),
            mode(1280, 800, 60),
            // Hardware default rate, other depth.
            mode(800, 450, 1),
            Mode {
                bits: 16,
                ..mode(960, 540, 60)
            },
        ];
        assert_eq!(
            resolutions_for(&modes, mode(1920, 1080, 120)),
            [(1920, 1080), (1600, 900), (1366, 768), (1280, 720)]
        );
        assert!(resolutions_for(&[], mode(1920, 1080, 120)).is_empty());
    }

    #[test]
    fn a_portrait_or_small_panel_follows_its_own_largest_mode() {
        let modes = [
            mode(1080, 1920, 60),
            mode(720, 1280, 60),
            mode(1280, 720, 60),
        ];
        assert_eq!(
            resolutions_for(&modes, mode(1080, 1920, 60)),
            [(1080, 1920), (720, 1280)]
        );
    }
}
