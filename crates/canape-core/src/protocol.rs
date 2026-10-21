//! IPC protocol between the service and its clients (FSE shell, Game Bar widget).
//!
//! Transport: one JSON object per line over the `\\.\pipe\canape` named pipe.
//! Every client starts with [`Request::Hello`].

use serde::{Deserialize, Serialize};

use crate::settings::{AutoTdpSettings, PerformanceMode};

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Hello { protocol_version: u32 },
    Status(Status),
    Ok,
    Error { message: String },
}

/// Snapshot of the device state, as shown in the shell and the widget.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Status {
    pub tdp_watts: Option<u32>,
    pub performance_mode: Option<PerformanceMode>,
    pub auto_tdp: AutoTdpSettings,
    pub fps: Option<f32>,
    pub cpu_temp_c: Option<f32>,
    pub battery_percent: Option<u8>,
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
