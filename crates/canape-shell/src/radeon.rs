//! Radeon Super Resolution and Fluid Motion Frames for the game in front.
//!
//! The Image section of a game's profile chooses them ([`crate::image`]);
//! while that game is in front they are turned on in the AMD driver, and the
//! driver is put back as it was when the game leaves. The decision
//! ([`Want`]) is pure and tested on any OS; the thread that talks to the
//! driver is Windows only. Called from the shell, in the user session.

use canape_core::{
    AmdSettings, FrameGenMethod, FrameGenSettings, UpscalingMethod, UpscalingSettings,
};

/// What the profile asks the driver for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Want {
    /// Radeon Super Resolution on, with its sharpness.
    pub rsr: Option<u32>,
    /// Fluid Motion Frames on.
    pub afmf: bool,
    pub anti_lag: bool,
    /// Chill on, between these frame rates.
    pub chill: Option<(u32, u32)>,
    /// Image Sharpening on, with its sharpness.
    pub sharpening: Option<u32>,
}

impl Want {
    #[cfg(any(windows, test))]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The driver features a profile's choices need. The other methods are not
/// the driver's (Lossless Scaling), and "none" leaves the driver as the
/// user has it.
pub fn want(upscaling: UpscalingSettings, frame_gen: FrameGenSettings, amd: AmdSettings) -> Want {
    let amd = amd.clamped();
    Want {
        rsr: (upscaling.method == UpscalingMethod::AmdRsr).then_some(upscaling.sharpness),
        afmf: frame_gen.method == FrameGenMethod::AmdAfmf,
        anti_lag: amd.anti_lag,
        chill: amd.chill.map(|c| (c.min, c.max)),
        sharpening: amd.sharpening,
    }
}

#[cfg(windows)]
pub use engine::{failed, follow};

/// Nothing to apply off Windows.
#[cfg(not(windows))]
pub fn follow(_want: Want) {}

#[cfg(not(windows))]
pub fn failed() -> bool {
    false
}

#[cfg(windows)]
mod engine {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::thread;

    use canape_hw::radeon::{Radeon, Snapshot};

    use super::Want;

    static WANTED: Mutex<Want> = Mutex::new(Want {
        rsr: None,
        afmf: false,
        anti_lag: false,
        chill: None,
        sharpening: None,
    });
    static CHANGED: Condvar = Condvar::new();
    static STARTED: Mutex<bool> = Mutex::new(false);
    /// The driver could not be used for what was asked.
    static FAILED: AtomicBool = AtomicBool::new(false);

    /// What the profile in front asks for now.
    pub fn follow(want: Want) {
        {
            let mut started = STARTED.lock().unwrap();
            if !*started {
                if want.is_empty() {
                    return;
                }
                *started = true;
                thread::spawn(run);
            }
        }
        let mut wanted = WANTED.lock().unwrap();
        if *wanted != want {
            *wanted = want;
            CHANGED.notify_all();
        }
    }

    /// The last attempt on the driver failed.
    pub fn failed() -> bool {
        FAILED.load(Ordering::Relaxed)
    }

    /// Logs a failure of one feature.
    fn report(feature: &str, result: canape_hw::Result<()>, failed: &mut bool) -> bool {
        match result {
            Ok(()) => true,
            Err(err) => {
                crate::log(&format!("radeon: {feature}: {err}"));
                *failed = true;
                false
            }
        }
    }

    fn run() {
        // Opened when something is asked for, and then kept for the life of
        // the process: the driver's module crashes when it is closed and
        // opened again. What it held when a game came is `before`.
        let mut driver: Option<Radeon> = None;
        let mut before: Option<Snapshot> = None;
        let mut applied = Want::default();
        let mut seen = Want::default();
        loop {
            let want = {
                let mut wanted = WANTED.lock().unwrap();
                while *wanted == seen {
                    wanted = CHANGED.wait(wanted).unwrap();
                }
                seen = *wanted;
                *wanted
            };
            if want.is_empty() {
                if let (Some(radeon), Some(snapshot)) = (driver.as_ref(), before.take()) {
                    for error in radeon.restore(&snapshot) {
                        crate::log(&format!("radeon: {error}"));
                    }
                    crate::log("radeon: settings given back");
                }
                applied = Want::default();
                FAILED.store(false, Ordering::Relaxed);
                continue;
            }
            if driver.is_none() {
                match Radeon::open() {
                    Ok(radeon) => driver = Some(radeon),
                    Err(err) => {
                        crate::log(&format!("radeon: {err}"));
                        FAILED.store(true, Ordering::Relaxed);
                        continue;
                    }
                }
            }
            let radeon = driver.as_ref().expect("opened above");
            let before = before.get_or_insert_with(|| {
                let snapshot = radeon.snapshot();
                crate::log(&format!("radeon: driver read, was {snapshot:?}"));
                snapshot
            });
            let mut failed = false;
            // A feature no longer asked for goes back as the driver had it,
            // not forced off.
            if want.rsr != applied.rsr {
                let result = match want.rsr {
                    Some(sharpness) => radeon.set_rsr(true, Some(sharpness as i32)),
                    None => before
                        .rsr
                        .map_or(Ok(()), |b| radeon.set_rsr(b.enabled, b.sharpness)),
                };
                if report("Radeon Super Resolution", result, &mut failed) {
                    applied.rsr = want.rsr;
                }
            }
            if want.afmf != applied.afmf {
                let result = if want.afmf {
                    radeon.set_afmf(true)
                } else {
                    before.afmf.map_or(Ok(()), |b| radeon.set_afmf(b.enabled))
                };
                if report("Fluid Motion Frames", result, &mut failed) {
                    applied.afmf = want.afmf;
                }
            }
            if want.anti_lag != applied.anti_lag {
                let result = if want.anti_lag {
                    radeon.set_anti_lag(true)
                } else {
                    before.anti_lag.map_or(Ok(()), |b| radeon.set_anti_lag(b))
                };
                if report("Anti-Lag", result, &mut failed) {
                    applied.anti_lag = want.anti_lag;
                }
            }
            if want.chill != applied.chill {
                let result = match want.chill {
                    Some((min, max)) => radeon.set_chill(true, Some((min as i32, max as i32))),
                    None => before.chill.map_or(Ok(()), |b| {
                        radeon.set_chill(b.enabled, Some((b.min, b.max)))
                    }),
                };
                if report("Chill", result, &mut failed) {
                    applied.chill = want.chill;
                }
            }
            if want.sharpening != applied.sharpening {
                let result = match want.sharpening {
                    Some(sharpness) => radeon.set_sharpening(true, Some(sharpness as i32)),
                    None => before
                        .sharpening
                        .map_or(Ok(()), |b| radeon.set_sharpening(b.enabled, b.sharpness)),
                };
                if report("Image Sharpening", result, &mut failed) {
                    applied.sharpening = want.sharpening;
                }
            }
            FAILED.store(failed, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_core::FrameGenRate;

    #[test]
    fn only_the_drivers_own_methods_are_asked_of_it() {
        let rsr = UpscalingSettings {
            method: UpscalingMethod::AmdRsr,
            sharpness: 40,
        };
        let afmf = FrameGenSettings {
            method: FrameGenMethod::AmdAfmf,
            rate: FrameGenRate::X2,
            target_fps: 60,
        };
        let none = AmdSettings::default();
        assert!(
            want(
                UpscalingSettings::default(),
                FrameGenSettings::default(),
                none
            )
            .is_empty()
        );
        assert_eq!(
            want(rsr, afmf, none),
            Want {
                rsr: Some(40),
                afmf: true,
                ..Want::default()
            }
        );
        let lossless_up = UpscalingSettings {
            method: UpscalingMethod::LosslessScaling,
            sharpness: 50,
        };
        let lossless_gen = FrameGenSettings {
            method: FrameGenMethod::LosslessScaling,
            ..afmf
        };
        assert!(want(lossless_up, lossless_gen, none).is_empty());
        // The driver's own settings, kept in range.
        let amd = AmdSettings {
            anti_lag: true,
            sharpening: Some(300),
            chill: Some(canape_core::ChillRange { min: 50, max: 10 }),
        };
        assert_eq!(
            want(
                UpscalingSettings::default(),
                FrameGenSettings::default(),
                amd
            ),
            Want {
                anti_lag: true,
                sharpening: Some(100),
                chill: Some((50, 50)),
                ..Want::default()
            }
        );
    }
}
