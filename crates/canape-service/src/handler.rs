use canape_autotdp::{Config, Controller};
use canape_core::{AutoTdpSettings, PerformanceMode, Request, Response, Status, protocol};
use canape_hw::Device;

/// Service state and request dispatch, independent of the transport.
pub struct Handler {
    device: Box<dyn Device>,
    tdp_watts: Option<u32>,
    mode: Option<PerformanceMode>,
    auto_tdp: AutoTdpSettings,
    controller: Option<Controller>,
}

impl Handler {
    pub fn new(device: Box<dyn Device>) -> Self {
        Self {
            device,
            tdp_watts: None,
            mode: None,
            auto_tdp: AutoTdpSettings::default(),
            controller: None,
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
                self.apply(|d| d.set_performance_mode(mode), |s| s.mode = Some(mode))
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
            Request::SetBatteryChargeLimit { percent } => {
                self.apply(|d| d.set_battery_charge_limit(percent), |_| {})
            }
        }
    }

    /// One auto TDP tick, fed with the foreground game's frame rate.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "driven by the ETW telemetry loop in M4")
    )]
    pub fn tick(&mut self, fps: Option<f32>) {
        let (Some(controller), Some(fps)) = (self.controller.as_mut(), fps) else {
            return;
        };
        let gpu_util = self.device.telemetry().ok().and_then(|t| t.gpu_util);
        if let Some(watts) = controller.update(canape_autotdp::Sample { fps, gpu_util }) {
            if self.device.set_tdp(watts).is_ok() {
                self.tdp_watts = Some(watts);
            }
        }
    }

    pub fn status(&mut self) -> Status {
        let telemetry = self.device.telemetry().unwrap_or_default();
        Status {
            tdp_watts: self.tdp_watts,
            performance_mode: self.mode,
            auto_tdp: self.auto_tdp,
            fps: self.controller.as_ref().and_then(Controller::smoothed_fps),
            cpu_temp_c: telemetry.cpu_temp_c,
            battery_percent: telemetry.battery_percent,
        }
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

    fn handler() -> Handler {
        Handler::new(Box::new(MockDevice::default()))
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
        let mut h = handler();
        let settings = AutoTdpSettings {
            enabled: true,
            ..AutoTdpSettings::default()
        };
        assert_eq!(h.handle(Request::SetAutoTdp { settings }), Response::Ok);
        for _ in 0..3 {
            h.tick(Some(120.0));
        }
        assert_eq!(h.status().tdp_watts, Some(24));
    }

    #[test]
    fn manual_tdp_disables_auto_tdp() {
        let mut h = handler();
        let settings = AutoTdpSettings {
            enabled: true,
            ..AutoTdpSettings::default()
        };
        h.handle(Request::SetAutoTdp { settings });
        h.handle(Request::SetTdp { watts: 10 });
        assert!(!h.status().auto_tdp.enabled);
        h.tick(Some(120.0));
        assert_eq!(h.status().tdp_watts, Some(10));
    }
}
