use canape_autotdp::{Config, Controller};
use canape_core::{AutoTdpSettings, PerformanceMode, Request, Response, Status, protocol};
use canape_hw::Device;
use serde::{Deserialize, Serialize};

/// Below this frame rate nothing is being played (desktop, idle shell).
const MIN_GAME_FPS: f32 = 5.0;
const MIN_GAME_GPU: f32 = 0.15;

/// What the service re-applies at startup: the firmware cannot report the
/// current mode or power limits back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub mode: Option<PerformanceMode>,
    pub tdp_watts: Option<u32>,
    pub auto_tdp: AutoTdpSettings,
    pub battery_limit: Option<u8>,
}

/// Service state and request dispatch, independent of the transport.
pub struct Handler {
    device: Box<dyn Device>,
    tdp_watts: Option<u32>,
    mode: Option<PerformanceMode>,
    auto_tdp: AutoTdpSettings,
    battery_limit: Option<u8>,
    controller: Option<Controller>,
    fps: Option<f32>,
    conflicts: Vec<String>,
}

impl Handler {
    pub fn new(device: Box<dyn Device>) -> Self {
        Self {
            device,
            tdp_watts: None,
            mode: None,
            auto_tdp: AutoTdpSettings::default(),
            battery_limit: None,
            controller: None,
            fps: None,
            conflicts: Vec::new(),
        }
    }

    pub fn handle(&mut self, request: Request) -> Response {
        match request {
            Request::Hello { protocol_version }
                if protocol_version == protocol::PROTOCOL_VERSION =>
            {
                Response::Hello { protocol_version }
            }
            Request::Hello { protocol_version } => Response::Error {
                message: format!(
                    "unsupported protocol version {protocol_version}, expected {}",
                    protocol::PROTOCOL_VERSION
                ),
            },
            Request::GetStatus | Request::Subscribe => Response::Status(self.status()),
            Request::SetPerformanceMode { mode } => {
                self.disable_auto_tdp();
                self.apply(
                    |d| d.set_performance_mode(mode),
                    |s| {
                        s.mode = Some(mode);
                        // The firmware applies the mode's own power limits.
                        if mode != PerformanceMode::Manual {
                            s.tdp_watts = None;
                        }
                    },
                )
            }
            Request::SetTdp { watts } => {
                self.disable_auto_tdp();
                self.apply(
                    |d| d.set_tdp(watts),
                    |s| {
                        s.tdp_watts = Some(watts);
                        s.mode = Some(PerformanceMode::Manual);
                    },
                )
            }
            Request::SetAutoTdp { settings } => self.set_auto_tdp(settings),
            Request::SetBatteryChargeLimit { percent } => self.apply(
                |d| d.set_battery_charge_limit(percent),
                |s| s.battery_limit = Some(percent),
            ),
        }
    }

    /// One tick: samples the frame rate and, when a game is running, lets
    /// auto TDP adjust the power.
    pub fn tick(&mut self) {
        self.fps = self.device.frame_rate();
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        let Some(fps) = self.fps.filter(|&fps| fps >= MIN_GAME_FPS) else {
            return;
        };
        let gpu_util = self.device.telemetry().ok().and_then(|t| t.gpu_util);
        if gpu_util.is_some_and(|util| util < MIN_GAME_GPU) {
            return;
        }
        if let Some(watts) = controller.update(canape_autotdp::Sample { fps, gpu_util })
            && self.device.set_tdp(watts).is_ok()
        {
            self.tdp_watts = Some(watts);
        }
    }

    pub fn status(&mut self) -> Status {
        let telemetry = self.device.telemetry().unwrap_or_default();
        Status {
            tdp_watts: self.tdp_watts,
            performance_mode: self.mode,
            auto_tdp: self.auto_tdp,
            fps: self.fps,
            gpu_util: telemetry.gpu_util,
            cpu_temp_c: telemetry.cpu_temp_c,
            battery_percent: telemetry.battery_percent,
            conflicts: self.conflicts.clone(),
        }
    }

    /// Returns true when the list changed.
    pub fn set_conflicts(&mut self, conflicts: Vec<String>) -> bool {
        let changed = conflicts != self.conflicts;
        self.conflicts = conflicts;
        changed
    }

    pub fn saved(&self) -> Saved {
        Saved {
            mode: self.mode,
            tdp_watts: self.tdp_watts,
            auto_tdp: self.auto_tdp,
            battery_limit: self.battery_limit,
        }
    }

    /// Re-applies a saved state; returns the errors, if any.
    pub fn restore(&mut self, saved: &Saved) -> Vec<String> {
        let mut requests = Vec::new();
        if let Some(percent) = saved.battery_limit {
            requests.push(Request::SetBatteryChargeLimit { percent });
        }
        match (saved.mode, saved.tdp_watts) {
            (Some(PerformanceMode::Manual), Some(watts)) => {
                requests.push(Request::SetTdp { watts })
            }
            (Some(mode), _) if mode != PerformanceMode::Manual => {
                requests.push(Request::SetPerformanceMode { mode })
            }
            _ => {}
        }
        if saved.auto_tdp.enabled {
            requests.push(Request::SetAutoTdp {
                settings: saved.auto_tdp,
            });
        }
        requests
            .into_iter()
            .filter_map(|request| match self.handle(request) {
                Response::Error { message } => Some(message),
                _ => None,
            })
            .collect()
    }

    fn set_auto_tdp(&mut self, settings: AutoTdpSettings) -> Response {
        if !settings.enabled {
            self.disable_auto_tdp();
            return Response::Ok;
        }
        let (dev_min, dev_max) = self.device.tdp_range();
        let (min_watts, max_watts) = (
            settings.min_watts.max(dev_min),
            settings.max_watts.min(dev_max),
        );
        if min_watts > max_watts || settings.target_fps == 0 {
            return Response::Error {
                message: "invalid auto TDP settings".into(),
            };
        }
        let config = Config {
            target_fps: settings.target_fps as f32,
            min_watts,
            max_watts,
            ..Config::default()
        };
        let start = self.tdp_watts.unwrap_or(max_watts);
        self.controller = Some(Controller::new(config, start));
        self.auto_tdp = settings;
        self.mode = Some(PerformanceMode::Manual);
        Response::Ok
    }

    fn disable_auto_tdp(&mut self) {
        self.controller = None;
        self.auto_tdp.enabled = false;
    }

    fn apply(
        &mut self,
        write: impl FnOnce(&mut dyn Device) -> canape_hw::Result<()>,
        record: impl FnOnce(&mut Self),
    ) -> Response {
        match write(self.device.as_mut()) {
            Ok(()) => {
                record(self);
                Response::Ok
            }
            Err(err) => Response::Error {
                message: err.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_hw::mock::MockDevice;

    fn handler_with(fps: Option<f32>, gpu_util: Option<f32>) -> Handler {
        Handler::new(Box::new(MockDevice {
            fps,
            gpu_util,
            ..MockDevice::default()
        }))
    }

    fn handler() -> Handler {
        handler_with(None, Some(0.7))
    }

    fn auto_on() -> Request {
        Request::SetAutoTdp {
            settings: AutoTdpSettings {
                enabled: true,
                ..AutoTdpSettings::default()
            },
        }
    }

    #[test]
    fn rejects_unknown_protocol_version() {
        let response = handler().handle(Request::Hello {
            protocol_version: 99,
        });
        assert!(matches!(response, Response::Error { .. }));
    }

    #[test]
    fn manual_tdp_shows_in_status() {
        let mut h = handler();
        assert_eq!(h.handle(Request::SetTdp { watts: 12 }), Response::Ok);
        let Response::Status(status) = h.handle(Request::GetStatus) else {
            panic!("expected status");
        };
        assert_eq!(status.tdp_watts, Some(12));
        assert_eq!(status.performance_mode, Some(PerformanceMode::Manual));
    }

    #[test]
    fn auto_tdp_lowers_power_on_an_easy_game() {
        let mut h = handler_with(Some(120.0), Some(0.7));
        assert_eq!(h.handle(auto_on()), Response::Ok);
        for _ in 0..3 {
            h.tick();
        }
        assert_eq!(h.status().tdp_watts, Some(24));
        assert_eq!(h.status().fps, Some(120.0));
    }

    #[test]
    fn auto_tdp_holds_when_nothing_is_played() {
        for (fps, gpu) in [
            (None, Some(0.7)),
            (Some(2.0), Some(0.7)),
            (Some(30.0), Some(0.05)),
        ] {
            let mut h = handler_with(fps, gpu);
            h.handle(Request::SetTdp { watts: 12 });
            h.handle(auto_on());
            for _ in 0..5 {
                h.tick();
            }
            assert_eq!(h.status().tdp_watts, Some(12), "fps {fps:?}, gpu {gpu:?}");
        }
    }

    #[test]
    fn manual_tdp_disables_auto_tdp() {
        let mut h = handler_with(Some(120.0), Some(0.7));
        h.handle(auto_on());
        h.handle(Request::SetTdp { watts: 10 });
        assert!(!h.status().auto_tdp.enabled);
        h.tick();
        assert_eq!(h.status().tdp_watts, Some(10));
    }

    #[test]
    fn saved_state_round_trips() {
        let mut h = handler();
        h.handle(Request::SetBatteryChargeLimit { percent: 80 });
        h.handle(Request::SetTdp { watts: 11 });
        h.handle(auto_on());
        let saved = h.saved();

        let mut restored = handler();
        assert!(restored.restore(&saved).is_empty());
        assert_eq!(restored.saved(), saved);
    }

    #[test]
    fn restores_a_firmware_mode() {
        let mut h = handler();
        h.handle(Request::SetPerformanceMode {
            mode: PerformanceMode::Turbo,
        });
        let mut restored = handler();
        assert!(restored.restore(&h.saved()).is_empty());
        assert_eq!(
            restored.status().performance_mode,
            Some(PerformanceMode::Turbo)
        );
        assert_eq!(restored.status().tdp_watts, None);
    }
}
