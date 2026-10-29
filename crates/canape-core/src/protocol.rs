//! IPC protocol between the service and its clients (FSE shell, Game Bar widget).
//!
//! Transport: one JSON object per line over the `\\.\pipe\canape` named pipe.
//! Every client starts with [`Request::Hello`].

use serde::{Deserialize, Serialize};

use crate::bench::{BenchPlan, BenchProgress, BenchReport};
use crate::fan::{Fan, FanCurve, FanCurves, FanPreset};
use crate::settings::{
    AutoTdpSettings, BackButton, BackButtons, ButtonTarget, ControllerMode, CpuPower, GyroSettings,
    Lighting, PerformanceMode, Profile,
};
use crate::tweaks::{TweakId, TweakState};

pub const PIPE_NAME: &str = r"\\.\pipe\canape";
pub const PROTOCOL_VERSION: u32 = 1;

/// A [`Request::UiForeground`] report older than this no longer counts.
pub const UI_FOREGROUND_TIMEOUT_SECS: u64 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello {
        protocol_version: u32,
    },
    GetStatus,
    /// Push a [`Response::Status`] about once per second until the client disconnects.
    /// The connection is push-only afterwards: the service reads no more requests on it.
    Subscribe,
    SetPerformanceMode {
        mode: PerformanceMode,
    },
    /// Manual TDP; disables auto TDP.
    SetTdp {
        watts: u32,
    },
    SetAutoTdp {
        settings: AutoTdpSettings,
    },
    SetBatteryChargeLimit {
        percent: u8,
    },
    /// Sent by the shell (user session) when the foreground program changes:
    /// the service, in session 0, cannot see the user's windows.
    ForegroundApp {
        exe: Option<String>,
    },
    /// Creates or replaces the profile for `profile.exe`.
    SaveProfile {
        profile: Profile,
    },
    DeleteProfile {
        exe: String,
    },
    /// Answered with [`Response::Profiles`].
    ListProfiles,
    /// Sweeps power limits on the running game; progress shows in the status.
    StartBench {
        plan: BenchPlan,
    },
    CancelBench,
    /// Answered with [`Response::Bench`]: the last run, finished or not.
    GetBenchReport,
    /// UI accent colour as `#RRGGBB`, shared by the shell and the widget.
    SetAccent {
        color: String,
    },
    /// Custom curve for one fan, validated by [`FanCurve::validate`]. Kept
    /// across performance mode changes (the service writes it again).
    SetFanCurve {
        fan: Fan,
        curve: FanCurve,
    },
    /// Every fan back to the performance mode's own curve.
    ResetFanCurves,
    /// Answered with [`Response::FanCurves`].
    GetFanCurves,
    /// One of the ready-made curves on the CPU and GPU fans, `None` for the
    /// firmware's own curves.
    SetFanPreset {
        preset: Option<FanPreset>,
    },
    /// Saves part of what is in effect into the profile of the game in
    /// front, created from the current auto TDP if it has none.
    SaveForegroundProfile {
        part: ProfilePart,
    },
    /// Frame rate cap of the GPU driver (AMD), `None` to remove it.
    SetFpsLimit {
        fps: Option<u32>,
    },
    /// Mode of the built-in controller.
    SetControllerMode {
        mode: ControllerMode,
    },
    /// Gyro aiming settings. The service stores them (globally, or in the
    /// active profile when it has its own) and pushes them in the status;
    /// the shell reads the sensor and moves the mouse.
    SetGyro {
        settings: GyroSettings,
    },
    /// What one back button sends. Saved and written again at startup and
    /// after every controller mode change.
    SetBackButton {
        button: BackButton,
        target: ButtonTarget,
    },
    /// Both back buttons back to their own keys.
    ResetBackButtons,
    /// Light of the stick rings, `None` to leave it to the controller.
    /// Saved and written again at startup; a ring that follows the accent
    /// changes with it.
    SetLighting {
        lighting: Option<Lighting>,
    },
    /// Processor settings of the Windows power plan (boost, energy
    /// preference, frequency limit, awake cores). Settings left `None` go
    /// back to what Windows had. Saved, and written again at startup; they
    /// belong to the active profile when it has its own.
    SetCpuPower {
        settings: CpuPower,
    },
    /// Answered with [`Response::Tweaks`]: every Windows tweak of the
    /// catalogue and its state.
    ListTweaks,
    /// Applies (`on`) or reverts one tweak of the catalogue. Reverting puts
    /// back what Canapé replaced, or the Windows default when it did not
    /// apply the tweak itself.
    SetTweak {
        id: TweakId,
        on: bool,
    },
    /// Reverts every tweak Canapé applied (before uninstalling).
    RevertTweaks,
    /// Sent by the shell about once a second: whether its window is the
    /// foreground one. The service gives the interface full power while it
    /// is, and stops after [`UI_FOREGROUND_TIMEOUT_SECS`] without a report
    /// (the shell closed, hung or lost its connection).
    UiForeground {
        front: bool,
    },
    /// Turns "full power in Canapé" on or off.
    SetUiPower {
        enabled: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(
    clippy::large_enum_variant,
    reason = "one response per message, encoded at once: the status is not worth a box"
)]
pub enum Response {
    Hello {
        protocol_version: u32,
    },
    Status(Status),
    Ok,
    Error {
        message: String,
    },
    Profiles {
        profiles: Vec<Profile>,
    },
    Bench {
        report: Option<BenchReport>,
    },
    /// The custom curves in effect, and the firmware's own curves for the
    /// current performance mode (a starting point for editing; a fan the
    /// firmware does not report is `null`).
    FanCurves {
        custom: FanCurves,
        defaults: FanCurves,
    },
    /// The catalogue in display order.
    Tweaks {
        tweaks: Vec<TweakState>,
    },
}

/// Snapshot of the device state, as shown in the shell and the widget.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Status {
    pub tdp_watts: Option<u32>,
    pub performance_mode: Option<PerformanceMode>,
    pub auto_tdp: AutoTdpSettings,
    pub fps: Option<f32>,
    /// GPU activity in `[0, 1]`.
    #[serde(default)]
    pub gpu_util: Option<f32>,
    /// What the APU actually draws, in watts (the TDP is only its limit).
    #[serde(default)]
    pub power_watts: Option<f32>,
    pub cpu_temp_c: Option<f32>,
    pub battery_percent: Option<u8>,
    /// Other running tools that write the same firmware settings.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Foreground program reported by the shell.
    #[serde(default)]
    pub foreground: Option<String>,
    /// Executable of the profile in use, if any.
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub on_battery: Option<bool>,
    /// Auto TDP is holding power down because of the temperature limit.
    #[serde(default)]
    pub thermal_limited: bool,
    #[serde(default)]
    pub bench: Option<BenchProgress>,
    /// UI accent colour as `#RRGGBB`, when the user picked one.
    #[serde(default)]
    pub accent: Option<String>,
    /// Sustained power range of the model, in watts: the bounds of a TDP
    /// stepper.
    #[serde(default)]
    pub tdp_min_watts: Option<u32>,
    #[serde(default)]
    pub tdp_max_watts: Option<u32>,
    #[serde(default)]
    pub cpu_fan_rpm: Option<u32>,
    #[serde(default)]
    pub gpu_fan_rpm: Option<u32>,
    /// Custom fan curves in effect; empty when the firmware's apply.
    #[serde(default)]
    pub fan_curves: FanCurves,
    /// The preset those curves are, if any.
    #[serde(default)]
    pub fan_preset: Option<FanPreset>,
    /// Frame rate cap set through Canape, if any.
    #[serde(default)]
    pub fps_limit: Option<u32>,
    #[serde(default)]
    pub controller_mode: Option<ControllerMode>,
    /// Battery charge limit set through Canape, in percent, if any.
    #[serde(default)]
    pub battery_charge_limit: Option<u8>,
    /// Gyro aiming in effect: the global settings, or the active profile's.
    #[serde(default)]
    pub gyro: GyroSettings,
    /// What the back buttons send.
    #[serde(default)]
    pub back_buttons: BackButtons,
    /// Light of the stick rings set through Canape, if any.
    #[serde(default)]
    pub lighting: Option<Lighting>,
    /// Processor power settings in effect: the global ones, or the active
    /// profile's.
    #[serde(default)]
    pub cpu_power: CpuPower,
    /// "Full power in Canapé" is on.
    #[serde(default = "default_true")]
    pub ui_power_enabled: bool,
    /// The interface power level is in effect right now: the performance
    /// mode's limits are written, whatever the mode and limits shown above.
    #[serde(default)]
    pub ui_power: bool,
}

fn default_true() -> bool {
    true
}

/// What [`Request::SaveForegroundProfile`] saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePart {
    /// Auto TDP and the wattage it holds as the learned one, with the
    /// processor power settings.
    Power,
    /// Fan curves, frame rate cap and gyro aiming.
    Hardware,
}

/// True for `#RRGGBB`.
pub fn is_hex_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Red, green and blue of `#RRGGBB`.
pub fn hex_rgb(color: &str) -> Option<[u8; 3]> {
    if !is_hex_color(color) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&color[i..i + 2], 16).ok();
    Some([channel(1)?, channel(3)?, channel(5)?])
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("invalid message: {0}")]
    Json(#[from] serde_json::Error),
}

/// Encodes a message as one line of JSON, newline included.
pub fn encode<T: Serialize>(message: &T) -> Result<String, CodecError> {
    let mut line = serde_json::to_string(message)?;
    line.push('\n');
    Ok(line)
}

pub fn decode<'a, T: Deserialize<'a>>(line: &'a str) -> Result<T, CodecError> {
    Ok(serde_json::from_str(line.trim_end())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_as_one_line() {
        let request = Request::SetTdp { watts: 15 };
        let line = encode(&request).unwrap();
        assert_eq!(line, "{\"type\":\"set_tdp\",\"watts\":15}\n");
        assert_eq!(decode::<Request>(&line).unwrap(), request);
    }

    #[test]
    fn tweak_requests_round_trip() {
        let request = Request::SetTweak {
            id: TweakId::CopilotOff,
            on: true,
        };
        let line = encode(&request).unwrap();
        assert_eq!(
            line,
            "{\"type\":\"set_tweak\",\"id\":\"copilot_off\",\"on\":true}\n"
        );
        assert_eq!(decode::<Request>(&line).unwrap(), request);
        assert!(decode::<Request>(r#"{"type":"set_tweak","id":"rm_rf","on":true}"#).is_err());
        assert_eq!(
            decode::<Request>(r#"{"type":"revert_tweaks"}"#).unwrap(),
            Request::RevertTweaks
        );
        let response = Response::Tweaks {
            tweaks: vec![TweakState {
                id: TweakId::DefenderRealtimeOff,
                applied: false,
                by_canape: false,
                unavailable: Some(crate::Unavailable::TamperProtection),
                reboot_pending: false,
                paths: Vec::new(),
            }],
        };
        let line = encode(&response).unwrap();
        assert!(line.contains("\"unavailable\":\"tamper_protection\""));
        assert_eq!(decode::<Response>(&line).unwrap(), response);
        // Optional fields may be left out.
        let Response::Tweaks { tweaks } =
            decode(r#"{"type":"tweaks","tweaks":[{"id":"game_mode_on","applied":true}]}"#).unwrap()
        else {
            panic!("expected tweaks");
        };
        assert!(tweaks[0].applied && !tweaks[0].by_canape && tweaks[0].paths.is_empty());
    }

    #[test]
    fn ui_power_requests_and_status() {
        assert_eq!(
            encode(&Request::UiForeground { front: true }).unwrap(),
            "{\"type\":\"ui_foreground\",\"front\":true}\n"
        );
        assert_eq!(
            decode::<Request>(r#"{"type":"set_ui_power","enabled":false}"#).unwrap(),
            Request::SetUiPower { enabled: false }
        );
    }

    #[test]
    fn hex_colors() {
        assert!(is_hex_color("#2563eb"));
        assert!(!is_hex_color("2563eb"));
        assert!(!is_hex_color("#25g3eb"));
        assert!(!is_hex_color("#fff"));
        assert_eq!(hex_rgb("#FF3d00"), Some([255, 61, 0]));
        assert_eq!(hex_rgb("#ff3d0"), None);
    }

    #[test]
    fn new_requests_in_json() {
        let request: Request = decode(
            r#"{"type":"set_fan_curve","fan":"cpu","curve":[[30,0],[40,10],[50,20],[60,30],[70,40],[80,50],[90,60],[100,70]]}"#,
        )
        .unwrap();
        let Request::SetFanCurve { fan, curve } = request else {
            panic!("expected set_fan_curve");
        };
        assert_eq!(fan, Fan::Cpu);
        assert_eq!(curve.0[7], (100, 70));
        assert_eq!(
            decode::<Request>(r#"{"type":"set_fps_limit","fps":null}"#).unwrap(),
            Request::SetFpsLimit { fps: None }
        );
        assert_eq!(
            decode::<Request>(r#"{"type":"set_controller_mode","mode":"mouse"}"#).unwrap(),
            Request::SetControllerMode {
                mode: ControllerMode::Mouse
            }
        );
        assert_eq!(
            decode::<Request>(
                r#"{"type":"set_gyro","settings":{"activation":"toggle","sensitivity":20,"horizontal":"roll","invert_y":true}}"#
            )
            .unwrap(),
            Request::SetGyro {
                settings: GyroSettings {
                    activation: crate::GyroActivation::Toggle,
                    sensitivity: 20,
                    horizontal: crate::GyroAxis::Roll,
                    invert_y: true,
                    ..GyroSettings::default()
                }
            }
        );
        assert_eq!(
            decode::<Request>(r#"{"type":"set_back_button","button":"m1","target":"pad_a"}"#)
                .unwrap(),
            Request::SetBackButton {
                button: BackButton::M1,
                target: ButtonTarget::PadA
            }
        );
        assert_eq!(
            decode::<Request>(r#"{"type":"set_lighting","lighting":{"level":2}}"#).unwrap(),
            Request::SetLighting {
                lighting: Some(Lighting {
                    level: 2,
                    ..Lighting::default()
                })
            }
        );
        assert_eq!(
            encode(&Request::ResetFanCurves).unwrap(),
            "{\"type\":\"reset_fan_curves\"}\n"
        );
    }

    #[test]
    fn older_status_still_decodes() {
        let line = r#"{"type":"status","tdp_watts":12,"performance_mode":"manual","auto_tdp":{"enabled":false,"target_fps":60,"min_watts":7,"max_watts":25},"fps":null,"cpu_temp_c":50.0,"battery_percent":80}"#;
        let Response::Status(status) = decode::<Response>(line).unwrap() else {
            panic!("expected status");
        };
        assert_eq!(status.tdp_min_watts, None);
        assert!(status.fan_curves.is_empty());
        assert_eq!(status.controller_mode, None);
        assert_eq!(status.battery_charge_limit, None);
        assert_eq!(status.gyro, GyroSettings::default());
        assert!(status.back_buttons.is_default());
        assert_eq!(status.lighting, None);
        assert!(status.ui_power_enabled && !status.ui_power);
        assert!(status.cpu_power.is_default());
    }

    #[test]
    fn cpu_power_request_in_json() {
        let request: Request = decode(
            r#"{"type":"set_cpu_power","settings":{"boost":"disabled","epp":80,"max_mhz":0}}"#,
        )
        .unwrap();
        assert_eq!(
            request,
            Request::SetCpuPower {
                settings: CpuPower {
                    boost: Some(crate::CpuBoost::Disabled),
                    epp: Some(80),
                    max_mhz: Some(0),
                    min_cores: None,
                }
            }
        );
        assert_eq!(
            decode::<Request>(&encode(&request).unwrap()).unwrap(),
            request
        );
    }

    #[test]
    fn status_round_trips() {
        let response = Response::Status(Status {
            tdp_watts: Some(12),
            fps: Some(59.8),
            tdp_min_watts: Some(5),
            tdp_max_watts: Some(30),
            cpu_fan_rpm: Some(3200),
            fps_limit: Some(40),
            controller_mode: Some(ControllerMode::Gamepad),
            battery_charge_limit: Some(80),
            ..Status::default()
        });
        let line = encode(&response).unwrap();
        assert_eq!(decode::<Response>(&line).unwrap(), response);
    }
}
