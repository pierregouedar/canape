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
    AutoTdpSettings, BackButton, BackButtons, ButtonTarget, ControllerMode, GyroActivation,
    GyroAxis, GyroSettings, Lighting, LightingEffect, LightingSpeed, PerformanceMode, Profile,
};
pub use tweaks::{TweakGroup, TweakId, TweakState, Unavailable};
