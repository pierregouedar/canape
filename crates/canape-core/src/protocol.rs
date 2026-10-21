//! IPC protocol between the service and its clients (FSE shell, Game Bar widget).
//!
//! Transport: one JSON object per line over the `\\.\pipe\canape` named pipe.
//! Every client starts with [`Request::Hello`].

use serde::{Deserialize, Serialize};

use crate::bench::{BenchPlan, BenchProgress, BenchReport};
use crate::settings::{AutoTdpSettings, PerformanceMode, Profile};

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Hello { protocol_version: u32 },
    Status(Status),
    Ok,
    Error { message: String },
    Profiles { profiles: Vec<Profile> },
    Bench { report: Option<BenchReport> },
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
    fn status_round_trips() {
        let response = Response::Status(Status {
            tdp_watts: Some(12),
            fps: Some(59.8),
            ..Status::default()
        });
        let line = encode(&response).unwrap();
        assert_eq!(decode::<Response>(&line).unwrap(), response);
    }
}
