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
    /// Lower ceiling while running on battery.
    #[serde(default)]
    pub battery_max_watts: Option<u32>,
    /// Above this CPU temperature, power only goes down.
    #[serde(default)]
    pub max_temp_c: Option<u32>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Executable name, compared case-insensitively (for example `eldenring.exe`).
    pub exe: String,
    pub auto_tdp: AutoTdpSettings,
    /// Wattage auto TDP last settled on for this game: the next session starts there.
    #[serde(default)]
    pub learned_watts: Option<u32>,
}

impl Profile {
    pub fn matches(&self, exe: &str) -> bool {
        self.exe.eq_ignore_ascii_case(exe)
    }
}
