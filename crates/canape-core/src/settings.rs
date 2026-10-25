use serde::{Deserialize, Serialize};

use crate::fan::FanCurves;

/// ASUS performance modes exposed by the Ally firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    Silent,
    Performance,
    Turbo,
    /// Power limits are set explicitly (manual TDP or auto TDP).
    Manual,
}

/// User-tunable auto TDP parameters, stored per game profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoTdpSettings {
    pub enabled: bool,
    pub target_fps: u32,
    pub min_watts: u32,
    pub max_watts: u32,
    /// Lower ceiling while running on battery.
    #[serde(default)]
    pub battery_max_watts: Option<u32>,
    /// Above this CPU temperature, power only goes down.
    #[serde(default)]
    pub max_temp_c: Option<u32>,
}

impl AutoTdpSettings {
    /// The settings with every wattage brought into the device's `(min, max)`
    /// range; `None` when the minimum ends up above the maximum or the
    /// target is 0.
    pub fn clamped(self, (low, high): (u32, u32)) -> Option<Self> {
        let settings = Self {
            min_watts: self.min_watts.clamp(low, high),
            max_watts: self.max_watts.clamp(low, high),
            battery_max_watts: self.battery_max_watts.map(|w| w.clamp(low, high)),
            ..self
        };
        (settings.min_watts <= settings.max_watts && settings.target_fps > 0).then_some(settings)
    }
}

impl Default for AutoTdpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            target_fps: 60,
            min_watts: 7,
            max_watts: 25,
            battery_max_watts: None,
            max_temp_c: None,
        }
    }
}

/// Settings applied automatically while a given game is in the foreground.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Executable name, compared case-insensitively (for example `eldenring.exe`).
    pub exe: String,
    pub auto_tdp: AutoTdpSettings,
    /// Wattage auto TDP last settled on for this game: the next session starts there.
    #[serde(default)]
    pub learned_watts: Option<u32>,
    /// Fan curves for this game; `None` keeps the global ones.
    #[serde(default)]
    pub fan_curves: Option<FanCurves>,
    /// Frame rate cap for this game; `None` keeps the global setting.
    #[serde(default)]
    pub fps_limit: Option<u32>,
    /// Refresh rate of the built-in screen for this game, in Hz. The
    /// service only stores it: the shell switches the screen, since display
    /// modes belong to the user's session. `None` keeps the shell's choice.
    #[serde(default)]
    pub refresh_hz: Option<u32>,
    /// Gyro aiming for this game; `None` keeps the global settings. Like
    /// the refresh rate, applied by the shell.
    #[serde(default)]
    pub gyro: Option<GyroSettings>,
}

impl Profile {
    pub fn matches(&self, exe: &str) -> bool {
        self.exe.eq_ignore_ascii_case(exe)
    }
}

/// Frame rate caps accepted by `set_fps_limit`: from 30 up to the Ally's
/// 120 Hz screen; above that a cap has no effect.
pub const FPS_LIMIT_RANGE: (u32, u32) = (30, 120);

/// Mode of the Ally's built-in controller, as the firmware names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerMode {
    /// XInput gamepad.
    Gamepad,
    /// Keyboard: sticks and buttons send keys.
    Wasd,
    /// Desktop: the right stick moves the mouse.
    Mouse,
}

/// When gyro aiming moves the mouse. It only ever acts while a game is in
/// front, never on the shell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroActivation {
    #[default]
    Off,
    /// While a back paddle is held.
    Hold,
    /// A back paddle press turns it on, the next one off.
    Toggle,
    /// All the time.
    Always,
}

/// Which rotation moves the mouse sideways.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroAxis {
    /// Turning the device left or right, as when looking around.
    #[default]
    Yaw,
    /// Tilting it like a steering wheel.
    Roll,
}

/// Gyro aiming: device rotation turned into mouse movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GyroSettings {
    pub activation: GyroActivation,
    /// Mouse counts per degree of rotation, within [`GYRO_SENSITIVITY_RANGE`].
    pub sensitivity: u32,
    pub horizontal: GyroAxis,
    /// Tilting the top away moves the mouse down instead of up.
    pub invert_y: bool,
}

impl Default for GyroSettings {
    fn default() -> Self {
        Self {
            activation: GyroActivation::Off,
            sensitivity: 10,
            horizontal: GyroAxis::Yaw,
            invert_y: false,
        }
    }
}

/// Sensitivities accepted by `set_gyro`, in mouse counts per degree.
pub const GYRO_SENSITIVITY_RANGE: (u32, u32) = (1, 50);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_tdp_clamps_to_the_device() {
        let settings = AutoTdpSettings {
            min_watts: 3,
            max_watts: 40,
            battery_max_watts: Some(50),
            ..AutoTdpSettings::default()
        };
        let clamped = settings.clamped((5, 30)).unwrap();
        assert_eq!(
            (
                clamped.min_watts,
                clamped.max_watts,
                clamped.battery_max_watts
            ),
            (5, 30, Some(30))
        );
        let inverted = AutoTdpSettings {
            min_watts: 20,
            max_watts: 10,
            ..AutoTdpSettings::default()
        };
        assert_eq!(inverted.clamped((5, 30)), None);
        let no_target = AutoTdpSettings {
            target_fps: 0,
            ..AutoTdpSettings::default()
        };
        assert_eq!(no_target.clamped((5, 30)), None);
    }

    #[test]
    fn older_profiles_still_load() {
        let json = r#"{"exe":"game.exe","auto_tdp":{"enabled":true,"target_fps":40,"min_watts":7,"max_watts":20}}"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.fan_curves, None);
        assert_eq!(profile.fps_limit, None);
        assert_eq!(profile.gyro, None);
    }

    #[test]
    fn gyro_settings_in_json() {
        let settings = GyroSettings {
            activation: GyroActivation::Hold,
            ..GyroSettings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            json,
            r#"{"activation":"hold","sensitivity":10,"horizontal":"yaw","invert_y":false}"#
        );
        // Missing fields take their defaults.
        let partial: GyroSettings = serde_json::from_str(r#"{"activation":"always"}"#).unwrap();
        assert_eq!(partial.activation, GyroActivation::Always);
        assert_eq!(partial.sensitivity, 10);
    }

    #[test]
    fn controller_modes_in_json() {
        assert_eq!(
            serde_json::to_string(&ControllerMode::Wasd).unwrap(),
            r#""wasd""#
        );
    }
}
