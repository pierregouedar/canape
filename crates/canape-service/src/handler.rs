use std::collections::BTreeMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use canape_autotdp::{Config, Controller};
use canape_core::settings::{DEFAULT_ACCENT, FPS_LIMIT_RANGE, GYRO_SENSITIVITY_RANGE};
use canape_core::{
    AutoTdpSettings, BackButtons, BenchPlan, BenchProgress, BenchReport, ControllerMode, Fan,
    FanCurve, FanCurves, FanPreset, GyroSettings, Lighting, PerformanceMode, Profile, ProfilePart,
    Request, Response, Status, TweakId, protocol,
};
use canape_hw::{Device, HwError, Telemetry};
use serde::{Deserialize, Serialize};

use crate::bench_run::{BenchRun, Reading, Step};
#[cfg(test)]
use crate::tweaks;
use crate::tweaks::{Applied, System, Tweaks};
use crate::ui_power;

/// Below this frame rate nothing is being played (desktop, idle shell).
const MIN_GAME_FPS: f32 = 5.0;
/// A bench asked while the game is not in front (the shell is) waits this
/// many ticks of frames before it starts, and gives up after `BENCH_WAIT_MAX`.
const BENCH_WAIT_FRAMES: u32 = 3;
const BENCH_WAIT_MAX: u32 = 120;
/// Below this GPU activity nothing is being played either.
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
    pub accent: Option<String>,
    /// Global custom fan curves (a profile may override them).
    pub fan_curves: FanCurves,
    /// Global frame rate cap (a profile may override it).
    pub fps_limit: Option<u32>,
    pub controller_mode: Option<ControllerMode>,
    /// Global gyro aiming (a profile may override it).
    pub gyro: GyroSettings,
    pub back_buttons: BackButtons,
    pub lighting: Option<Lighting>,
    /// Windows tweaks Canapé applied, and what they replaced.
    pub tweaks: BTreeMap<TweakId, Applied>,
    /// "Full power in Canapé" turned off (it is on unless said otherwise).
    pub ui_power_off: bool,
}

/// Where "full power in Canapé" stands.
#[derive(Debug, Default)]
struct UiPowerState {
    /// Last report from the shell: whether it is in front, and when.
    seen: Option<(bool, Instant)>,
    /// The performance mode's limits are written now.
    active: bool,
    /// Since when the shell has been out of front while active.
    out_since: Option<Instant>,
    /// A user power request overwrote the firmware while active: the level
    /// is written again without a log line.
    resume: bool,
    /// Writing the level failed (see [`ui_power::Input::failed`]).
    failed: bool,
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
    /// A bench waiting for its game to come back to the front: the plan,
    /// ticks waited and ticks in a row with frames.
    pending_bench: Option<(BenchPlan, u32, u32)>,
    /// State to return to when the bench ends.
    before_bench: Option<Saved>,
    last_bench: Option<BenchReport>,
    accent: Option<String>,
    /// Custom curves in effect: the global ones, or the active profile's.
    fan_curves: FanCurves,
    /// Frame rate cap in effect: the global one, or the active profile's.
    fps_limit: Option<u32>,
    controller_mode: Option<ControllerMode>,
    /// Gyro aiming in effect: the global settings, or the active profile's.
    gyro: GyroSettings,
    back_buttons: BackButtons,
    lighting: Option<Lighting>,
    tweaks: Tweaks,
    /// Something worth saving changed outside a request (learned watts, bench).
    dirty: bool,
    ui: UiPowerState,
    ui_power_off: bool,
    clock: Box<dyn Fn() -> Instant + Send>,
    /// Lines for the service log, taken by the server after each call.
    notes: Vec<String>,
}

impl Handler {
    /// With a simulated Windows for the tweaks.
    #[cfg(test)]
    pub fn new(device: Box<dyn Device>) -> Self {
        Self::with_system(device, Box::new(tweaks::SimSystem::new(true)))
    }

    pub fn with_system(device: Box<dyn Device>, system: Box<dyn System>) -> Self {
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
            pending_bench: None,
            before_bench: None,
            last_bench: None,
            accent: None,
            fan_curves: FanCurves::default(),
            fps_limit: None,
            controller_mode: None,
            gyro: GyroSettings::default(),
            back_buttons: BackButtons::default(),
            lighting: None,
            tweaks: Tweaks::new(system),
            dirty: false,
            ui: UiPowerState::default(),
            ui_power_off: false,
            clock: Box::new(Instant::now),
            notes: Vec::new(),
        }
    }

    pub fn handle(&mut self, request: Request) -> Response {
        // The request writes the firmware itself: what the interface level
        // wrote is gone, and is written again below if it still applies.
        if self.ui.active
            && matches!(
                request,
                Request::SetPerformanceMode { .. }
                    | Request::SetTdp { .. }
                    | Request::SetAutoTdp { .. }
            )
        {
            self.ui.active = false;
            self.ui.resume = true;
        }
        let response = self.dispatch(request);
        self.update_ui_power();
        response
    }

    fn dispatch(&mut self, request: Request) -> Response {
        match request {
            Request::UiForeground { front } => {
                self.ui.seen = Some((front, (self.clock)()));
                Response::Ok
            }
            Request::SetUiPower { enabled } => {
                self.ui_power_off = !enabled;
                Response::Ok
            }
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
                let response = self.apply(
                    |d| d.set_performance_mode(mode),
                    |s| {
                        s.mode = Some(mode);
                        // The firmware applies the mode's own power limits.
                        if mode != PerformanceMode::Manual {
                            s.tdp_watts = None;
                        }
                    },
                );
                if response == Response::Ok {
                    // A mode write resets the fan curves: write ours again.
                    self.rewrite_fan_curves();
                }
                response
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
                    self.remember_auto_tdp(self.auto_tdp);
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
                let mut profile = profile;
                // Saved from the shell while the game runs: the shell in front
                // disturbs the game, so the wattage that held before is a
                // better guess than the current one.
                let running = self
                    .foreground
                    .as_deref()
                    .is_some_and(|exe| profile.matches(exe));
                if running
                    && profile.auto_tdp.enabled
                    && let Some(watts) = self
                        .controller
                        .as_ref()
                        .and_then(Controller::last_settled_watts)
                {
                    profile.learned_watts = Some(watts);
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
                None if self.pending_bench.take().is_some() => Response::Ok,
                None => Response::Error {
                    message: "no bench is running".into(),
                },
            },
            Request::SetAccent { color } => {
                if !protocol::is_hex_color(&color) {
                    return Response::Error {
                        message: format!("not a #RRGGBB colour: {color}"),
                    };
                }
                self.accent = Some(color.to_ascii_lowercase());
                // Rings that follow the accent take the new one. A failed
                // write does not undo the accent.
                if let Some(lighting) = self.lighting.clone()
                    && lighting.color.is_none()
                    && lighting.effect.uses_color()
                {
                    let _ = self.write_lighting(lighting);
                }
                Response::Ok
            }
            // While a run is in progress, the previous one.
            Request::GetBenchReport => Response::Bench {
                report: self.last_bench.clone(),
            },
            Request::SetFanCurve { fan, curve } => {
                if let Err(message) = curve.validate() {
                    return Response::Error { message };
                }
                let response = self.write_fan_curve(fan, curve);
                if response == Response::Ok {
                    self.remember_fan_curves();
                }
                response
            }
            Request::ResetFanCurves => {
                let response = self.reset_fan_curves();
                if response == Response::Ok {
                    self.remember_fan_curves();
                }
                response
            }
            Request::GetFanCurves => self.fan_curves_response(),
            Request::SetFanPreset { preset: None } => self.handle(Request::ResetFanCurves),
            Request::SetFanPreset {
                preset: Some(preset),
            } => {
                // The Ally has no middle fan.
                for fan in [Fan::Cpu, Fan::Gpu] {
                    let response = self.write_fan_curve(fan, preset.curve());
                    if response != Response::Ok {
                        return response;
                    }
                }
                self.remember_fan_curves();
                Response::Ok
            }
            Request::SaveForegroundProfile { part } => {
                let Some(exe) = self.foreground.clone() else {
                    return Response::Error {
                        message: "no game in the foreground".into(),
                    };
                };
                // What the profile already has is kept.
                let existing = self
                    .profiles
                    .iter()
                    .find(|p| p.matches(&exe))
                    .cloned()
                    .unwrap_or_else(|| Profile {
                        exe,
                        auto_tdp: self.auto_tdp,
                        ..Default::default()
                    });
                let profile = match part {
                    ProfilePart::Power => Profile {
                        auto_tdp: self.auto_tdp,
                        learned_watts: self.tdp_watts.filter(|_| self.auto_tdp.enabled),
                        ..existing
                    },
                    ProfilePart::Hardware => Profile {
                        fan_curves: Some(self.fan_curves.clone()),
                        fps_limit: self.fps_limit,
                        gyro: Some(self.gyro),
                        ..existing
                    },
                };
                self.handle(Request::SaveProfile { profile })
            }
            Request::SetFpsLimit { fps } => {
                let (low, high) = FPS_LIMIT_RANGE;
                if fps.is_some_and(|fps| !(low..=high).contains(&fps)) {
                    return Response::Error {
                        message: format!("frame rate limit not in {low}..={high} FPS"),
                    };
                }
                let response = self.write_fps_limit(fps);
                if response == Response::Ok {
                    self.remember_fps_limit();
                }
                response
            }
            Request::SetControllerMode { mode } => {
                let response = self.apply(
                    |d| d.set_controller_mode(mode),
                    |s| s.controller_mode = Some(mode),
                );
                if response != Response::Ok || self.back_buttons.is_default() {
                    return response;
                }
                // A mode change brings the mode's own bindings back. WASD mode
                // keeps them: the saved buttons return with another mode.
                let buttons = self.back_buttons;
                self.apply(
                    |d| match d.set_back_buttons(&buttons, mode) {
                        Err(HwError::Unsupported(_)) => Ok(()),
                        result => result,
                    },
                    |_| {},
                )
            }
            Request::SetBackButton { button, target } => {
                let mut buttons = self.back_buttons;
                buttons.set(button, target);
                self.write_back_buttons(buttons)
            }
            Request::ResetBackButtons => self.write_back_buttons(BackButtons::default()),
            Request::SetLighting { lighting: None } => {
                self.lighting = None;
                Response::Ok
            }
            Request::SetLighting {
                lighting: Some(lighting),
            } => match lighting.normalized() {
                Ok(lighting) => self.write_lighting(lighting),
                Err(message) => Response::Error { message },
            },
            Request::ListTweaks => Response::Tweaks {
                tweaks: self.tweaks.list(),
            },
            Request::SetTweak { id, on } => match self.tweaks.set(id, on) {
                Ok(()) => Response::Ok,
                Err(message) => Response::Error { message },
            },
            Request::RevertTweaks => match self.tweaks.revert_all() {
                Ok(()) => Response::Ok,
                Err(message) => Response::Error { message },
            },
            Request::SetGyro { settings } => {
                let (low, high) = GYRO_SENSITIVITY_RANGE;
                if !(low..=high).contains(&settings.sensitivity) {
                    return Response::Error {
                        message: format!("gyro sensitivity not in {low}..={high}"),
                    };
                }
                self.gyro = settings;
                self.remember_gyro();
                Response::Ok
            }
        }
    }

    /// One tick: samples the hardware, advances a running bench, or lets auto
    /// TDP adjust the power while a game runs.
    pub fn tick(&mut self) {
        self.fps = self.device.frame_rate();
        self.telemetry = self.device.telemetry().ok();
        let telemetry = self.telemetry.unwrap_or_default();
        self.update_ui_power();

        if let Some((plan, waited, frames)) = self.pending_bench.as_mut() {
            *waited += 1;
            *frames = if self.fps.is_some_and(|fps| fps >= MIN_GAME_FPS) {
                *frames + 1
            } else {
                0
            };
            let (plan, waited, frames) = (*plan, *waited, *frames);
            if frames >= BENCH_WAIT_FRAMES {
                self.pending_bench = None;
                let _ = self.start_bench(plan);
            } else if waited >= BENCH_WAIT_MAX {
                self.pending_bench = None;
            }
        }

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

        // Auto TDP waits: it would write over the interface level.
        if self.ui.active {
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
        if let Some(watts) = controller.enforce_ceiling()
            && self.device.set_tdp(watts).is_ok()
        {
            self.tdp_watts = Some(watts);
        }
        let Some(controller) = self.controller.as_mut() else {
            return;
        };
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
        let (tdp_min, tdp_max) = self.device.tdp_range();
        Status {
            tdp_watts: self.tdp_watts,
            performance_mode: self.mode,
            auto_tdp: self.auto_tdp,
            fps: self.fps,
            gpu_util: telemetry.gpu_util,
            power_watts: telemetry.power_watts,
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
            bench: self.bench.as_ref().map(BenchRun::progress).or_else(|| {
                self.pending_bench.map(|(plan, _, _)| BenchProgress {
                    step: 0,
                    steps: plan.steps().len() as u32,
                    watts: plan.from_watts,
                    seconds_left: plan.total_secs(),
                })
            }),
            accent: self.accent.clone(),
            tdp_min_watts: Some(tdp_min),
            tdp_max_watts: Some(tdp_max),
            cpu_fan_rpm: telemetry.cpu_fan_rpm,
            gpu_fan_rpm: telemetry.gpu_fan_rpm,
            fan_curves: self.fan_curves.clone(),
            fan_preset: FanPreset::of(&self.fan_curves),
            fps_limit: self.fps_limit,
            controller_mode: self.controller_mode,
            battery_charge_limit: self.battery_limit,
            gyro: self.gyro,
            back_buttons: self.back_buttons,
            lighting: self.lighting.clone(),
            ui_power_enabled: !self.ui_power_off,
            ui_power: self.ui.active,
        }
    }

    /// Takes the lines waiting for the service log.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Replaces the clock the shell's reports are dated with.
    #[cfg(test)]
    fn set_clock(&mut self, clock: impl Fn() -> Instant + Send + 'static) {
        self.clock = Box::new(clock);
    }

    /// Applies the interface power level or puts the user's state back, as
    /// [`ui_power::decide`] says. The user's state (`mode`, `tdp_watts`, the
    /// auto TDP and profile in effect) is never touched: the level only
    /// lives in the firmware, and stays out of what is saved.
    fn update_ui_power(&mut self) {
        let now = (self.clock)();
        let timeout = std::time::Duration::from_secs(protocol::UI_FOREGROUND_TIMEOUT_SECS);
        let shell_front = self
            .ui
            .seen
            .is_some_and(|(front, at)| front && now.saturating_duration_since(at) <= timeout);
        let input = ui_power::Input {
            enabled: !self.ui_power_off,
            shell_front,
            game_running: self.foreground.is_some(),
            bench: self.bench.is_some() || self.pending_bench.is_some(),
            mode: self.mode,
            watts: self.tdp_watts,
            failed: self.ui.failed,
        };
        if !shell_front {
            self.ui.failed = false;
        }
        if self.ui.active && !shell_front {
            self.ui.out_since.get_or_insert(now);
        } else {
            self.ui.out_since = None;
        }
        let out_for = self
            .ui
            .out_since
            .map(|since| now.saturating_duration_since(since));
        let resume = std::mem::take(&mut self.ui.resume);
        match ui_power::decide(input, self.ui.active, out_for) {
            ui_power::Step::Boost => self.boost_ui_power(resume),
            ui_power::Step::Release => {
                let why = if input.game_running {
                    "game running"
                } else if input.bench {
                    "bench"
                } else if !input.enabled {
                    "setting off"
                } else if !ui_power::below_ui_level(input.mode, input.watts) {
                    "power level changed"
                } else if self.ui.seen.is_some_and(|(front, _)| front) {
                    "shell stopped reporting"
                } else {
                    "shell left the foreground"
                };
                self.release_ui_power(why);
            }
            ui_power::Step::Keep => {
                if resume {
                    self.note("ui power: off (power level changed)".into());
                }
            }
        }
    }

    fn note(&mut self, line: String) {
        self.notes.push(line);
    }

    /// The user's state as a few words for the log.
    fn describe_level(&self) -> String {
        match (self.mode, self.tdp_watts) {
            (Some(PerformanceMode::Manual), Some(watts)) => format!("manual {watts} W"),
            (Some(mode), _) => format!("{mode:?}").to_lowercase(),
            (None, _) => "unknown".into(),
        }
    }

    fn boost_ui_power(&mut self, quiet: bool) {
        match self
            .device
            .set_performance_mode(PerformanceMode::Performance)
        {
            Ok(()) => {
                self.ui.active = true;
                // A mode write resets the fan curves: write ours again.
                self.rewrite_fan_curves();
                if !quiet {
                    let line = format!(
                        "ui power: on (shell in front, was {})",
                        self.describe_level()
                    );
                    self.note(line);
                }
            }
            Err(err) => {
                self.ui.failed = true;
                self.note(format!("ui power: not applied ({err})"));
            }
        }
    }

    fn release_ui_power(&mut self, why: &str) {
        self.ui.active = false;
        self.ui.out_since = None;
        let result = match (self.mode, self.tdp_watts) {
            (Some(PerformanceMode::Manual), Some(watts)) => self.device.set_tdp(watts),
            (Some(PerformanceMode::Manual), None) | (None, _) => Ok(()),
            (Some(mode), _) => self.device.set_performance_mode(mode).inspect(|()| {
                self.rewrite_fan_curves();
            }),
        };
        let level = self.describe_level();
        match result {
            Ok(()) => self.note(format!("ui power: off ({why}, back to {level})")),
            Err(err) => self.note(format!(
                "ui power: off ({why}), {level} not restored: {err}"
            )),
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
            accent: None,
            fan_curves: self.fan_curves.clone(),
            fps_limit: self.fps_limit,
            controller_mode: None,
            gyro: self.gyro,
            back_buttons: BackButtons::default(),
            lighting: None,
            tweaks: BTreeMap::new(),
            ui_power_off: false,
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
            accent: self.accent.clone(),
            controller_mode: self.controller_mode,
            back_buttons: self.back_buttons,
            lighting: self.lighting.clone(),
            tweaks: self.tweaks.saved(),
            ui_power_off: self.ui_power_off,
            ..global
        }
    }

    /// Re-applies a saved state; returns the errors, if any.
    pub fn restore(&mut self, saved: &Saved) -> Vec<String> {
        self.profiles = saved.profiles.clone();
        self.last_bench = saved.last_bench.clone();
        self.accent = saved.accent.clone();
        self.gyro = saved.gyro;
        self.tweaks.restore(saved.tweaks.clone());
        self.ui_power_off = saved.ui_power_off;
        let mut errors = self.apply_power_state(saved);
        errors.extend(self.apply_fans_and_fps(&saved.fan_curves, saved.fps_limit));
        let mut requests = Vec::new();
        if let Some(percent) = saved.battery_limit {
            requests.push(Request::SetBatteryChargeLimit { percent });
        }
        if let Some(mode) = saved.controller_mode {
            requests.push(Request::SetControllerMode { mode });
        }
        // Without a saved mode, nothing above writes the buttons.
        if saved.controller_mode.is_none() && !saved.back_buttons.is_default() {
            if let Response::Error { message } = self.write_back_buttons(saved.back_buttons) {
                errors.push(message);
            }
        } else {
            self.back_buttons = saved.back_buttons;
        }
        if let Some(lighting) = saved.lighting.clone() {
            requests.push(Request::SetLighting {
                lighting: Some(lighting),
            });
        }
        for request in requests {
            if let Response::Error { message } = self.handle(request) {
                errors.push(message);
            }
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
        (self.bench.is_some() || self.pending_bench.is_some()).then(|| Response::Error {
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
            self.auto_tdp = settings
                .clamped(self.device.tdp_range())
                .unwrap_or(settings);
            return Response::Ok;
        }
        // Bounded by the model; the status shows the clamped values.
        let Some(settings) = settings.clamped(self.device.tdp_range()) else {
            return Response::Error {
                message: "invalid auto TDP settings".into(),
            };
        };
        let (min_watts, max_watts) = (settings.min_watts, settings.max_watts);
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
                // What the profile leaves unset follows the global state.
                let global = self.before_profile.as_ref().expect("set above");
                let curves = profile
                    .fan_curves
                    .unwrap_or_else(|| global.fan_curves.clone());
                let fps_limit = profile.fps_limit.or(global.fps_limit);
                self.gyro = profile.gyro.unwrap_or(global.gyro);
                self.apply_fans_and_fps(&curves, fps_limit);
            }
            Some(_) => {}
            None => {
                self.active_profile = None;
                if let Some(global) = self.before_profile.take() {
                    if self.bench.is_none() {
                        self.apply_power_state(&global);
                    }
                    self.gyro = global.gyro;
                    self.apply_fans_and_fps(&global.fan_curves, global.fps_limit);
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
            // Started from the shell, which hides the game: wait for it to
            // come back to the front.
            if self.foreground.is_some() {
                self.pending_bench = Some((plan, 0, 0));
                return Response::Ok;
            }
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
        let report = run.into_report();
        // A cancelled run does not hide a complete one of the same game.
        let keep_previous = !report.completed
            && self
                .last_bench
                .as_ref()
                .is_some_and(|last| last.completed && last.exe == report.exe);
        if !keep_previous {
            self.last_bench = Some(report);
        }
        self.dirty = true;
        // Restored as global settings: with the profile still active, the
        // auto TDP part would be written into the profile.
        self.active_profile = None;
        if let Some(before) = self.before_bench.take() {
            self.apply_power_state(&before);
        }
        // The foreground game's profile, if any, takes over again.
        self.follow_foreground();
    }

    fn write_fan_curve(&mut self, fan: Fan, curve: FanCurve) -> Response {
        self.apply(
            |d| d.set_fan_curve(fan, &curve),
            |s| s.fan_curves.set(fan, Some(curve)),
        )
    }

    /// Every fan back to the mode's curve. The firmware applies the mode's
    /// power limits too, so a manual TDP is written again.
    fn reset_fan_curves(&mut self) -> Response {
        let response = self.apply(
            |d| d.reset_fan_curves(),
            |s| s.fan_curves = FanCurves::default(),
        );
        if response == Response::Ok
            && self.mode == Some(PerformanceMode::Manual)
            && let Some(watts) = self.tdp_watts
            && let Err(err) = self.device.set_tdp(watts)
        {
            return Response::Error {
                message: format!("fan curves reset, but the TDP was not applied again: {err}"),
            };
        }
        response
    }

    /// After a performance mode write: the firmware dropped the custom
    /// curves. One that cannot be written again is forgotten, so the status
    /// shows the fan on the mode's curve.
    fn rewrite_fan_curves(&mut self) {
        for (fan, curve) in self.fan_curves.clone().iter() {
            if self.device.set_fan_curve(fan, &curve).is_err() {
                self.fan_curves.set(fan, None);
                self.dirty = true;
            }
        }
    }

    /// Moves to `curves` and `fps_limit`, writing only what differs.
    fn apply_fans_and_fps(&mut self, curves: &FanCurves, fps_limit: Option<u32>) -> Vec<String> {
        let mut errors = Vec::new();
        if *curves != self.fan_curves {
            // Going back to a firmware curve needs a reset, which drops all.
            let dropped = Fan::ALL
                .into_iter()
                .any(|fan| self.fan_curves.get(fan).is_some() && curves.get(fan).is_none());
            if dropped && let Response::Error { message } = self.reset_fan_curves() {
                errors.push(message);
            }
            for (fan, curve) in curves.iter() {
                if self.fan_curves.get(fan) != Some(&curve)
                    && let Response::Error { message } = self.write_fan_curve(fan, curve)
                {
                    errors.push(message);
                }
            }
        }
        if fps_limit != self.fps_limit
            && let Response::Error { message } = self.write_fps_limit(fps_limit)
        {
            errors.push(message);
        }
        errors
    }

    fn write_fps_limit(&mut self, fps: Option<u32>) -> Response {
        self.apply(|d| d.set_fps_limit(fps), |s| s.fps_limit = fps)
    }

    /// The global state being held aside, by a profile and / or a bench.
    fn global_snapshots(&mut self) -> impl Iterator<Item = &mut Saved> {
        self.before_bench
            .iter_mut()
            .chain(self.before_profile.iter_mut())
    }

    /// A curve change belongs to the active profile when it has its own
    /// curves, else to the global settings.
    fn remember_fan_curves(&mut self) {
        let curves = self.fan_curves.clone();
        if let Some(profile) = self.active_profile_mut()
            && profile.fan_curves.is_some()
        {
            profile.fan_curves = Some(curves);
            return;
        }
        for saved in self.global_snapshots() {
            saved.fan_curves = curves.clone();
        }
    }

    /// Same rule as [`Self::remember_fan_curves`].
    fn remember_fps_limit(&mut self) {
        let fps_limit = self.fps_limit;
        if let Some(profile) = self.active_profile_mut()
            && profile.fps_limit.is_some()
        {
            // `None` there means "follow the global cap" from the next
            // activation on.
            profile.fps_limit = fps_limit;
            return;
        }
        for saved in self.global_snapshots() {
            saved.fps_limit = fps_limit;
        }
    }

    /// Same rule as [`Self::remember_fan_curves`].
    fn remember_gyro(&mut self) {
        let gyro = self.gyro;
        if let Some(profile) = self.active_profile_mut()
            && profile.gyro.is_some()
        {
            profile.gyro = Some(gyro);
            return;
        }
        for saved in self.global_snapshots() {
            saved.gyro = gyro;
        }
    }

    fn active_profile_mut(&mut self) -> Option<&mut Profile> {
        let exe = self.active_profile.as_deref()?;
        self.profiles.iter_mut().find(|p| p.matches(exe))
    }

    fn fan_curves_response(&mut self) -> Response {
        let mode = self.mode.unwrap_or(PerformanceMode::Performance);
        let mut defaults = FanCurves::default();
        for fan in Fan::ALL {
            defaults.set(fan, self.device.default_fan_curve(fan, mode).ok().flatten());
        }
        Response::FanCurves {
            custom: self.fan_curves.clone(),
            defaults,
        }
    }

    /// Writes `lighting`, its colour or the accent's.
    fn write_lighting(&mut self, lighting: Lighting) -> Response {
        let color = lighting
            .color
            .as_deref()
            .or(self.accent.as_deref())
            .unwrap_or(DEFAULT_ACCENT);
        let Some(rgb) = protocol::hex_rgb(color) else {
            return Response::Error {
                message: format!("not a #RRGGBB colour: {color}"),
            };
        };
        let (level, effect, speed) = (lighting.level, lighting.effect, lighting.speed);
        self.apply(
            |d| d.set_lighting(level, rgb, effect, speed),
            |s| s.lighting = Some(lighting),
        )
    }

    /// The bindings go with the mode Canapé set, gamepad when it set none
    /// (the controller's own after a restart).
    fn write_back_buttons(&mut self, buttons: BackButtons) -> Response {
        let mode = self.controller_mode.unwrap_or(ControllerMode::Gamepad);
        self.apply(
            |d| d.set_back_buttons(&buttons, mode),
            |s| s.back_buttons = buttons,
        )
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
    use canape_core::{FanPreset, LightingEffect, LightingSpeed};
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
            ..Profile::default()
        }
    }

    fn set_curve(fan: Fan, preset: FanPreset) -> Request {
        Request::SetFanCurve {
            fan,
            curve: preset.curve(),
        }
    }

    fn custom(h: &mut Handler) -> FanCurves {
        h.status().fan_curves
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

        // No game on screen: the ceiling still applies.
        let mut idle = Handler::new(Box::new(MockDevice {
            on_battery: true,
            ..MockDevice::default()
        }));
        idle.handle(Request::SetAutoTdp { settings });
        assert_eq!(idle.status().tdp_watts, Some(25));
        idle.tick();
        assert_eq!(idle.status().tdp_watts, Some(15));
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
        assert_eq!(
            h.handle(Request::SetAccent {
                color: "#2563EB".into()
            }),
            Response::Ok
        );
        assert!(matches!(
            h.handle(Request::SetAccent {
                color: "blue".into()
            }),
            Response::Error { .. }
        ));
        let saved = h.saved();
        assert_eq!(saved.accent.as_deref(), Some("#2563eb"));

        let mut restored = handler();
        assert!(restored.restore(&saved).is_empty());
        assert_eq!(restored.saved(), saved);
        assert_eq!(restored.status().battery_charge_limit, Some(80));
    }

    #[test]
    fn rejects_a_charge_limit_out_of_range() {
        let mut h = handler();
        assert!(matches!(
            h.handle(Request::SetBatteryChargeLimit { percent: 20 }),
            Response::Error { .. }
        ));
        assert_eq!(h.status().battery_charge_limit, None);
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

    fn short_plan() -> BenchPlan {
        BenchPlan {
            from_watts: 20,
            to_watts: 10,
            step_watts: 5,
            settle_secs: 1,
            measure_secs: 2,
        }
    }

    #[test]
    fn the_game_profile_takes_over_again_after_a_bench() {
        let mut h = handler_with(Some(60.0), Some(0.8));
        h.handle(Request::SetTdp { watts: 20 });
        h.handle(Request::SaveProfile {
            profile: profile("game.exe", 40, None),
        });
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        h.tick();
        assert_eq!(
            h.handle(Request::StartBench { plan: short_plan() }),
            Response::Ok
        );
        for _ in 0..9 {
            h.tick();
        }
        assert!(h.status().bench.is_none());
        let status = h.status();
        assert_eq!(status.active_profile.as_deref(), Some("game.exe"));
        assert!(status.auto_tdp.enabled);
        assert_eq!(status.auto_tdp.target_fps, 40);
        let Response::Profiles { profiles } = h.handle(Request::ListProfiles) else {
            panic!("expected profiles");
        };
        assert!(profiles[0].auto_tdp.enabled, "profile left as it was");
        assert!(!h.saved().auto_tdp.enabled, "global settings untouched");
    }

    #[test]
    fn a_bench_waits_for_its_game_to_come_back_to_the_front() {
        let mut h = handler();
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        h.tick();
        // The shell is in front: no frames, the bench waits.
        assert_eq!(
            h.handle(Request::StartBench { plan: short_plan() }),
            Response::Ok
        );
        h.tick();
        let waiting = h.status().bench.expect("waiting shows as a bench");
        assert_eq!((waiting.step, waiting.steps), (0, 3));
        assert!(matches!(
            h.handle(Request::SetTdp { watts: 8 }),
            Response::Error { .. }
        ));
        // The game is back in front.
        h.device = Box::new(MockDevice {
            fps: Some(60.0),
            ..MockDevice::default()
        });
        for _ in 0..BENCH_WAIT_FRAMES {
            h.tick();
        }
        let running = h.status().bench.expect("bench started");
        assert_eq!(running.step, 1);
        assert_eq!(h.status().tdp_watts, Some(20));
    }

    #[test]
    fn a_waiting_bench_gives_up_or_cancels() {
        let mut h = handler();
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        h.handle(Request::StartBench { plan: short_plan() });
        assert_eq!(h.handle(Request::CancelBench), Response::Ok);
        assert!(h.status().bench.is_none());

        h.handle(Request::StartBench { plan: short_plan() });
        for _ in 0..BENCH_WAIT_MAX {
            h.tick();
        }
        assert!(h.status().bench.is_none());
        assert!(h.handle(Request::SetTdp { watts: 12 }) == Response::Ok);
    }

    #[test]
    fn a_cancelled_run_keeps_the_last_complete_report() {
        let mut h = handler_with(Some(60.0), Some(0.8));
        h.tick();
        h.handle(Request::StartBench { plan: short_plan() });
        for _ in 0..9 {
            h.tick();
        }
        h.handle(Request::StartBench { plan: short_plan() });
        h.tick();
        h.handle(Request::CancelBench);
        let Response::Bench {
            report: Some(report),
        } = h.handle(Request::GetBenchReport)
        else {
            panic!("expected a report");
        };
        assert!(report.completed);
        assert_eq!(report.points.len(), 3);
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

    #[test]
    fn status_reports_the_model_range_and_clamped_auto_tdp() {
        let mut h = handler();
        let settings = AutoTdpSettings {
            enabled: true,
            min_watts: 3,
            max_watts: 50,
            battery_max_watts: Some(40),
            ..AutoTdpSettings::default()
        };
        assert_eq!(h.handle(Request::SetAutoTdp { settings }), Response::Ok);
        let status = h.status();
        assert_eq!(
            (status.tdp_min_watts, status.tdp_max_watts),
            (Some(7), Some(30))
        );
        let auto = status.auto_tdp;
        assert_eq!((auto.min_watts, auto.max_watts), (7, 30));
        assert_eq!(auto.battery_max_watts, Some(30));
        assert_eq!(status.tdp_watts, Some(30), "starts at the clamped ceiling");
        assert_eq!(h.saved().auto_tdp, auto);
    }

    #[test]
    fn fan_curves_are_validated_shown_and_survive_a_mode_change() {
        let mut h = handler();
        let mut bad = FanPreset::Balanced.curve();
        bad.0[7] = (95, 0);
        assert!(matches!(
            h.handle(Request::SetFanCurve {
                fan: Fan::Cpu,
                curve: bad
            }),
            Response::Error { .. }
        ));
        assert!(matches!(
            h.handle(set_curve(Fan::Mid, FanPreset::Balanced)),
            Response::Error { .. }
        ));
        assert_eq!(
            h.handle(set_curve(Fan::Cpu, FanPreset::MaxCooling)),
            Response::Ok
        );
        assert_eq!(custom(&mut h).cpu, Some(FanPreset::MaxCooling.curve()));
        let fast = h.status().cpu_fan_rpm.unwrap();
        // The mock, like the firmware, drops curves on a mode write; the
        // handler writes them again.
        h.handle(Request::SetPerformanceMode {
            mode: PerformanceMode::Silent,
        });
        assert_eq!(custom(&mut h).cpu, Some(FanPreset::MaxCooling.curve()));
        h.tick();
        assert_eq!(h.status().cpu_fan_rpm, Some(fast));

        let Response::FanCurves { custom, defaults } = h.handle(Request::GetFanCurves) else {
            panic!("expected fan curves");
        };
        assert!(custom.cpu.is_some());
        assert_eq!(defaults.cpu, Some(FanPreset::Balanced.curve()));
        assert_eq!(defaults.mid, None);
    }

    #[test]
    fn resetting_fan_curves_keeps_a_manual_tdp() {
        let mut h = handler();
        h.handle(Request::SetTdp { watts: 12 });
        h.handle(set_curve(Fan::Gpu, FanPreset::Balanced));
        assert_eq!(h.handle(Request::ResetFanCurves), Response::Ok);
        assert!(custom(&mut h).is_empty());
        assert!(h.saved().fan_curves.is_empty());
        assert_eq!(h.status().tdp_watts, Some(12));
        assert_eq!(h.status().performance_mode, Some(PerformanceMode::Manual));
    }

    #[test]
    fn fps_limit_and_controller_mode() {
        let mut h = handler_with(Some(90.0), Some(0.7));
        for fps in [Some(10), Some(500)] {
            assert!(matches!(
                h.handle(Request::SetFpsLimit { fps }),
                Response::Error { .. }
            ));
        }
        assert_eq!(
            h.handle(Request::SetFpsLimit { fps: Some(40) }),
            Response::Ok
        );
        h.tick();
        assert_eq!(h.status().fps_limit, Some(40));
        assert_eq!(h.status().fps, Some(40.0), "the mock caps its frame rate");
        assert_eq!(
            h.handle(Request::SetControllerMode {
                mode: ControllerMode::Mouse
            }),
            Response::Ok
        );
        assert_eq!(h.status().controller_mode, Some(ControllerMode::Mouse));
        assert_eq!(h.handle(Request::SetFpsLimit { fps: None }), Response::Ok);
        assert_eq!(h.status().fps_limit, None);
    }

    #[test]
    fn back_buttons_are_kept_and_reset() {
        use canape_core::{BackButton, ButtonTarget};
        let mut h = handler();
        assert!(h.status().back_buttons.is_default());
        assert_eq!(
            h.handle(Request::SetBackButton {
                button: BackButton::M1,
                target: ButtonTarget::PadA,
            }),
            Response::Ok
        );
        h.handle(Request::SetBackButton {
            button: BackButton::M2,
            target: ButtonTarget::KeyF12,
        });
        // A mode change keeps them.
        h.handle(Request::SetControllerMode {
            mode: ControllerMode::Mouse,
        });
        let buttons = h.status().back_buttons;
        assert_eq!(buttons.m1, ButtonTarget::PadA);
        assert_eq!(buttons.m2, ButtonTarget::KeyF12);

        let json = serde_json::to_string(&h.saved()).unwrap();
        let mut restored = handler();
        assert!(
            restored
                .restore(&serde_json::from_str(&json).unwrap())
                .is_empty()
        );
        assert_eq!(restored.status().back_buttons, buttons);

        // WASD mode keeps its own bindings: the mode change goes through,
        // a new binding does not, and the saved ones stay for later.
        assert_eq!(
            h.handle(Request::SetControllerMode {
                mode: ControllerMode::Wasd,
            }),
            Response::Ok
        );
        assert!(matches!(
            h.handle(Request::SetBackButton {
                button: BackButton::M1,
                target: ButtonTarget::PadB,
            }),
            Response::Error { .. }
        ));
        assert_eq!(h.status().back_buttons, buttons);

        assert_eq!(
            h.handle(Request::SetControllerMode {
                mode: ControllerMode::Gamepad,
            }),
            Response::Ok
        );
        assert_eq!(h.handle(Request::ResetBackButtons), Response::Ok);
        assert!(h.status().back_buttons.is_default());
        assert!(h.saved().back_buttons.is_default());
    }

    #[test]
    fn lighting_follows_the_accent_and_is_kept() {
        let mut h = handler();
        assert_eq!(h.status().lighting, None);
        let follow = Lighting {
            level: 2,
            color: None,
            ..Lighting::default()
        };
        assert_eq!(
            h.handle(Request::SetLighting {
                lighting: Some(follow.clone()),
            }),
            Response::Ok
        );
        assert!(matches!(
            h.handle(Request::SetLighting {
                lighting: Some(Lighting {
                    level: 9,
                    color: None,
                    ..Lighting::default()
                }),
            }),
            Response::Error { .. }
        ));
        assert_eq!(h.status().lighting, Some(follow.clone()));
        h.handle(Request::SetAccent {
            color: "#10B981".into(),
        });

        let json = serde_json::to_string(&h.saved()).unwrap();
        let mut restored = handler();
        assert!(
            restored
                .restore(&serde_json::from_str(&json).unwrap())
                .is_empty()
        );
        assert_eq!(restored.status().lighting, Some(follow));

        let red = Lighting {
            level: 3,
            color: Some("#FF0000".into()),
            ..Lighting::default()
        };
        h.handle(Request::SetLighting {
            lighting: Some(red),
        });
        assert_eq!(
            h.status().lighting.and_then(|l| l.color).as_deref(),
            Some("#ff0000")
        );
        assert_eq!(
            h.handle(Request::SetLighting { lighting: None }),
            Response::Ok
        );
        assert_eq!(h.saved().lighting, None);
    }

    #[test]
    fn lighting_writes_the_accent_colour() {
        use std::sync::{Arc, Mutex};

        type Writes = Arc<Mutex<Vec<(u8, [u8; 3], LightingEffect)>>>;
        /// The mock, with the lighting writes kept where the test sees them.
        struct Rings(MockDevice, Writes);
        impl Device for Rings {
            fn name(&self) -> &str {
                self.0.name()
            }
            fn tdp_range(&self) -> (u32, u32) {
                self.0.tdp_range()
            }
            fn set_tdp(&mut self, watts: u32) -> canape_hw::Result<()> {
                self.0.set_tdp(watts)
            }
            fn set_performance_mode(&mut self, mode: PerformanceMode) -> canape_hw::Result<()> {
                self.0.set_performance_mode(mode)
            }
            fn set_battery_charge_limit(&mut self, percent: u8) -> canape_hw::Result<()> {
                self.0.set_battery_charge_limit(percent)
            }
            fn telemetry(&mut self) -> canape_hw::Result<Telemetry> {
                self.0.telemetry()
            }
            fn set_lighting(
                &mut self,
                level: u8,
                rgb: [u8; 3],
                effect: LightingEffect,
                _speed: LightingSpeed,
            ) -> canape_hw::Result<()> {
                self.1.lock().unwrap().push((level, rgb, effect));
                Ok(())
            }
        }

        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut h = Handler::new(Box::new(Rings(MockDevice::default(), writes.clone())));
        let follow = Some(Lighting {
            level: 1,
            color: None,
            ..Lighting::default()
        });
        h.handle(Request::SetLighting {
            lighting: follow.clone(),
        });
        h.handle(Request::SetAccent {
            color: "#102030".into(),
        });
        h.handle(Request::SetLighting {
            lighting: Some(Lighting {
                level: 3,
                color: Some("#00ff7f".into()),
                ..Lighting::default()
            }),
        });
        // A fixed colour no longer follows the accent.
        h.handle(Request::SetAccent {
            color: "#ffffff".into(),
        });
        // Nor does an effect that shows no colour of its own.
        h.handle(Request::SetLighting {
            lighting: Some(Lighting {
                level: 2,
                effect: LightingEffect::Rainbow,
                ..Lighting::default()
            }),
        });
        h.handle(Request::SetAccent {
            color: "#000000".into(),
        });
        let steady = LightingEffect::Steady;
        assert_eq!(
            *writes.lock().unwrap(),
            [
                (1, [0xff, 0x3d, 0x5e], steady),
                (1, [0x10, 0x20, 0x30], steady),
                (3, [0x00, 0xff, 0x7f], steady),
                (2, [0xff, 0xff, 0xff], LightingEffect::Rainbow),
            ]
        );
    }

    #[test]
    fn new_settings_round_trip_through_state() {
        let mut h = handler();
        h.handle(set_curve(Fan::Cpu, FanPreset::Balanced));
        h.handle(Request::SetFpsLimit { fps: Some(45) });
        h.handle(Request::SetControllerMode {
            mode: ControllerMode::Wasd,
        });
        let saved = h.saved();
        assert_eq!(saved.fps_limit, Some(45));
        assert_eq!(saved.controller_mode, Some(ControllerMode::Wasd));
        let json = serde_json::to_string(&saved).unwrap();
        let mut restored = handler();
        assert!(
            restored
                .restore(&serde_json::from_str(&json).unwrap())
                .is_empty()
        );
        assert_eq!(restored.saved(), saved);
        assert_eq!(
            restored.status().fan_curves.cpu,
            Some(FanPreset::Balanced.curve())
        );

        // A state.json from before these settings still loads.
        let old: Saved = serde_json::from_str(r#"{"mode":"turbo","profiles":[]}"#).unwrap();
        assert!(old.fan_curves.is_empty());
        assert_eq!(old.fps_limit, None);
    }

    #[test]
    fn a_profile_brings_its_fans_and_cap_and_hands_back() {
        let mut h = handler();
        h.handle(set_curve(Fan::Cpu, FanPreset::Balanced));
        h.handle(Request::SetFpsLimit { fps: Some(60) });
        let mut curves = FanCurves::default();
        curves.set(Fan::Gpu, Some(FanPreset::MaxCooling.curve()));
        h.handle(Request::SaveProfile {
            profile: Profile {
                fan_curves: Some(curves.clone()),
                fps_limit: Some(40),
                ..profile("game.exe", 40, None)
            },
        });
        h.handle(Request::SaveProfile {
            profile: profile("other.exe", 40, None),
        });

        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        assert_eq!(custom(&mut h), curves, "the global CPU curve is reset");
        assert_eq!(h.status().fps_limit, Some(40));
        // Changes during the game belong to the profile.
        h.handle(set_curve(Fan::Gpu, FanPreset::Balanced));
        h.handle(Request::SetFpsLimit { fps: Some(50) });
        let saved = h.saved();
        assert_eq!(saved.fan_curves.cpu, Some(FanPreset::Balanced.curve()));
        assert_eq!(saved.fps_limit, Some(60));
        let game = &saved.profiles[0];
        assert_eq!(game.fps_limit, Some(50));
        assert_eq!(
            game.fan_curves.as_ref().and_then(|c| c.gpu),
            Some(FanPreset::Balanced.curve())
        );

        // A profile without its own settings follows the global ones, and a
        // change there is global.
        h.handle(Request::ForegroundApp {
            exe: Some("other.exe".into()),
        });
        assert_eq!(custom(&mut h).cpu, Some(FanPreset::Balanced.curve()));
        assert_eq!(custom(&mut h).gpu, None);
        assert_eq!(h.status().fps_limit, Some(60));
        h.handle(Request::SetFpsLimit { fps: Some(30) });
        assert_eq!(h.saved().fps_limit, Some(30));
        assert_eq!(h.saved().profiles[1].fps_limit, None);

        h.handle(Request::ForegroundApp { exe: None });
        assert_eq!(custom(&mut h).cpu, Some(FanPreset::Balanced.curve()));
        assert_eq!(h.status().fps_limit, Some(30));
    }

    #[test]
    fn gyro_settings_are_checked_saved_and_follow_profiles() {
        use canape_core::{GyroActivation, GyroAxis};
        let mut h = handler();
        let hold = GyroSettings {
            activation: GyroActivation::Hold,
            sensitivity: 12,
            ..GyroSettings::default()
        };
        for sensitivity in [0, 51] {
            assert!(matches!(
                h.handle(Request::SetGyro {
                    settings: GyroSettings {
                        sensitivity,
                        ..hold
                    }
                }),
                Response::Error { .. }
            ));
        }
        assert_eq!(h.handle(Request::SetGyro { settings: hold }), Response::Ok);
        assert_eq!(h.status().gyro, hold);
        assert_eq!(h.saved().gyro, hold);

        let roll = GyroSettings {
            activation: GyroActivation::Always,
            horizontal: GyroAxis::Roll,
            ..hold
        };
        h.handle(Request::SaveProfile {
            profile: Profile {
                gyro: Some(roll),
                ..profile("game.exe", 60, None)
            },
        });
        h.handle(Request::SaveProfile {
            profile: profile("other.exe", 60, None),
        });
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        assert_eq!(h.status().gyro, roll);
        // A change during the game belongs to its profile.
        let toggle = GyroSettings {
            activation: GyroActivation::Toggle,
            ..roll
        };
        h.handle(Request::SetGyro { settings: toggle });
        assert_eq!(h.saved().gyro, hold);
        assert_eq!(h.saved().profiles[0].gyro, Some(toggle));

        // A profile without its own gyro follows the global one.
        h.handle(Request::ForegroundApp {
            exe: Some("other.exe".into()),
        });
        assert_eq!(h.status().gyro, hold);
        h.handle(Request::ForegroundApp { exe: None });
        assert_eq!(h.status().gyro, hold);

        let json = serde_json::to_string(&h.saved()).unwrap();
        let mut restored = handler();
        restored.restore(&serde_json::from_str(&json).unwrap());
        assert_eq!(restored.status().gyro, hold);
        let old: Saved = serde_json::from_str(r#"{"mode":"turbo"}"#).unwrap();
        assert_eq!(old.gyro, GyroSettings::default());
    }

    #[test]
    fn a_fan_preset_sets_both_fans_and_shows_in_the_status() {
        let mut h = handler();
        assert_eq!(h.status().fan_preset, None);
        h.handle(Request::SetFanPreset {
            preset: Some(FanPreset::MaxCooling),
        });
        let curves = custom(&mut h);
        assert_eq!(curves.cpu, Some(FanPreset::MaxCooling.curve()));
        assert_eq!(curves.gpu, Some(FanPreset::MaxCooling.curve()));
        assert_eq!(h.status().fan_preset, Some(FanPreset::MaxCooling));
        // One fan changed by hand: no preset any more.
        h.handle(set_curve(Fan::Cpu, FanPreset::Balanced));
        assert_eq!(h.status().fan_preset, None);
        h.handle(Request::SetFanPreset { preset: None });
        assert!(custom(&mut h).is_empty());
    }

    #[test]
    fn saving_the_game_in_front_keeps_the_other_part() {
        let mut h = handler();
        assert!(matches!(
            h.handle(Request::SaveForegroundProfile {
                part: ProfilePart::Hardware
            }),
            Response::Error { .. }
        ));
        h.handle(Request::SaveProfile {
            profile: profile("game.exe", 40, Some(9)),
        });
        h.handle(Request::ForegroundApp {
            exe: Some("game.exe".into()),
        });
        h.handle(Request::SetFanPreset {
            preset: Some(FanPreset::Balanced),
        });
        h.handle(Request::SetFpsLimit { fps: Some(45) });
        assert_eq!(
            h.handle(Request::SaveForegroundProfile {
                part: ProfilePart::Hardware
            }),
            Response::Ok
        );
        let game = &h.saved().profiles[0];
        assert_eq!(game.fps_limit, Some(45));
        assert_eq!(
            game.fan_curves.as_ref().and_then(|c| c.cpu),
            Some(FanPreset::Balanced.curve())
        );
        // The power part is untouched.
        assert_eq!(game.auto_tdp.target_fps, 40);
        assert_eq!(game.learned_watts, Some(9));
    }

    #[test]
    fn saving_a_new_game_takes_the_current_auto_tdp() {
        let mut h = handler();
        h.handle(auto_on());
        h.handle(Request::ForegroundApp {
            exe: Some("new.exe".into()),
        });
        h.handle(Request::SaveForegroundProfile {
            part: ProfilePart::Power,
        });
        let saved = h.saved();
        let game = saved
            .profiles
            .iter()
            .find(|p| p.matches("new.exe"))
            .unwrap();
        assert!(game.auto_tdp.enabled);
        assert_eq!(game.fan_curves, None);
    }

    mod ui_power_flow {
        use super::*;
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        /// The mock, with the power writes kept where the test sees them.
        struct Spy(MockDevice, Arc<Mutex<Vec<String>>>);
        impl Device for Spy {
            fn name(&self) -> &str {
                self.0.name()
            }
            fn tdp_range(&self) -> (u32, u32) {
                self.0.tdp_range()
            }
            fn set_tdp(&mut self, watts: u32) -> canape_hw::Result<()> {
                self.1.lock().unwrap().push(format!("tdp {watts}"));
                self.0.set_tdp(watts)
            }
            fn set_performance_mode(&mut self, mode: PerformanceMode) -> canape_hw::Result<()> {
                self.1.lock().unwrap().push(format!("mode {mode:?}"));
                self.0.set_performance_mode(mode)
            }
            fn set_battery_charge_limit(&mut self, percent: u8) -> canape_hw::Result<()> {
                self.0.set_battery_charge_limit(percent)
            }
            fn telemetry(&mut self) -> canape_hw::Result<Telemetry> {
                self.0.telemetry()
            }
            fn frame_rate(&mut self) -> Option<f32> {
                self.0.frame_rate()
            }
            fn set_fan_curve(&mut self, fan: Fan, curve: &FanCurve) -> canape_hw::Result<()> {
                self.1.lock().unwrap().push(format!("curve {fan:?}"));
                self.0.set_fan_curve(fan, curve)
            }
        }

        struct Rig {
            h: Handler,
            writes: Arc<Mutex<Vec<String>>>,
            now: Arc<Mutex<Instant>>,
        }

        impl Rig {
            fn new() -> Self {
                Self::with_fps(None)
            }

            fn with_fps(fps: Option<f32>) -> Self {
                let writes = Arc::new(Mutex::new(Vec::new()));
                let now = Arc::new(Mutex::new(Instant::now()));
                let device = MockDevice {
                    fps,
                    ..MockDevice::default()
                };
                let mut h = Handler::new(Box::new(Spy(device, writes.clone())));
                let clock = now.clone();
                h.set_clock(move || *clock.lock().unwrap());
                Self { h, writes, now }
            }

            fn wait(&mut self, millis: u64) {
                *self.now.lock().unwrap() += Duration::from_millis(millis);
                self.h.tick();
            }

            fn front(&mut self, front: bool) {
                self.h.handle(Request::UiForeground { front });
            }

            fn writes(&self) -> Vec<String> {
                std::mem::take(&mut *self.writes.lock().unwrap())
            }
        }

        fn silent(rig: &mut Rig) {
            rig.h.handle(Request::SetPerformanceMode {
                mode: PerformanceMode::Silent,
            });
            rig.writes();
            rig.h.take_notes();
        }

        #[test]
        fn a_low_mode_gets_full_power_while_the_shell_is_in_front() {
            let mut rig = Rig::new();
            silent(&mut rig);
            rig.front(true);
            assert_eq!(rig.writes(), ["mode Performance"]);
            let status = rig.h.status();
            assert!(status.ui_power);
            assert_eq!(status.performance_mode, Some(PerformanceMode::Silent));
            assert_eq!(rig.h.saved().mode, Some(PerformanceMode::Silent));
            assert_eq!(
                rig.h.take_notes(),
                ["ui power: on (shell in front, was silent)"]
            );
            // Nothing changes, nothing is written.
            rig.front(true);
            rig.wait(1000);
            rig.front(true);
            assert!(rig.writes().is_empty());
        }

        #[test]
        fn the_user_state_comes_back_after_the_hold() {
            let mut rig = Rig::new();
            silent(&mut rig);
            rig.front(true);
            rig.writes();
            rig.h.take_notes();
            rig.front(false);
            rig.wait(1000);
            assert!(rig.writes().is_empty(), "held for a moment");
            // Back in front: no flapping.
            rig.front(true);
            rig.wait(1000);
            assert!(rig.writes().is_empty());
            rig.front(false);
            rig.wait(500);
            rig.wait(1600);
            assert_eq!(rig.writes(), ["mode Silent"]);
            assert!(!rig.h.status().ui_power);
            assert_eq!(
                rig.h.take_notes(),
                ["ui power: off (shell left the foreground, back to silent)"]
            );
        }

        #[test]
        fn a_silent_shell_times_out() {
            let mut rig = Rig::new();
            rig.h.handle(Request::SetTdp { watts: 9 });
            rig.writes();
            rig.front(true);
            assert_eq!(rig.writes(), ["mode Performance"]);
            for _ in 0..8 {
                rig.wait(1000);
            }
            assert_eq!(rig.writes(), ["tdp 9"]);
            assert!(
                rig.h
                    .take_notes()
                    .last()
                    .is_some_and(|note| note.starts_with("ui power: off (shell stopped reporting"))
            );
        }

        #[test]
        fn a_game_in_front_restores_at_once_and_the_shell_does_not_boost_it() {
            let mut rig = Rig::new();
            silent(&mut rig);
            rig.front(true);
            rig.writes();
            rig.h.handle(Request::ForegroundApp {
                exe: Some("game.exe".into()),
            });
            assert_eq!(rig.writes(), ["mode Silent"]);
            // The game is still running behind the shell.
            rig.front(true);
            rig.wait(1000);
            assert!(rig.writes().is_empty());
            // The game is gone: the shell has the level again.
            rig.h.handle(Request::ForegroundApp { exe: None });
            assert_eq!(rig.writes(), ["mode Performance"]);
        }

        #[test]
        fn a_profile_is_restored_not_the_state_before_it() {
            let mut rig = Rig::new();
            silent(&mut rig);
            rig.h.handle(Request::SaveProfile {
                profile: profile("game.exe", 40, Some(11)),
            });
            rig.front(true);
            rig.writes();
            rig.h.handle(Request::ForegroundApp {
                exe: Some("game.exe".into()),
            });
            let status = rig.h.status();
            assert_eq!(status.tdp_watts, Some(11));
            assert!(!status.ui_power);
            assert_eq!(rig.writes().last().map(String::as_str), Some("tdp 11"));
        }

        #[test]
        fn levels_at_or_above_the_ui_level_are_left_alone() {
            for mode in [PerformanceMode::Performance, PerformanceMode::Turbo] {
                let mut rig = Rig::new();
                rig.h.handle(Request::SetPerformanceMode { mode });
                rig.writes();
                rig.front(true);
                rig.wait(1000);
                assert!(rig.writes().is_empty());
                assert!(!rig.h.status().ui_power);
            }
            let mut rig = Rig::new();
            rig.h.handle(Request::SetTdp { watts: 25 });
            rig.writes();
            rig.front(true);
            assert!(rig.writes().is_empty());
        }

        #[test]
        fn a_bench_never_gets_the_boost() {
            let mut rig = Rig::with_fps(Some(60.0));
            silent(&mut rig);
            rig.wait(1000);
            assert_eq!(
                rig.h.handle(Request::StartBench { plan: short_plan() }),
                Response::Ok
            );
            rig.front(true);
            rig.wait(1000);
            assert!(rig.h.status().bench.is_some());
            assert!(!rig.h.status().ui_power);
        }

        #[test]
        fn the_setting_turns_it_off_and_is_saved() {
            let mut rig = Rig::new();
            silent(&mut rig);
            rig.front(true);
            rig.writes();
            rig.h.handle(Request::SetUiPower { enabled: false });
            assert_eq!(rig.writes(), ["mode Silent"]);
            let status = rig.h.status();
            assert!(!status.ui_power && !status.ui_power_enabled);
            rig.wait(1000);
            assert!(rig.writes().is_empty());

            let saved = rig.h.saved();
            assert!(saved.ui_power_off);
            let mut restored = Rig::new();
            restored.h.restore(&saved);
            assert!(!restored.h.status().ui_power_enabled);
            assert!(Rig::new().h.status().ui_power_enabled);
        }

        #[test]
        fn a_user_change_while_boosted_is_written_then_boosted_again() {
            let mut rig = Rig::new();
            silent(&mut rig);
            rig.front(true);
            rig.writes();
            rig.h.take_notes();
            rig.h.handle(Request::SetTdp { watts: 8 });
            assert_eq!(rig.writes(), ["tdp 8", "mode Performance"]);
            assert!(rig.h.take_notes().is_empty(), "no log line for a step");
            let status = rig.h.status();
            assert!(status.ui_power);
            assert_eq!(status.tdp_watts, Some(8));
            // Raised past the ui level: nothing to boost any more.
            rig.h.handle(Request::SetTdp { watts: 20 });
            assert_eq!(rig.writes(), ["tdp 20"]);
            assert!(!rig.h.status().ui_power);
            assert_eq!(rig.h.take_notes(), ["ui power: off (power level changed)"]);
        }

        #[test]
        fn auto_tdp_waits_while_boosted_and_the_fan_curve_is_kept() {
            let mut rig = Rig::new();
            rig.h.handle(Request::SetAutoTdp {
                settings: AutoTdpSettings {
                    enabled: true,
                    min_watts: 7,
                    max_watts: 12,
                    ..AutoTdpSettings::default()
                },
            });
            rig.h.handle(set_curve(Fan::Cpu, FanPreset::Balanced));
            rig.writes();
            rig.front(true);
            assert_eq!(rig.writes(), ["mode Performance", "curve Cpu"]);
            rig.wait(1000);
            assert!(rig.writes().is_empty(), "the controller does not write");
            rig.front(false);
            rig.wait(2000);
            assert_eq!(rig.writes(), ["tdp 12"]);
            assert!(rig.h.status().auto_tdp.enabled);
        }
    }
}
