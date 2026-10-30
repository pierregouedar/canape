//! Radeon Super Resolution and Fluid Motion Frames for the game in front.
//!
//! The Image section of a game's profile chooses them ([`crate::image`]);
//! while that game is in front they are turned on in the AMD driver, and the
//! driver is put back as it was when the game leaves. The decision
//! ([`Want`]) is pure and tested on any OS; the thread that talks to the
//! driver is Windows only. Called from the shell, in the user session.

use canape_core::{FrameGenMethod, FrameGenSettings, UpscalingMethod, UpscalingSettings};

/// What the profile asks the driver for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Want {
    /// Radeon Super Resolution on, with its sharpness.
    pub rsr: Option<u32>,
    /// Fluid Motion Frames on.
    pub afmf: bool,
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
pub fn want(upscaling: UpscalingSettings, frame_gen: FrameGenSettings) -> Want {
    Want {
        rsr: (upscaling.method == UpscalingMethod::AmdRsr).then_some(upscaling.sharpness),
        afmf: frame_gen.method == FrameGenMethod::AmdAfmf,
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

    fn run() {
        // Opened when something is asked for, closed when it is all given back.
        let mut driver: Option<(Radeon, Snapshot)> = None;
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
                if let Some((radeon, snapshot)) = driver.take() {
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
                    Ok(radeon) => {
                        let snapshot = radeon.snapshot();
                        crate::log(&format!("radeon: driver opened, was {snapshot:?}"));
                        driver = Some((radeon, snapshot));
                    }
                    Err(err) => {
                        crate::log(&format!("radeon: {err}"));
                        FAILED.store(true, Ordering::Relaxed);
                        continue;
                    }
                }
            }
            let (radeon, snapshot) = driver.as_ref().expect("opened above");
            let mut failed = false;
            if want.rsr != applied.rsr {
                // Off again means back as the driver had it, not forced off.
                let result = match want.rsr {
                    Some(sharpness) => radeon.set_rsr(true, Some(sharpness as i32)),
                    None => match snapshot.rsr {
                        Some(before) => radeon.set_rsr(before.enabled, before.sharpness),
                        None => Ok(()),
                    },
                };
                match result {
                    Ok(()) => applied.rsr = want.rsr,
                    Err(err) => {
                        crate::log(&format!("radeon: Radeon Super Resolution: {err}"));
                        failed = true;
                    }
                }
            }
            if want.afmf != applied.afmf {
                let result = if want.afmf {
                    radeon.set_afmf(true)
                } else {
                    match snapshot.afmf {
                        Some(before) => radeon.set_afmf(before.enabled),
                        None => Ok(()),
                    }
                };
                match result {
                    Ok(()) => applied.afmf = want.afmf,
                    Err(err) => {
                        crate::log(&format!("radeon: Fluid Motion Frames: {err}"));
                        failed = true;
                    }
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
        assert!(want(UpscalingSettings::default(), FrameGenSettings::default()).is_empty());
        assert_eq!(
            want(rsr, afmf),
            Want {
                rsr: Some(40),
                afmf: true
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
        assert!(want(lossless_up, lossless_gen).is_empty());
    }
}
