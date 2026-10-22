//! IPC protocol between the service and its clients (FSE shell, Game Bar widget).
//!
//! Transport: one JSON object per line over the `\\.\pipe\canape` named pipe.
//! Every client starts with [`Request::Hello`].

use serde::{Deserialize, Serialize};

use crate::bench::{BenchPlan, BenchProgress, BenchReport};
use crate::fan::{Fan, FanCurve, FanCurves};
use crate::settings::{AutoTdpSettings, ControllerMode, PerformanceMode, Profile};

pub const PIPE_NAME: &str = r"\\.\pipe\canape";
pub const PROTOCOL_VERSION: u32 = 1;

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
    /// Frame rate cap of the GPU driver (AMD), `None` to remove it.
    SetFpsLimit {
        fps: Option<u32>,
    },
    /// Mode of the built-in controller.
    SetControllerMode {
        mode: ControllerMode,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
    /// Other programs that also drive the hardware (Winhanced, Armoury Crate…).
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
    /// Frame rate cap set through Canape, if any.
    #[serde(default)]
    pub fps_limit: Option<u32>,
    #[serde(default)]
    pub controller_mode: Option<ControllerMode>,
}

/// True for `#RRGGBB`.
pub fn is_hex_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
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
    fn hex_colors() {
        assert!(is_hex_color("#2563eb"));
        assert!(!is_hex_color("2563eb"));
        assert!(!is_hex_color("#25g3eb"));
        assert!(!is_hex_color("#fff"));
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
            ..Status::default()
        });
        let line = encode(&response).unwrap();
        assert_eq!(decode::<Response>(&line).unwrap(), response);
    }
}
