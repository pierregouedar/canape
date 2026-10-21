//! Types shared by every Canape component: the IPC protocol spoken over
//! `\\.\pipe\canape` and the user-facing settings.

pub mod log;
pub mod protocol;
pub mod settings;

pub use protocol::{Request, Response, Status};
pub use settings::{AutoTdpSettings, PerformanceMode};
