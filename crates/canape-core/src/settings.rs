use serde::{Deserialize, Serialize};

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
}

impl Default for AutoTdpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            target_fps: 60,
            min_watts: 7,
            max_watts: 25,
        }
    }
}
