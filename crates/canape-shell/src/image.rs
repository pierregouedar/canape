//! Upscaling and frame generation of the Hardware page's Image section.
//!
//! The choices are kept here, taken from the active game profile when it
//! changes, and saved back into the profile with the rest of the hardware
//! settings. The AMD driver's own methods are applied by [`crate::radeon`]
//! while the game is in front; Lossless Scaling is not applied yet.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use canape_core::settings::{CHILL_FPS_RANGE, FRAME_GEN_TARGET_RANGE, SHARPNESS_RANGE};
use canape_core::{
    AmdSettings, ChillRange, FrameGenMethod, FrameGenRate, FrameGenSettings, UpscalingMethod,
    UpscalingSettings,
};

/// The choices of a profile's Image section.
pub type Choices = (UpscalingSettings, FrameGenSettings, AmdSettings);

/// What is chosen for the game now.
#[derive(Debug, Default)]
struct State {
    /// The profile the choices were taken from.
    profile: Option<String>,
    /// Whether they were read from that profile (its list can arrive
    /// after the status that names it).
    seeded: bool,
    /// What the profile held when they were read: a change made elsewhere
    /// since (the profile list arrives late, or `canape-ctl`) is taken up
    /// as long as the page has not edited the choices.
    read: Option<Choices>,
    /// The page edited the choices since.
    edited: bool,
    upscaling: UpscalingSettings,
    frame_gen: FrameGenSettings,
    amd: AmdSettings,
}

static STATE: Mutex<State> = Mutex::new(State {
    profile: None,
    seeded: false,
    read: None,
    edited: false,
    upscaling: UpscalingSettings {
        method: UpscalingMethod::None,
        sharpness: 50,
    },
    frame_gen: FrameGenSettings {
        method: FrameGenMethod::None,
        rate: FrameGenRate::X2,
        target_fps: 60,
    },
    amd: AmdSettings {
        anti_lag: false,
        sharpening: None,
        chill: None,
    },
});

/// Follows the active profile: its image settings (`saved`, `None` while
/// the profile list is not known) become the choices when it changes, and
/// the defaults come back when it leaves.
pub fn follow(active: Option<&str>, saved: Option<Choices>) {
    let mut state = STATE.lock().unwrap();
    if state.profile.as_deref() != active {
        *state = State {
            profile: active.map(str::to_owned),
            ..State::default()
        };
    }
    if active.is_some()
        && let Some((upscaling, frame_gen, amd)) = saved
        && (!state.seeded || (!state.edited && state.read != saved))
    {
        state.upscaling = upscaling;
        state.frame_gen = frame_gen;
        state.amd = amd;
        state.seeded = true;
        state.read = saved;
    }
    apply(&state);
}

/// Asks the driver for what the active profile chooses, or gives its
/// settings back when no profile is active.
fn apply(state: &State) {
    crate::radeon::follow(if state.profile.is_some() {
        crate::radeon::want(state.upscaling, state.frame_gen, state.amd)
    } else {
        crate::radeon::Want::default()
    });
}

/// The choices now.
pub fn current() -> Choices {
    let state = STATE.lock().unwrap();
    (state.upscaling, state.frame_gen, state.amd)
}

/// The methods in the order the page lists them.
const UPSCALING: [UpscalingMethod; 3] = [
    UpscalingMethod::None,
    UpscalingMethod::AmdRsr,
    UpscalingMethod::LosslessScaling,
];
const FRAME_GEN: [FrameGenMethod; 3] = [
    FrameGenMethod::None,
    FrameGenMethod::AmdAfmf,
    FrameGenMethod::LosslessScaling,
];
const RATES: [FrameGenRate; 4] = [
    FrameGenRate::X2,
    FrameGenRate::X3,
    FrameGenRate::X4,
    FrameGenRate::Adaptive,
];

pub fn upscaling_index(method: UpscalingMethod) -> i32 {
    UPSCALING.iter().position(|&m| m == method).unwrap_or(0) as i32
}

pub fn frame_gen_index(method: FrameGenMethod) -> i32 {
    FRAME_GEN.iter().position(|&m| m == method).unwrap_or(0) as i32
}

pub fn rate_index(rate: FrameGenRate) -> i32 {
    RATES.iter().position(|&r| r == rate).unwrap_or(0) as i32
}

/// Sets the upscaling from the page's indexes; an unknown method is none,
/// the sharpness is brought into range.
pub fn set_upscaling(method: i32, sharpness: i32) {
    let (low, high) = SHARPNESS_RANGE;
    let mut state = STATE.lock().unwrap();
    state.edited = true;
    state.upscaling = UpscalingSettings {
        method: usize::try_from(method)
            .ok()
            .and_then(|i| UPSCALING.get(i).copied())
            .unwrap_or_default(),
        sharpness: sharpness.clamp(low as i32, high as i32) as u32,
    };
    apply(&state);
}

/// Sets the frame generation from the page's indexes.
pub fn set_frame_gen(method: i32, rate: i32, target_fps: i32) {
    let (low, high) = FRAME_GEN_TARGET_RANGE;
    let mut state = STATE.lock().unwrap();
    state.edited = true;
    state.frame_gen = FrameGenSettings {
        method: usize::try_from(method)
            .ok()
            .and_then(|i| FRAME_GEN.get(i).copied())
            .unwrap_or_default(),
        rate: usize::try_from(rate)
            .ok()
            .and_then(|i| RATES.get(i).copied())
            .unwrap_or_default(),
        target_fps: target_fps.clamp(low as i32, high as i32) as u32,
    };
    apply(&state);
}

/// Sets the AMD driver's settings from the page: Anti-Lag, the sharpness of
/// Image Sharpening (`-1` for off), Chill on or off with its two frame rates.
/// Values are brought into range.
pub fn set_amd(anti_lag: bool, sharpening: i32, chill: bool, chill_min: i32, chill_max: i32) {
    let (low, high) = CHILL_FPS_RANGE;
    let mut state = STATE.lock().unwrap();
    state.edited = true;
    state.amd = AmdSettings {
        anti_lag,
        sharpening: u32::try_from(sharpening).ok(),
        chill: chill.then(|| ChillRange {
            min: chill_min.clamp(low as i32, high as i32) as u32,
            max: chill_max.clamp(low as i32, high as i32) as u32,
        }),
    }
    .clamped();
    apply(&state);
}

/// How long a look at the Steam libraries is trusted.
const LOSSLESS_CACHE: Duration = Duration::from_secs(30);

static LOSSLESS: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Whether Lossless Scaling is installed (Steam libraries), checked at
/// most every 30 seconds. `CANAPE_MOCK_LOSSLESS=1` fakes it off Windows.
pub fn lossless_installed() -> bool {
    let mut cache = LOSSLESS.lock().unwrap();
    if let Some((at, found)) = *cache
        && at.elapsed() < LOSSLESS_CACHE
    {
        return found;
    }
    let found = if cfg!(windows) {
        canape_library::Roots::system()
            .steam
            .and_then(|steam| canape_library::steam::lossless_scaling(&steam))
            .is_some()
    } else {
        std::env::var_os("CANAPE_MOCK_LOSSLESS").is_some()
    };
    *cache = Some((Instant::now(), found));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_round_trip() {
        for (i, method) in UPSCALING.into_iter().enumerate() {
            assert_eq!(upscaling_index(method), i as i32);
        }
        for (i, method) in FRAME_GEN.into_iter().enumerate() {
            assert_eq!(frame_gen_index(method), i as i32);
        }
        for (i, rate) in RATES.into_iter().enumerate() {
            assert_eq!(rate_index(rate), i as i32);
        }
    }

    #[test]
    fn choices_follow_the_profile_and_are_kept_in_range() {
        let saved = (
            UpscalingSettings {
                method: UpscalingMethod::AmdRsr,
                sharpness: 30,
            },
            FrameGenSettings::default(),
            AmdSettings::default(),
        );
        // The profile is named before its list arrives.
        follow(Some("a.exe"), None);
        assert_eq!(current().0.method, UpscalingMethod::None);
        follow(Some("a.exe"), Some(saved));
        assert_eq!(current(), saved);
        // Edits stay until the profile changes.
        set_upscaling(2, 400);
        assert_eq!(current().0.method, UpscalingMethod::LosslessScaling);
        assert_eq!(current().0.sharpness, 100);
        follow(Some("a.exe"), Some(saved));
        assert_eq!(current().0.sharpness, 100);
        follow(None, None);
        // A profile changed elsewhere after it was read is taken up, until
        // the page edits it.
        let changed = (
            UpscalingSettings {
                method: UpscalingMethod::AmdRsr,
                sharpness: 80,
            },
            FrameGenSettings::default(),
            AmdSettings::default(),
        );
        follow(Some("b.exe"), Some(saved));
        assert_eq!(current(), saved);
        follow(Some("b.exe"), Some(changed));
        assert_eq!(current(), changed);
        set_upscaling(0, 10);
        follow(Some("b.exe"), Some(saved));
        assert_eq!(current().0.method, UpscalingMethod::None);
        follow(Some("a.exe"), Some(saved));
        set_frame_gen(2, 3, 10);
        assert_eq!(current().1.rate, FrameGenRate::Adaptive);
        assert_eq!(current().1.target_fps, 30);
        // Unknown indexes mean none.
        set_frame_gen(-1, 9, 60);
        assert_eq!(current().1.method, FrameGenMethod::None);
        set_amd(true, 500, true, 50, 10);
        assert_eq!(
            current().2,
            AmdSettings {
                anti_lag: true,
                sharpening: Some(100),
                chill: Some(ChillRange { min: 50, max: 50 }),
            }
        );
        set_amd(false, -1, false, 40, 60);
        assert_eq!(current().2, AmdSettings::default());
        follow(None, None);
        assert_eq!(current(), Default::default());
    }
}
