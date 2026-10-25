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

/// The two buttons on the back of the Ally, as ASUS names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackButton {
    M1,
    M2,
}

/// What a back button sends. The controller's firmware does the
/// translation, so it works in every program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonTarget {
    /// The button's own key, as it leaves the factory.
    #[default]
    Default,
    /// Nothing at all.
    Disabled,
    PadA,
    PadB,
    PadX,
    PadY,
    PadLb,
    PadRb,
    PadLt,
    PadRt,
    /// Left stick click.
    PadLs,
    /// Right stick click.
    PadRs,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    PadView,
    PadMenu,
    /// The guide button.
    PadXbox,
    KeyEscape,
    KeyEnter,
    KeySpace,
    KeyTab,
    KeyBackspace,
    KeyUp,
    KeyDown,
    KeyLeft,
    KeyRight,
    KeyPageUp,
    KeyPageDown,
    KeyShift,
    KeyCtrl,
    KeyWindows,
    KeyF11,
    KeyF12,
    MouseLeft,
    MouseRight,
    MouseMiddle,
    WheelUp,
    WheelDown,
    VolumeUp,
    VolumeDown,
    MuteMic,
    /// The touch keyboard.
    ShowKeyboard,
}

impl ButtonTarget {
    /// Every target, in the order the shell offers them.
    pub const ALL: [ButtonTarget; 44] = [
        Self::Default,
        Self::Disabled,
        Self::PadA,
        Self::PadB,
        Self::PadX,
        Self::PadY,
        Self::PadLb,
        Self::PadRb,
        Self::PadLt,
        Self::PadRt,
        Self::PadLs,
        Self::PadRs,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
        Self::PadView,
        Self::PadMenu,
        Self::PadXbox,
        Self::KeyEscape,
        Self::KeyEnter,
        Self::KeySpace,
        Self::KeyTab,
        Self::KeyBackspace,
        Self::KeyUp,
        Self::KeyDown,
        Self::KeyLeft,
        Self::KeyRight,
        Self::KeyPageUp,
        Self::KeyPageDown,
        Self::KeyShift,
        Self::KeyCtrl,
        Self::KeyWindows,
        Self::KeyF11,
        Self::KeyF12,
        Self::MouseLeft,
        Self::MouseRight,
        Self::MouseMiddle,
        Self::WheelUp,
        Self::WheelDown,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::MuteMic,
        Self::ShowKeyboard,
    ];

    /// The JSON name, such as `pad_a`.
    pub fn key(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The target with this JSON name.
    pub fn from_key(key: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(key.to_owned())).ok()
    }
}

/// What each back button sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackButtons {
    pub m1: ButtonTarget,
    pub m2: ButtonTarget,
}

impl BackButtons {
    pub fn get(&self, button: BackButton) -> ButtonTarget {
        match button {
            BackButton::M1 => self.m1,
            BackButton::M2 => self.m2,
        }
    }

    pub fn set(&mut self, button: BackButton, target: ButtonTarget) {
        match button {
            BackButton::M1 => self.m1 = target,
            BackButton::M2 => self.m2 = target,
        }
    }

    /// Both buttons as they leave the factory.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

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
    }

    #[test]
    fn button_targets_in_json() {
        assert_eq!(ButtonTarget::PadLb.key(), "pad_lb");
        assert_eq!(ButtonTarget::KeyF11.key(), "key_f11");
        for target in ButtonTarget::ALL {
            assert_eq!(ButtonTarget::from_key(&target.key()), Some(target));
        }
        for (i, a) in ButtonTarget::ALL.iter().enumerate() {
            assert!(!ButtonTarget::ALL[i + 1..].contains(a), "{a:?} twice");
        }
        assert_eq!(ButtonTarget::from_key("pad_z"), None);
        let buttons: BackButtons = serde_json::from_str(r#"{"m2":"pad_a"}"#).unwrap();
        assert_eq!(buttons.get(BackButton::M1), ButtonTarget::Default);
        assert_eq!(buttons.get(BackButton::M2), ButtonTarget::PadA);
        assert!(!buttons.is_default());
    }

    #[test]
    fn controller_modes_in_json() {
        assert_eq!(
            serde_json::to_string(&ControllerMode::Wasd).unwrap(),
            r#""wasd""#
        );
    }
}
