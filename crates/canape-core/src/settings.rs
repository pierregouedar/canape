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
    /// Processor power settings for this game; `None` keeps the global ones.
    #[serde(default)]
    pub cpu_power: Option<CpuPower>,
    /// Resolution of the built-in screen for this game; `None` keeps the
    /// current one. Applied by the shell, like the refresh rate.
    #[serde(default)]
    pub resolution: Option<Resolution>,
    /// Upscaling for this game. Stored only: nothing applies it yet.
    #[serde(default)]
    pub upscaling: UpscalingSettings,
    /// Frame generation for this game. Stored only: nothing applies it yet.
    #[serde(default)]
    pub frame_gen: FrameGenSettings,
}

/// A screen resolution in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

/// What scales the game's image up from a lower resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpscalingMethod {
    #[default]
    None,
    /// The graphics driver's Radeon Super Resolution.
    AmdRsr,
    LosslessScaling,
}

impl UpscalingMethod {
    /// Name used in the log.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AmdRsr => "AMD (RSR)",
            Self::LosslessScaling => "Lossless Scaling",
        }
    }
}

/// Sharpness range of upscaling, in percent.
pub const SHARPNESS_RANGE: (u32, u32) = (0, 100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpscalingSettings {
    pub method: UpscalingMethod,
    /// Within [`SHARPNESS_RANGE`].
    pub sharpness: u32,
}

impl Default for UpscalingSettings {
    fn default() -> Self {
        Self {
            method: UpscalingMethod::None,
            sharpness: 50,
        }
    }
}

/// What inserts generated frames between the game's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameGenMethod {
    #[default]
    None,
    /// The graphics driver's Fluid Motion Frames.
    AmdAfmf,
    LosslessScaling,
}

impl FrameGenMethod {
    /// Name used in the log.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AmdAfmf => "AMD (AFMF)",
            Self::LosslessScaling => "Lossless Scaling",
        }
    }
}

/// How many frames Lossless Scaling generates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameGenRate {
    /// Twice the game's frame rate.
    #[default]
    X2,
    X3,
    X4,
    /// Whatever reaches [`FrameGenSettings::target_fps`].
    Adaptive,
}

/// Frame rates accepted as the target of adaptive frame generation.
pub const FRAME_GEN_TARGET_RANGE: (u32, u32) = (30, 240);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameGenSettings {
    pub method: FrameGenMethod,
    /// Only Lossless Scaling has a choice of rate.
    pub rate: FrameGenRate,
    /// For [`FrameGenRate::Adaptive`], within [`FRAME_GEN_TARGET_RANGE`].
    pub target_fps: u32,
}

impl Default for FrameGenSettings {
    fn default() -> Self {
        Self {
            method: FrameGenMethod::None,
            rate: FrameGenRate::X2,
            target_fps: 60,
        }
    }
}

impl Profile {
    /// What the image settings of this profile ask for that nothing applies
    /// yet, one line each, for the log.
    pub fn unapplied_image(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.upscaling.method != UpscalingMethod::None {
            lines.push(format!(
                "image: upscaling {} not applied yet",
                self.upscaling.method.name()
            ));
        }
        if self.frame_gen.method != FrameGenMethod::None {
            lines.push(format!(
                "image: frame generation {} not applied yet",
                self.frame_gen.method.name()
            ));
        }
        lines
    }
}

impl Profile {
    pub fn matches(&self, exe: &str) -> bool {
        self.exe.eq_ignore_ascii_case(exe)
    }
}

/// How far the processor may boost above its base frequency, as the
/// power plan's boost mode numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuBoost {
    Disabled,
    Enabled,
    Aggressive,
}

impl CpuBoost {
    pub const ALL: [CpuBoost; 3] = [CpuBoost::Disabled, CpuBoost::Enabled, CpuBoost::Aggressive];

    /// The value the power plan stores.
    pub fn mode(self) -> u32 {
        match self {
            CpuBoost::Disabled => 0,
            CpuBoost::Enabled => 1,
            CpuBoost::Aggressive => 2,
        }
    }

    pub fn from_mode(mode: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.mode() == mode)
    }
}

/// Processor settings of the active Windows power plan, for plugged in and
/// on battery alike. A `None` leaves the setting as Windows has it (and
/// puts back what Canapé replaced).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuPower {
    pub boost: Option<CpuBoost>,
    /// Energy performance preference: 0 favours speed, 100 saving power.
    pub epp: Option<u32>,
    /// Highest frequency in MHz; `Some(0)` removes the limit.
    pub max_mhz: Option<u32>,
    /// Share of the cores that stay awake, in percent.
    pub min_cores: Option<u32>,
}

/// Energy performance preference, in percent.
pub const CPU_EPP_RANGE: (u32, u32) = (0, 100);
/// Frequency limits accepted, in MHz (0 removes the limit).
pub const CPU_MAX_MHZ_RANGE: (u32, u32) = (1000, 6000);
/// Share of the cores kept awake, in percent.
pub const CPU_MIN_CORES_RANGE: (u32, u32) = (0, 100);

impl CpuPower {
    /// True when every setting is left to Windows.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Why these settings cannot be applied, if they cannot.
    pub fn validate(&self) -> Result<(), String> {
        let check =
            |name: &str, value: Option<u32>, (low, high): (u32, u32), zero: bool| match value {
                Some(v) if !(low..=high).contains(&v) && !(zero && v == 0) => Err(format!(
                    "{name} not in {low}..={high}{}",
                    if zero { " (or 0)" } else { "" }
                )),
                _ => Ok(()),
            };
        check("energy preference", self.epp, CPU_EPP_RANGE, false)?;
        check("maximum frequency", self.max_mhz, CPU_MAX_MHZ_RANGE, true)?;
        check(
            "minimum active cores",
            self.min_cores,
            CPU_MIN_CORES_RANGE,
            false,
        )
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

/// Accent colour of the shell until the user picks one.
pub const DEFAULT_ACCENT: &str = "#ff3d5e";

/// Brightest level of the stick rings.
pub const LIGHTING_MAX_LEVEL: u8 = 3;

/// Light of the rings around the sticks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lighting {
    /// 0 off, 1 to [`LIGHTING_MAX_LEVEL`] brighter.
    pub level: u8,
    /// `#rrggbb`; `None` follows the accent colour.
    #[serde(default)]
    pub color: Option<String>,
    /// What the rings do with the colour.
    #[serde(default)]
    pub effect: LightingEffect,
    /// How fast the effect goes; unused when steady.
    #[serde(default)]
    pub speed: LightingSpeed,
}

/// An effect of the controller's own for the stick rings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingEffect {
    /// The colour, steady.
    #[default]
    Steady,
    /// The colour fading in and out.
    Breathing,
    /// Every colour in turn, both rings alike.
    ColorCycle,
    /// Every colour running around the rings.
    Rainbow,
    /// The colour flashing.
    Strobe,
}

impl LightingEffect {
    /// Every effect, in the order the shell offers them.
    pub const ALL: [LightingEffect; 5] = [
        Self::Steady,
        Self::Breathing,
        Self::ColorCycle,
        Self::Rainbow,
        Self::Strobe,
    ];

    /// Whether the effect shows the chosen colour.
    pub fn uses_color(self) -> bool {
        matches!(self, Self::Steady | Self::Breathing | Self::Strobe)
    }

    /// Whether the effect moves, so has a speed.
    pub fn moves(self) -> bool {
        self != Self::Steady
    }
}

/// Speed of a lighting effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingSpeed {
    Slow,
    #[default]
    Medium,
    Fast,
}

impl LightingSpeed {
    /// Every speed, slowest first.
    pub const ALL: [LightingSpeed; 3] = [Self::Slow, Self::Medium, Self::Fast];
}

impl Lighting {
    /// The same lighting with its colour in lower case, or why it is not
    /// valid.
    pub fn normalized(&self) -> Result<Self, String> {
        if self.level > LIGHTING_MAX_LEVEL {
            return Err(format!(
                "lighting level {} above {LIGHTING_MAX_LEVEL}",
                self.level
            ));
        }
        if let Some(color) = &self.color
            && !crate::protocol::is_hex_color(color)
        {
            return Err(format!("not a #RRGGBB colour: {color}"));
        }
        Ok(Self {
            color: self.color.as_ref().map(|c| c.to_ascii_lowercase()),
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_power_is_validated_and_defaults_to_untouched() {
        assert!(CpuPower::default().is_default());
        let ok = CpuPower {
            boost: Some(CpuBoost::Disabled),
            epp: Some(100),
            max_mhz: Some(0),
            min_cores: Some(100),
        };
        assert_eq!(ok.validate(), Ok(()));
        assert!(!ok.is_default());
        for bad in [
            CpuPower {
                epp: Some(101),
                ..CpuPower::default()
            },
            CpuPower {
                max_mhz: Some(500),
                ..CpuPower::default()
            },
            CpuPower {
                max_mhz: Some(9000),
                ..CpuPower::default()
            },
            CpuPower {
                min_cores: Some(101),
                ..CpuPower::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        for boost in CpuBoost::ALL {
            assert_eq!(CpuBoost::from_mode(boost.mode()), Some(boost));
        }
        assert_eq!(CpuBoost::from_mode(3), None);
        // A profile saved before CPU settings existed still decodes.
        let profile: Profile = serde_json::from_str(
            r#"{"exe":"a.exe","auto_tdp":{"enabled":false,"target_fps":60,"min_watts":7,"max_watts":25}}"#,
        )
        .unwrap();
        assert_eq!(profile.cpu_power, None);
    }

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
    fn older_profiles_have_no_image_settings() {
        let json = r#"{"exe":"game.exe","auto_tdp":{"enabled":true,"target_fps":40,"min_watts":7,"max_watts":20},"refresh_hz":60}"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.resolution, None);
        assert_eq!(profile.upscaling, UpscalingSettings::default());
        assert_eq!(profile.frame_gen, FrameGenSettings::default());
        assert!(profile.unapplied_image().is_empty());
    }

    #[test]
    fn image_settings_round_trip() {
        let profile = Profile {
            exe: "game.exe".into(),
            resolution: Some(Resolution {
                width: 1600,
                height: 900,
            }),
            upscaling: UpscalingSettings {
                method: UpscalingMethod::AmdRsr,
                sharpness: 70,
            },
            frame_gen: FrameGenSettings {
                method: FrameGenMethod::LosslessScaling,
                rate: FrameGenRate::Adaptive,
                target_fps: 90,
            },
            ..Profile::default()
        };
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains(r#""method":"amd_rsr""#));
        assert_eq!(serde_json::from_str::<Profile>(&json).unwrap(), profile);
        assert_eq!(
            profile.unapplied_image(),
            [
                "image: upscaling AMD (RSR) not applied yet",
                "image: frame generation Lossless Scaling not applied yet"
            ]
        );
        assert_eq!(profile.resolution.unwrap().to_string(), "1600×900");
        // Partial image blocks take their defaults.
        let partial: UpscalingSettings = serde_json::from_str(r#"{"method":"amd_rsr"}"#).unwrap();
        assert_eq!(partial.sharpness, 50);
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
    fn lighting_is_checked() {
        let light = |level, color: Option<&str>| Lighting {
            level,
            color: color.map(str::to_owned),
            ..Lighting::default()
        };
        assert_eq!(
            light(2, Some("#FF8800")).normalized(),
            Ok(light(2, Some("#ff8800")))
        );
        assert_eq!(light(0, None).normalized(), Ok(light(0, None)));
        assert!(light(4, None).normalized().is_err());
        assert!(light(1, Some("orange")).normalized().is_err());
        // Saved before effects existed: steady, medium speed.
        let old: Lighting = serde_json::from_str(r##"{"level":2,"color":"#ff0000"}"##).unwrap();
        assert_eq!(old.effect, LightingEffect::Steady);
        assert_eq!(old.speed, LightingSpeed::Medium);
        let breathing = Lighting {
            effect: LightingEffect::ColorCycle,
            speed: LightingSpeed::Fast,
            ..light(3, None)
        };
        let json = serde_json::to_string(&breathing).unwrap();
        assert!(json.contains(r#""effect":"color_cycle""#), "{json}");
        assert!(json.contains(r#""speed":"fast""#), "{json}");
        assert_eq!(breathing.normalized(), Ok(breathing.clone()));
        assert!(!LightingEffect::Rainbow.uses_color());
        assert!(!LightingEffect::Steady.moves());
    }

    #[test]
    fn controller_modes_in_json() {
        assert_eq!(
            serde_json::to_string(&ControllerMode::Wasd).unwrap(),
            r#""wasd""#
        );
    }
}
