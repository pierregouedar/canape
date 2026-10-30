//! Types shared by every Canape component: the IPC protocol spoken over
//! `\\.\pipe\canape` and the user-facing settings.

pub mod bench;
pub mod conflicts;
pub mod fan;
pub mod log;
pub mod protocol;
pub mod settings;
pub mod tweaks;

pub use bench::{BenchPlan, BenchPoint, BenchProgress, BenchReport};
pub use fan::{Fan, FanCurve, FanCurves, FanPreset};
pub use protocol::{ProfilePart, Request, Response, Status};
pub use settings::{
    AmdSettings, AutoTdpSettings, BackButton, BackButtons, ButtonTarget, ChillRange,
    ControllerMode, CpuBoost, CpuPower, FrameGenMethod, FrameGenRate, FrameGenSettings,
    GyroActivation, GyroAxis, GyroSettings, Lighting, LightingEffect, LightingSpeed,
    PerformanceMode, Profile, Resolution, UpscalingMethod, UpscalingSettings,
};
pub use tweaks::{TweakGroup, TweakId, TweakState, Unavailable};
