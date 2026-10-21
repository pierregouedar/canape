use std::time::{SystemTime, UNIX_EPOCH};

use canape_autotdp::{Config, Controller};
use canape_core::{
    AutoTdpSettings, BenchPlan, BenchReport, PerformanceMode, Profile, Request, Response, Status,
    protocol,
};
use canape_hw::{Device, Telemetry};
use serde::{Deserialize, Serialize};

use crate::bench_run::{BenchRun, Reading, Step};

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
    /// Global auto TDP settings, used when no game profile applies.
    pub auto_tdp: AutoTdpSettings,
    pub battery_limit: Option<u8>,
    pub profiles: Vec<Profile>,
    pub last_bench: Option<BenchReport>,
}

/// Service state and request dispatch, independent of the transport.
pub struct Handler {
    device: Box<dyn Device>,
    tdp_watts: Option<u32>,
    mode: Option<PerformanceMode>,
    /// Auto TDP settings in effect: the global ones, or the active profile's.
    auto_tdp: AutoTdpSettings,
    battery_limit: Option<u8>,
    controller: Option<Controller>,
    fps: Option<f32>,
    telemetry: Option<Telemetry>,
    conflicts: Vec<String>,
    profiles: Vec<Profile>,
    foreground: Option<String>,
    active_profile: Option<String>,
    /// Global state to return to when the profiled game leaves the foreground.
    before_profile: Option<Saved>,
    bench: Option<BenchRun>,
    /// State to return to when the bench ends.
    before_bench: Option<Saved>,
    last_bench: Option<BenchReport>,
    /// Something worth saving changed outside a request (learned watts, bench).
    dirty: bool,
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
            telemetry: None,
            conflicts: Vec::new(),
            profiles: Vec::new(),
            foreground: None,
            active_profile: None,
            before_profile: None,
            bench: None,
            before_bench: None,
            last_bench: None,
            dirty: false,
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
                if let Some(error) = self.bench_running() {
                    return error;
                }
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
                if let Some(error) = self.bench_running() {
                    return error;
                }
                self.disable_auto_tdp();
                self.set_tdp(watts)
            }
            Request::SetAutoTdp { settings } => {
                if let Some(error) = self.bench_running() {
                    return error;
                }
                let response = self.set_auto_tdp(settings, None);
                if response == Response::Ok {
                    self.remember_auto_tdp(settings);
                }
                response
            }
            Request::SetBatteryChargeLimit { percent } => self.apply(
                |d| d.set_battery_charge_limit(percent),
                |s| s.battery_limit = Some(percent),
            ),
            Request::ForegroundApp { exe } => {
                self.foreground = exe;
                self.follow_foreground();
                Response::Ok
            }
            Request::SaveProfile { profile } => {
                if profile.exe.trim().is_empty() {
                    return Response::Error {
                        message: "a profile needs an executable name".into(),
                    };
                }
                self.profiles.retain(|p| !p.matches(&profile.exe));
                self.profiles.push(profile);
                // The settings of an active profile may have changed: re-apply.
                self.active_profile = None;
                self.follow_foreground();
                Response::Ok
            }
            Request::DeleteProfile { exe } => {
                self.profiles.retain(|p| !p.matches(&exe));
                self.follow_foreground();
                Response::Ok
            }
            Request::ListProfiles => Response::Profiles {
                profiles: self.profiles.clone(),
            },
            Request::StartBench { plan } => self.start_bench(plan),
            Request::CancelBench => match self.bench.take() {
                Some(run) => {
                    self.end_bench(run);
                    Response::Ok
                }
                None => Response::Error {
                    message: "no bench is running".into(),
                },
            },
            // While a run is in progress, the previous one.
            Request::GetBenchReport => Response::Bench {
                report: self.last_bench.clone(),
            },
        }
    }

    /// One tick: samples the hardware, advances a running bench, or lets auto
    /// TDP adjust the power while a game runs.
    pub fn tick(&mut self) {
        self.fps = self.device.frame_rate();
        self.telemetry = self.device.telemetry().ok();
        let telemetry = self.telemetry.unwrap_or_default();

        if let Some(run) = self.bench.as_mut() {
            let reading = Reading {
                fps: self.fps,
                gpu_util: telemetry.gpu_util,
                cpu_temp: telemetry.cpu_temp_c,
            };
            match run.tick(reading) {
                Some(Step::SetTdp(watts)) => {
                    if self.device.set_tdp(watts).is_ok() {
                        self.tdp_watts = Some(watts);
                    }
                }
                Some(Step::Done) => {
                    let run = self.bench.take().expect("bench is running");
                    self.end_bench(run);
                }
                None => {}
            }
            return;
        }

        let Some(controller) = self.controller.as_mut() else {
            return;
        };
        let ceiling = match telemetry.on_battery {
            Some(true) => self.auto_tdp.battery_max_watts,
            _ => None,
        };
        controller.set_ceiling(ceiling);
        let Some(fps) = self.fps.filter(|&fps| fps >= MIN_GAME_FPS) else {
            return;
        };
        if telemetry.gpu_util.is_some_and(|util| util < MIN_GAME_GPU) {
            return;
        }
        let sample = canape_autotdp::Sample {
            fps,
            gpu_util: telemetry.gpu_util,
            cpu_temp: telemetry.cpu_temp_c,
        };
        if let Some(watts) = controller.update(sample)
            && self.device.set_tdp(watts).is_ok()
        {
            self.tdp_watts = Some(watts);
        }
        self.learn_watts();
    }

    pub fn status(&mut self) -> Status {
        let telemetry = match self.telemetry {
            Some(telemetry) => telemetry,
            None => self.device.telemetry().unwrap_or_default(),
        };
        Status {
            tdp_watts: self.tdp_watts,
            performance_mode: self.mode,
            auto_tdp: self.auto_tdp,
            fps: self.fps,
            gpu_util: telemetry.gpu_util,
            cpu_temp_c: telemetry.cpu_temp_c,
            battery_percent: telemetry.battery_percent,
            conflicts: self.conflicts.clone(),
            foreground: self.foreground.clone(),
            active_profile: self.active_profile.clone(),
            on_battery: telemetry.on_battery,
            thermal_limited: self
                .controller
                .as_ref()
                .is_some_and(Controller::thermal_limited),
            bench: self.bench.as_ref().map(BenchRun::progress),
        }
    }

    /// Returns true when the list changed.
    pub fn set_conflicts(&mut self, conflicts: Vec<String>) -> bool {
        let changed = conflicts != self.conflicts;
        self.conflicts = conflicts;
        changed
    }

    /// True once after something worth saving changed outside a request.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// The state to persist: the global one, even while a profile or a bench
    /// temporarily overrides it.
    pub fn saved(&self) -> Saved {
        let current = Saved {
            mode: self.mode,
            tdp_watts: self.tdp_watts,
            auto_tdp: self.auto_tdp,
            battery_limit: self.battery_limit,
            profiles: Vec::new(),
            last_bench: None,
        };
        let global = self
            .before_bench
            .as_ref()
            .or(self.before_profile.as_ref())
            .cloned()
            .unwrap_or(current);
        Saved {
            battery_limit: self.battery_limit,
            profiles: self.profiles.clone(),
            last_bench: self.last_bench.clone(),
            ..global
        }
    }

    /// Re-applies a saved state; returns the errors, if any.
    pub fn restore(&mut self, saved: &Saved) -> Vec<String> {
        self.profiles = saved.profiles.clone();
        self.last_bench = saved.last_bench.clone();
        let mut errors = self.apply_power_state(saved);
        if let Some(percent) = saved.battery_limit
            && let Response::Error { message } =
                self.handle(Request::SetBatteryChargeLimit { percent })
        {
            errors.push(message);
        }
        errors
    }

    /// Mode, power limit and auto TDP of `saved`.
    fn apply_power_state(&mut self, saved: &Saved) -> Vec<String> {
        let mut requests = Vec::new();
        match (saved.mode, saved.tdp_watts) {
            (Some(PerformanceMode::Manual), Some(watts)) => {
                requests.push(Request::SetTdp { watts })
            }
            (Some(mode), _) if mode != PerformanceMode::Manual => {
                requests.push(Request::SetPerformanceMode { mode })
            }
            _ => {}
        }
        requests.push(Request::SetAutoTdp {
            settings: saved.auto_tdp,
        });
        requests
            .into_iter()
            .filter_map(|request| match self.handle(request) {
                Response::Error { message } => Some(message),
                _ => None,
            })
            .collect()
    }

    fn bench_running(&self) -> Option<Response> {
        self.bench.is_some().then(|| Response::Error {
            message: "a bench is running: cancel it first".into(),
        })
    }

    fn set_tdp(&mut self, watts: u32) -> Response {
        self.apply(
            |d| d.set_tdp(watts),
            |s| {
                s.tdp_watts = Some(watts);
                s.mode = Some(PerformanceMode::Manual);
            },
        )
    }

    /// Starts auto TDP (or stops it when `settings.enabled` is false),
    /// optionally from a known-good wattage.
    fn set_auto_tdp(&mut self, settings: AutoTdpSettings, start: Option<u32>) -> Response {
        if !settings.enabled {
            self.disable_auto_tdp();
            self.auto_tdp = settings;
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
            max_temp_c: settings.max_temp_c.map(|t| t as f32),
            ..Config::default()
        };
        let start = start.or(self.tdp_watts).unwrap_or(max_watts);
        let controller = Controller::new(config, start);
        // Apply the starting point at once, so a learned wattage takes effect
        // before the first adjustment.
        let start = controller.watts();
        if self.tdp_watts != Some(start)
            && let Response::Error { message } = self.set_tdp(start)
        {
            return Response::Error { message };
        }
        self.controller = Some(controller);
        self.auto_tdp = settings;
        self.mode = Some(PerformanceMode::Manual);
        Response::Ok
    }

    /// A user change of auto TDP settings belongs to the active profile, if
    /// any, else to the global settings.
    fn remember_auto_tdp(&mut self, settings: AutoTdpSettings) {
        if let Some(exe) = &self.active_profile
            && let Some(profile) = self.profiles.iter_mut().find(|p| p.matches(exe))
        {
            profile.auto_tdp = settings;
        }
    }

    fn disable_auto_tdp(&mut self) {
        self.controller = None;
        self.auto_tdp.enabled = false;
    }

    /// Activates the profile of the foreground game, or leaves the active one.
    fn follow_foreground(&mut self) {
        let profile = self
            .foreground
            .as_deref()
            .and_then(|exe| self.profiles.iter().find(|p| p.matches(exe)))
            .cloned();
        match profile {
            Some(profile) if self.active_profile.as_deref() != Some(profile.exe.as_str()) => {
                if self.before_profile.is_none() {
                    self.before_profile = Some(self.saved());
                }
                self.active_profile = Some(profile.exe.clone());
                if self.bench.is_none() {
                    let _ = self.set_auto_tdp(profile.auto_tdp, profile.learned_watts);
                }
            }
            Some(_) => {}
            None => {
                self.active_profile = None;
                if let Some(global) = self.before_profile.take()
                    && self.bench.is_none()
                {
                    self.apply_power_state(&global);
                }
            }
        }
    }

    /// Remembers where auto TDP settles for the active game.
    fn learn_watts(&mut self) {
        let Some(watts) = self.controller.as_ref().and_then(Controller::settled_watts) else {
            return;
        };
        let Some(exe) = &self.active_profile else {
            return;
        };
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.matches(exe))
            && profile.learned_watts != Some(watts)
        {
            profile.learned_watts = Some(watts);
            self.dirty = true;
        }
    }

    fn start_bench(&mut self, plan: BenchPlan) -> Response {
        if let Some(error) = self.bench_running() {
            return error;
        }
        if !self.fps.is_some_and(|fps| fps >= MIN_GAME_FPS) {
            return Response::Error {
                message: "no frame rate: start a game before the bench".into(),
            };
        }
        let (dev_min, dev_max) = self.device.tdp_range();
        let plan = BenchPlan {
            from_watts: plan.from_watts.clamp(dev_min, dev_max),
            to_watts: plan.to_watts.clamp(dev_min, dev_max),
            step_watts: plan.step_watts.max(1),
            settle_secs: plan.settle_secs.max(1),
            measure_secs: plan.measure_secs.max(1),
        };
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let snapshot = self.saved();
        let (run, first) = BenchRun::new(plan, self.foreground.clone(), started);
        self.controller = None;
        if let Response::Error { message } = self.set_tdp(first) {
            return Response::Error { message };
        }
        self.before_bench = Some(snapshot);
        self.bench = Some(run);
        Response::Ok
    }

    /// Keeps the report and gives the power back to what ran before.
    fn end_bench(&mut self, run: BenchRun) {
        self.last_bench = Some(run.into_report());
        self.dirty = true;
        if let Some(before) = self.before_bench.take() {
            self.apply_power_state(&before);
        }
        // The foreground game's profile, if any, takes over again.
        self.active_profile = None;
        self.follow_foreground();
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

    fn profile(exe: &str, target_fps: u32, learned: Option<u32>) -> Profile {
        Profile {
            exe: exe.into(),
            auto_tdp: AutoTdpSettings {
                enabled: true,
                target_fps,
                ..AutoTdpSettings::default()
            },
            learned_watts: learned,
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
    fn auto_tdp_starts_at_the_ceiling_then_lowers_on_an_easy_game() {
        let mut h = handler_with(Some(120.0), Some(0.7));
        assert_eq!(h.handle(auto_on()), Response::Ok);
        assert_eq!(h.status().tdp_watts, Some(25));
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
    fn battery_caps_auto_tdp() {
        let mut h = Handler::new(Box::new(MockDevice {
            fps: Some(30.0),
            on_battery: true,
            ..MockDevice::default()
        }));
        let settings = AutoTdpSettings {
            enabled: true,
            battery_max_watts: Some(15),
            ..AutoTdpSettings::default()
        };
        h.handle(Request::SetAutoTdp { settings });
        h.tick();
        assert_eq!(h.status().tdp_watts, Some(15));
        assert_eq!(h.status().on_battery, Some(true));
    }

    #[test]
    fn saved_state_round_trips() {
        let mut h = handler();
        h.handle(Request::SetBatteryChargeLimit { percent: 80 });
        h.handle(Request::SetTdp { watts: 11 });
        h.handle(auto_on());
        h.handle(Request::SaveProfile {
            profile: profile("game.exe", 40, Some(12)),
        });
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

    #[test]
    fn profile_follows_the_foreground_game_and_hands_back() {
        let mut h = handler();
        h.handle(Request::SetPerformanceMode {
            mode: PerformanceMode::Turbo,
        });
        h.handle(Request::SaveProfile {
            profile: profile("Game.exe", 40, Some(13)),
        });
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        let status = h.status();
        assert_eq!(status.active_profile.as_deref(), Some("Game.exe"));
        assert_eq!(status.auto_tdp.target_fps, 40);
        assert_eq!(status.tdp_watts, Some(13), "starts at the learned wattage");
        // The global state is what gets saved meanwhile.
        assert_eq!(h.saved().mode, Some(PerformanceMode::Turbo));

        h.handle(Request::ForegroundApp {
            exe: Some("explorer.exe".into()),
        });
        let status = h.status();
        assert_eq!(status.active_profile, None);
        assert_eq!(status.performance_mode, Some(PerformanceMode::Turbo));
        assert!(!status.auto_tdp.enabled);
    }

    #[test]
    fn auto_tdp_changes_during_a_profile_update_the_profile() {
        let mut h = handler();
        h.handle(Request::SaveProfile {
            profile: profile("game.exe", 40, None),
        });
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        let settings = AutoTdpSettings {
            enabled: true,
            target_fps: 60,
            ..AutoTdpSettings::default()
        };
        h.handle(Request::SetAutoTdp { settings });
        let Response::Profiles { profiles } = h.handle(Request::ListProfiles) else {
            panic!("expected profiles");
        };
        assert_eq!(profiles[0].auto_tdp.target_fps, 60);
        assert!(!h.saved().auto_tdp.enabled, "global settings untouched");
    }

    #[test]
    fn bench_needs_a_running_game() {
        let mut h = handler();
        h.tick();
        let response = h.handle(Request::StartBench {
            plan: BenchPlan::default(),
        });
        assert!(matches!(response, Response::Error { .. }));
    }

    #[test]
    fn bench_sweeps_reports_and_restores_the_previous_state() {
        let mut h = handler_with(Some(60.0), Some(0.8));
        h.handle(Request::SetTdp { watts: 12 });
        h.tick();
        let plan = BenchPlan {
            from_watts: 20,
            to_watts: 10,
            step_watts: 5,
            settle_secs: 1,
            measure_secs: 2,
        };
        assert_eq!(h.handle(Request::StartBench { plan }), Response::Ok);
        assert_eq!(h.status().tdp_watts, Some(20));
        assert!(h.status().bench.is_some());
        assert!(matches!(
            h.handle(Request::SetTdp { watts: 8 }),
            Response::Error { .. }
        ));
        for _ in 0..9 {
            h.tick();
        }
        assert!(h.status().bench.is_none());
        assert_eq!(h.status().tdp_watts, Some(12), "previous TDP is back");
        assert!(h.take_dirty());
        let Response::Bench {
            report: Some(report),
        } = h.handle(Request::GetBenchReport)
        else {
            panic!("expected a report");
        };
        assert!(report.completed);
        assert_eq!(report.points.len(), 3);
        assert_eq!(h.saved().last_bench, Some(report));
    }

    #[test]
    fn cancelling_a_bench_keeps_a_partial_report() {
        let mut h = handler_with(Some(60.0), Some(0.8));
        h.tick();
        h.handle(Request::StartBench {
            plan: BenchPlan::default(),
        });
        h.tick();
        assert_eq!(h.handle(Request::CancelBench), Response::Ok);
        let Response::Bench {
            report: Some(report),
        } = h.handle(Request::GetBenchReport)
        else {
            panic!("expected a report");
        };
        assert!(!report.completed);
        assert!(h.status().bench.is_none());
    }
}
