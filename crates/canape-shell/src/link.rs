//! Connection to the service: one subscription feeding the UI, and one
//! connection for commands and their answers (profiles, bench report), both
//! reconnecting on their own.

use std::io;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use canape_core::{
    BenchReport, ButtonTarget, ControllerMode, GyroActivation, GyroAxis, LightingEffect,
    LightingSpeed, PerformanceMode, Profile, Request, Response, Status, protocol,
};
use canape_ipc::Connection;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};

use crate::history::History;
use crate::report::{self, Recommendation};
use crate::{App, AppWindow, BenchRow, BenchView, Notice, ProfileView, RecView, StatusView, Theme};

const RETRY: Duration = Duration::from_secs(2);

/// At or under this power limit, in watts, the interface is slow enough for
/// its costly animations to be skipped (see [`reduced_motion`]).
pub const REDUCED_MOTION_WATTS: u32 = 12;

/// True when the power is low and the interface does not run at full power
/// (it does while "full power in Canapé" is in effect): silent mode, or a
/// power limit of [`REDUCED_MOTION_WATTS`] or less.
pub fn reduced_motion(status: &Status) -> bool {
    !status.ui_power
        && (status.performance_mode == Some(PerformanceMode::Silent)
            || status
                .tdp_watts
                .is_some_and(|watts| watts <= REDUCED_MOTION_WATTS))
}

#[derive(Default)]
struct Shared {
    status: Mutex<Status>,
    report: Mutex<Option<BenchReport>>,
    /// Last foreground program seen by the shell, resent after a reconnection.
    foreground: Mutex<Option<String>>,
    /// Last profile list from the service.
    profiles: Mutex<Vec<Profile>>,
    history: Mutex<History>,
    accent: Mutex<AccentState>,
}

#[derive(Default)]
struct AccentState {
    /// Accent shown by the shell, `#rrggbb`.
    local: String,
    /// Last accent sent to the service: its echo must not undo a newer pick.
    sent: Option<String>,
}

impl AccentState {
    /// Whether an accent pushed by the service should replace the local one.
    /// The echo of our own last send is consumed once, so the same colour
    /// set again later from elsewhere is still followed.
    fn adopt(&mut self, remote: &str) -> bool {
        if self.sent.as_deref() == Some(remote) {
            self.sent = None;
            return false;
        }
        if remote == self.local {
            return false;
        }
        self.local = remote.to_owned();
        true
    }
}

#[derive(Clone)]
pub struct Link {
    commands: Sender<Request>,
    shared: Arc<Shared>,
}

/// Requests sent by the shell on its own: their failures are logged, not
/// shown to the user.
fn is_background(request: &Request) -> bool {
    matches!(
        request,
        Request::ForegroundApp { .. }
            | Request::UiForeground { .. }
            | Request::ListProfiles
            | Request::GetBenchReport
            | Request::ListTweaks
    )
}

impl Link {
    /// `accent`: the one shown at startup, `#rrggbb`.
    pub fn start(ui: Weak<AppWindow>, accent: String) -> Self {
        let shared = Arc::new(Shared::default());
        shared.accent.lock().unwrap().local = accent;
        let (commands, rx) = mpsc::channel::<Request>();

        let (ui_status, state, again) = (ui.clone(), shared.clone(), commands.clone());
        thread::spawn(move || {
            loop {
                if let Err(err) = follow(&ui_status, &state, &again) {
                    crate::log(&format!("service link: {err}"));
                }
                show_status(&ui_status, &state, None);
                // No settings without the service: gyro aiming stops.
                #[cfg(windows)]
                {
                    crate::gyro::follow(canape_core::GyroSettings::default(), false);
                    crate::motion_server::follow(canape_core::GyroSettings::default());
                }
                thread::sleep(RETRY);
            }
        });

        let (state, again) = (shared.clone(), commands.clone());
        thread::spawn(move || {
            let mut conn: Option<Connection> = None;
            for request in rx {
                let background = is_background(&request);
                let answer = send(&mut conn, &request);
                // A tweak changed (or failed): show its new state.
                if matches!(request, Request::SetTweak { .. }) {
                    let _ = again.send(Request::ListTweaks);
                }
                let notice = match answer {
                    Ok(Response::Profiles { profiles }) => {
                        *state.profiles.lock().unwrap() = profiles.clone();
                        show_profiles(&ui, &profiles);
                        continue;
                    }
                    Ok(Response::Bench { report }) => {
                        *state.report.lock().unwrap() = report;
                        show_report(&ui, &state);
                        continue;
                    }
                    Ok(Response::Tweaks { tweaks }) => {
                        crate::tweaks::show(&ui, tweaks);
                        continue;
                    }
                    Ok(Response::Error { message }) => {
                        crate::log(&format!("{request:?}: {message}"));
                        (Notice::ServiceError, message)
                    }
                    Ok(_) => {
                        if matches!(
                            request,
                            Request::SaveProfile { .. } | Request::DeleteProfile { .. }
                        ) {
                            let _ = again.send(Request::ListProfiles);
                        }
                        let saved = matches!(request, Request::SaveProfile { .. });
                        (
                            if saved { Notice::Saved } else { Notice::Idle },
                            String::new(),
                        )
                    }
                    Err(err) => {
                        conn = None;
                        (Notice::Unreachable, err.to_string())
                    }
                };
                if !background {
                    let _ = ui.upgrade_in_event_loop(move |ui| {
                        let app = ui.global::<App>();
                        app.set_notice(notice.0);
                        app.set_notice_detail(notice.1.into());
                    });
                }
            }
        });

        Self { commands, shared }
    }

    pub fn send(&self, request: Request) {
        let _ = self.commands.send(request);
    }

    /// Last status pushed by the service.
    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    /// Last bench report fetched from the service.
    pub fn report(&self) -> Option<BenchReport> {
        self.shared.report.lock().unwrap().clone()
    }

    /// The user is picking `hex`: from now on it is the shell's accent.
    pub fn accent_picked(&self, hex: &str) {
        self.shared.accent.lock().unwrap().local = hex.to_owned();
    }

    /// Shares the accent with the service, hence with the widget.
    pub fn send_accent(&self, hex: &str) {
        self.shared.accent.lock().unwrap().sent = Some(hex.to_owned());
        self.send(Request::SetAccent {
            color: hex.to_owned(),
        });
    }

    /// True when `exe` has a game profile.
    #[cfg(windows)]
    pub fn has_profile(&self, exe: &str) -> bool {
        self.profile(exe).is_some()
    }

    /// The saved profile of `exe`, if any.
    pub fn profile(&self, exe: &str) -> Option<Profile> {
        self.shared
            .profiles
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.matches(exe))
            .cloned()
    }

    /// Reports the foreground program to the service, now and after every
    /// reconnection.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn set_foreground(&self, exe: Option<String>) {
        *self.shared.foreground.lock().unwrap() = exe.clone();
        self.send(Request::ForegroundApp { exe });
    }
}

fn open() -> io::Result<Connection> {
    let mut conn = canape_ipc::connect()?;
    conn.send(&Request::Hello {
        protocol_version: protocol::PROTOCOL_VERSION,
    })?;
    match conn.recv::<Response>()? {
        Some(Response::Hello { .. }) => Ok(conn),
        other => Err(io::Error::other(format!("unexpected handshake: {other:?}"))),
    }
}

fn send(conn: &mut Option<Connection>, request: &Request) -> io::Result<Response> {
    if conn.is_none() {
        *conn = Some(open()?);
    }
    let c = conn.as_mut().unwrap();
    c.send(request)?;
    c.recv::<Response>()?
        .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
}

/// Subscribes and forwards every status push until the connection drops.
/// Status pushes (about one per second) between two profile list refreshes.
const PROFILES_REFRESH_PUSHES: u32 = 10;

fn follow(ui: &Weak<AppWindow>, shared: &Shared, commands: &Sender<Request>) -> io::Result<()> {
    let mut conn = open()?;
    conn.send(&Request::Subscribe)?;
    // A (re)started service knows nothing of the user session.
    let foreground = shared.foreground.lock().unwrap().clone();
    if foreground.is_some() {
        let _ = commands.send(Request::ForegroundApp { exe: foreground });
    }
    let _ = commands.send(Request::ListProfiles);
    let _ = commands.send(Request::GetBenchReport);

    let mut first = true;
    let mut pushes = 0u32;
    while let Some(response) = conn.recv::<Response>()? {
        if let Response::Status(status) = response {
            pushes = pushes.wrapping_add(1);
            let previous = std::mem::replace(&mut *shared.status.lock().unwrap(), status.clone());
            if !first && previous.bench.is_some() && status.bench.is_none() {
                // A run just ended: fetch its results.
                let _ = commands.send(Request::GetBenchReport);
            }
            if previous.auto_tdp.target_fps != status.auto_tdp.target_fps {
                show_report(ui, shared);
            }
            // Profiles can also change elsewhere (widget, canape-ctl): the
            // status does not say, so poll the short list now and then.
            if previous.active_profile != status.active_profile
                || previous.foreground != status.foreground
                || pushes % PROFILES_REFRESH_PUSHES == 0
            {
                let _ = commands.send(Request::ListProfiles);
            }
            if first || previous.accent != status.accent {
                follow_accent(ui, shared, commands, status.accent.as_deref(), first);
            }
            let active = status.active_profile.as_deref().and_then(|exe| {
                let profiles = shared.profiles.lock().unwrap();
                profiles.iter().find(|p| p.matches(exe)).cloned()
            });
            let profile_size = active
                .as_ref()
                .and_then(|p| p.resolution)
                .map(|r| (r.width, r.height));
            crate::screen::follow(
                status.foreground.as_deref(),
                active.as_ref().and_then(|p| p.refresh_hz),
                profile_size,
            );
            crate::image::follow(
                status.active_profile.as_deref(),
                active.as_ref().map(|p| (p.upscaling, p.frame_gen)),
            );
            #[cfg(windows)]
            {
                crate::gyro::follow(status.gyro, status.foreground.is_some());
                crate::motion_server::follow(status.gyro);
            }
            if first || pushes % SCREEN_REFRESH_PUSHES == 0 {
                show_screen(ui);
            }
            show_image(ui);
            first = false;
            show_status(ui, shared, Some(status));
        }
    }
    Ok(())
}

/// Adopts an accent changed elsewhere (another client, or the service's
/// saved one at startup); gives ours to a service that has none.
fn follow_accent(
    ui: &Weak<AppWindow>,
    shared: &Shared,
    commands: &Sender<Request>,
    remote: Option<&str>,
    first: bool,
) {
    let mut accent = shared.accent.lock().unwrap();
    let Some(remote) = remote.filter(|hex| protocol::is_hex_color(hex)) else {
        if first {
            accent.sent = Some(accent.local.clone());
            let _ = commands.send(Request::SetAccent {
                color: accent.local.clone(),
            });
        }
        return;
    };
    let remote = remote.to_ascii_lowercase();
    if !accent.adopt(&remote) {
        return;
    }
    drop(accent);
    crate::save_accent(&remote);
    if let Some(color) = crate::accent::parse(&remote) {
        let _ = ui.upgrade_in_event_loop(move |ui| crate::accent::show(&ui, color, None));
    }
}

/// Status pushes between two reads of the screen's refresh rate, which
/// Windows settings can change behind the shell's back.
const SCREEN_REFRESH_PUSHES: u32 = 5;

/// Shows the screen's current refresh rate and the ones it offers.
pub fn show_screen(ui: &Weak<AppWindow>) {
    let (current, rates) = crate::screen::state();
    let rates: Vec<i32> = rates.into_iter().map(|hz| hz as i32).collect();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let app = ui.global::<App>();
        app.set_refresh_hz(current as i32);
        app.set_refresh_rates(ModelRc::new(VecModel::from(rates)));
    });
    show_resolutions(ui);
}

/// Shows the screen's current resolution and the ones it offers.
pub fn show_resolutions(ui: &Weak<AppWindow>) {
    let (current, sizes) = crate::screen::resolution_state();
    let size = current;
    let index = sizes
        .iter()
        .position(|&size| size == current)
        .map_or(-1, |i| i as i32);
    let label = |(w, h): (u32, u32)| SharedString::from(format!("{w}×{h}"));
    let current = if current.0 > 0 {
        label(current)
    } else {
        SharedString::new()
    };
    let labels: Vec<SharedString> = sizes.into_iter().map(label).collect();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let app = ui.global::<App>();
        app.set_resolution(current);
        app.set_resolutions(ModelRc::new(VecModel::from(labels)));
        app.set_resolution_index(index);
        refit_window(&ui, size);
    });
}

/// A full-screen window that did not follow a change of resolution (a game's
/// profile gave it back) is put full screen again.
fn refit_window(ui: &AppWindow, screen: (u32, u32)) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let window = ui.window();
    let size = window.size();
    if screen.0 == 0 || !window.is_fullscreen() || (size.width, size.height) == screen {
        return;
    }
    let mut last = LAST.lock().unwrap();
    if last.is_some_and(|at| at.elapsed() < Duration::from_secs(10)) {
        return;
    }
    *last = Some(Instant::now());
    crate::log(&format!(
        "window: {}x{} on a {}x{} screen, full screen again",
        size.width, size.height, screen.0, screen.1
    ));
    window.set_fullscreen(false);
    window.set_fullscreen(true);
}

/// Shows the upscaling and frame generation choices, and whether Lossless
/// Scaling can be chosen.
pub fn show_image(ui: &Weak<AppWindow>) {
    let (upscaling, frame_gen) = crate::image::current();
    let lossless = crate::image::lossless_installed();
    let amd_failed = crate::radeon::failed();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let app = ui.global::<App>();
        app.set_image_upscaling(crate::image::upscaling_index(upscaling.method));
        app.set_image_sharpness(upscaling.sharpness as i32);
        app.set_image_frame_gen(crate::image::frame_gen_index(frame_gen.method));
        app.set_image_rate(crate::image::rate_index(frame_gen.rate));
        app.set_image_target_fps(frame_gen.target_fps as i32);
        app.set_lossless_scaling(lossless);
        app.set_image_amd_failed(amd_failed);
    });
}

fn show_status(ui: &Weak<AppWindow>, shared: &Shared, status: Option<Status>) {
    let view = status_view(status.as_ref());
    let reduced = status.as_ref().is_some_and(reduced_motion);
    let graphs = {
        let mut history = shared.history.lock().unwrap();
        history.push(status.as_ref(), Instant::now());
        history.view()
    };
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.global::<Theme>().set_reduced_motion(reduced);
        let app = ui.global::<App>();
        let learned = app
            .get_profiles()
            .iter()
            .find(|p| !view.active_profile.is_empty() && p.exe == view.active_profile)
            .map_or(-1, |p| p.learned_watts);
        app.set_learned_watts(learned);
        app.set_status(view);
        app.set_graphs(graphs);
    });
}

fn show_profiles(ui: &Weak<AppWindow>, profiles: &[Profile]) {
    let mut rows: Vec<ProfileView> = profiles.iter().map(profile_view).collect();
    rows.sort_by_key(|p| p.exe.to_lowercase());
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.global::<App>()
            .set_profiles(ModelRc::new(VecModel::from(rows)));
    });
}

fn show_report(ui: &Weak<AppWindow>, shared: &Shared) {
    let target = shared.status.lock().unwrap().auto_tdp.target_fps;
    let report = shared.report.lock().unwrap().clone();
    let (view, rows) = bench_view(report.as_ref(), target);
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let app = ui.global::<App>();
        app.set_bench(view);
        app.set_bench_rows(ModelRc::new(VecModel::from(rows)));
    });
}

fn mode_name(mode: Option<PerformanceMode>) -> &'static str {
    match mode {
        Some(PerformanceMode::Silent) => "silent",
        Some(PerformanceMode::Performance) => "performance",
        Some(PerformanceMode::Turbo) => "turbo",
        Some(PerformanceMode::Manual) => "manual",
        None => "",
    }
}

/// Status as the UI shows it. Numbers stay numbers: Slint formats and
/// translates them. -1 means unknown.
pub fn status_view(status: Option<&Status>) -> StatusView {
    let Some(s) = status else {
        return StatusView {
            tdp: -1,
            power_tenths: -1,
            fps: -1,
            gpu: -1,
            temp: -1,
            battery: -1,
            cpu_rpm: -1,
            gpu_rpm: -1,
            cpu_boost: -1,
            cpu_epp: -1,
            cpu_max_mhz: -1,
            cpu_min_cores: -1,
            ..StatusView::default()
        };
    };
    let known = |v: Option<f32>| v.map_or(-1, |v| v.round() as i32);
    let bench = s.bench;
    let (fan_line, fan_area) = s
        .fan_curves
        .cpu
        .as_ref()
        .map(crate::hardware::curve_paths)
        .unwrap_or_default();
    StatusView {
        connected: true,
        ui_power_enabled: s.ui_power_enabled,
        ui_power: s.ui_power,
        tdp: s.tdp_watts.map_or(-1, |w| w as i32),
        mode: mode_name(s.performance_mode).into(),
        power_tenths: known(s.power_watts.map(|w| w * 10.0)),
        fps: known(s.fps),
        gpu: known(s.gpu_util.map(|g| g * 100.0)),
        temp: known(s.cpu_temp_c),
        battery: s.battery_percent.map_or(-1, i32::from),
        power_known: s.on_battery.is_some(),
        on_battery: s.on_battery.unwrap_or(false),
        auto_tdp: s.auto_tdp.enabled,
        target_fps: s.auto_tdp.target_fps as i32,
        min_watts: s.auto_tdp.min_watts as i32,
        max_watts: s.auto_tdp.max_watts as i32,
        battery_cap: s.auto_tdp.battery_max_watts.map_or(0, |w| w as i32),
        temp_limit: s.auto_tdp.max_temp_c.map_or(0, |t| t as i32),
        thermal_limited: s.thermal_limited,
        foreground: s.foreground.clone().unwrap_or_default().into(),
        game: s
            .foreground
            .as_deref()
            .map(game_name)
            .unwrap_or_default()
            .into(),
        active_profile: s.active_profile.clone().unwrap_or_default().into(),
        conflicts: s.conflicts.join(", ").into(),
        bench_running: bench.is_some(),
        bench_step: bench.map_or(0, |b| b.step as i32),
        bench_steps: bench.map_or(0, |b| b.steps as i32),
        bench_watts: bench.map_or(0, |b| b.watts as i32),
        bench_seconds_left: bench.map_or(0, |b| b.seconds_left as i32),
        tdp_floor: tdp_floor(s) as i32,
        tdp_ceiling: tdp_ceiling(s) as i32,
        cpu_rpm: s.cpu_fan_rpm.map_or(-1, |r| r as i32),
        gpu_rpm: s.gpu_fan_rpm.map_or(-1, |r| r as i32),
        fan_preset: crate::hardware::preset_name(&s.fan_curves).into(),
        fan_line: fan_line.into(),
        fan_area: fan_area.into(),
        fps_limit: s.fps_limit.map_or(0, |f| f as i32),
        cpu_boost: s.cpu_power.boost.map_or(-1, |b| b.mode() as i32),
        cpu_epp: s.cpu_power.epp.map_or(-1, |v| v as i32),
        cpu_max_mhz: s.cpu_power.max_mhz.map_or(-1, |v| v as i32),
        cpu_min_cores: s.cpu_power.min_cores.map_or(-1, |v| v as i32),
        controller: match s.controller_mode {
            Some(ControllerMode::Gamepad) => "gamepad",
            Some(ControllerMode::Wasd) => "wasd",
            Some(ControllerMode::Mouse) => "mouse",
            None => "",
        }
        .into(),
        charge_limit: s.battery_charge_limit.map_or(0, i32::from),
        gyro_activation: match s.gyro.activation {
            GyroActivation::Off => "off",
            GyroActivation::Hold => "hold",
            GyroActivation::Toggle => "toggle",
            GyroActivation::Always => "always",
        }
        .into(),
        gyro_sensitivity: s.gyro.sensitivity as i32,
        gyro_roll: s.gyro.horizontal == GyroAxis::Roll,
        gyro_invert: s.gyro.invert_y,
        gyro_server: s.gyro.motion_server,
        m1: target_index(s.back_buttons.m1),
        m2: target_index(s.back_buttons.m2),
        lighting_level: s.lighting.as_ref().map_or(-1, |l| i32::from(l.level)),
        lighting_color: s
            .lighting
            .as_ref()
            .and_then(|l| l.color.clone())
            .unwrap_or_default()
            .into(),
        lighting_effect: s.lighting.as_ref().map_or(0, |l| {
            LightingEffect::ALL
                .iter()
                .position(|&e| e == l.effect)
                .map_or(0, |i| i as i32)
        }),
        lighting_speed: s.lighting.as_ref().map_or(1, |l| {
            LightingSpeed::ALL
                .iter()
                .position(|&e| e == l.speed)
                .map_or(1, |i| i as i32)
        }),
    }
}

/// The name shown for a program: its file name without the folder and the
/// `.exe`.
fn game_name(exe: &str) -> &str {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    match file.len().checked_sub(4) {
        Some(at) if file.is_char_boundary(at) && file[at..].eq_ignore_ascii_case(".exe") => {
            &file[..at]
        }
        _ => file,
    }
}

/// Position of `target` in [`ButtonTarget::ALL`], the order of the
/// controller page's names.
fn target_index(target: ButtonTarget) -> i32 {
    ButtonTarget::ALL
        .iter()
        .position(|&t| t == target)
        .map_or(0, |i| i as i32)
}

/// Lowest wattage this model accepts, when the service knows it.
pub fn tdp_floor(status: &Status) -> u32 {
    status.tdp_min_watts.unwrap_or(crate::MIN_WATTS)
}

/// Highest wattage this model accepts, when the service knows it.
pub fn tdp_ceiling(status: &Status) -> u32 {
    status.tdp_max_watts.unwrap_or(crate::MAX_WATTS)
}

fn profile_view(profile: &Profile) -> ProfileView {
    let a = profile.auto_tdp;
    ProfileView {
        exe: profile.exe.as_str().into(),
        auto_tdp: a.enabled,
        target_fps: a.target_fps as i32,
        min_watts: a.min_watts as i32,
        max_watts: a.max_watts as i32,
        learned_watts: profile.learned_watts.map_or(-1, |w| w as i32),
        fps_limit: profile.fps_limit.map_or(0, |f| f as i32),
        cpu_power: profile.cpu_power.is_some_and(|p| !p.is_default()),
        screen: screen_text(profile).into(),
    }
}

/// The screen mode a profile asks for, as shown on the game page.
fn screen_text(profile: &Profile) -> String {
    let size = profile
        .resolution
        .map(|r| format!("{}×{}", r.width, r.height));
    let rate = profile.refresh_hz.map(|hz| format!("{hz} Hz"));
    [size, rate]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn rec_view(rec: Option<Recommendation>) -> RecView {
    rec.map_or(RecView::default(), |r| RecView {
        available: true,
        watts: r.watts as i32,
        fps: r.fps.round() as i32,
    })
}

/// Bench page content for `report`, recommendations computed for `target_fps`.
pub fn bench_view(report: Option<&BenchReport>, target_fps: u32) -> (BenchView, Vec<BenchRow>) {
    let Some(report) = report.filter(|r| !r.points.is_empty()) else {
        return (BenchView::default(), Vec::new());
    };
    let recs = report::recommendations(report, target_fps);
    let best = report.points.iter().map(|p| p.fps_avg).fold(1.0, f32::max);
    let rows = report
        .points
        .iter()
        .map(|p| BenchRow {
            watts: p.watts as i32,
            fps_avg: p.fps_avg.round() as i32,
            fps_low: p.fps_low.round() as i32,
            // One decimal is all a frames-per-watt figure needs.
            fps_per_watt_tenths: (p.fps_per_watt() * 10.0).round() as i32,
            temp: p.temp_max.map_or(-1, |t| t.round() as i32),
            share: p.fps_avg / best,
        })
        .collect();
    let view = BenchView {
        available: true,
        completed: report.completed,
        exe: report.exe.clone().unwrap_or_default().into(),
        target_fps: target_fps as i32,
        sweet_spot: rec_view(recs.sweet_spot),
        for_target: rec_view(recs.for_target),
        most_efficient: rec_view(recs.most_efficient),
    };
    (view, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_core::{AutoTdpSettings, BenchPlan, BenchPoint, BenchProgress};

    #[test]
    fn motion_is_reduced_on_a_low_limit_unless_the_ui_has_full_power() {
        let status = |mode, tdp_watts, ui_power| Status {
            performance_mode: mode,
            tdp_watts,
            ui_power,
            ..Status::default()
        };
        let manual = Some(PerformanceMode::Manual);
        assert!(reduced_motion(&status(
            Some(PerformanceMode::Silent),
            None,
            false
        )));
        assert!(reduced_motion(&status(manual, Some(12), false)));
        assert!(!reduced_motion(&status(manual, Some(13), false)));
        assert!(!reduced_motion(&status(
            Some(PerformanceMode::Performance),
            None,
            false
        )));
        assert!(!reduced_motion(&status(
            Some(PerformanceMode::Silent),
            None,
            true
        )));
    }

    #[test]
    fn disconnected_status_is_all_unknown() {
        let view = status_view(None);
        assert!(!view.connected);
        assert_eq!((view.tdp, view.fps, view.battery), (-1, -1, -1));
    }

    #[test]
    fn status_numbers_are_rounded_and_options_flattened() {
        let status = Status {
            tdp_watts: None,
            performance_mode: Some(PerformanceMode::Turbo),
            fps: Some(59.6),
            gpu_util: Some(0.874),
            battery_percent: Some(81),
            on_battery: Some(true),
            auto_tdp: AutoTdpSettings {
                battery_max_watts: Some(15),
                ..AutoTdpSettings::default()
            },
            foreground: Some("game.exe".into()),
            bench: Some(BenchProgress {
                step: 2,
                steps: 7,
                watts: 22,
                seconds_left: 110,
            }),
            ..Status::default()
        };
        let view = status_view(Some(&status));
        assert_eq!(view.tdp, -1);
        assert_eq!(view.mode, "turbo");
        assert_eq!((view.fps, view.gpu, view.temp), (60, 87, -1));
        assert!(view.power_known && view.on_battery);
        assert_eq!((view.battery_cap, view.temp_limit), (15, 0));
        assert_eq!(view.foreground, "game.exe");
        assert_eq!(view.game, "game");
        assert_eq!(super::game_name(r"C:\Games\Ashen Tide.EXE"), "Ashen Tide");
        assert_eq!(super::game_name("ashen"), "ashen");
        assert!(view.bench_running);
        assert_eq!((view.bench_step, view.bench_steps), (2, 7));
        assert_eq!(view.gyro_activation, "off");
        assert_eq!(view.gyro_sensitivity, 10);
        assert!(!view.gyro_roll && !view.gyro_invert && !view.gyro_server);
    }

    #[test]
    fn back_buttons_index_the_page_names() {
        let mut status = Status::default();
        status.back_buttons.m2 = ButtonTarget::PadA;
        let view = status_view(Some(&status));
        assert_eq!((view.m1, view.m2), (0, 2));
        assert_eq!(view.lighting_level, -1);
        assert_eq!((view.lighting_effect, view.lighting_speed), (0, 1));
        status.lighting = Some(canape_core::Lighting {
            level: 0,
            color: Some("#ffffff".into()),
            effect: LightingEffect::Rainbow,
            speed: LightingSpeed::Fast,
        });
        let view = status_view(Some(&status));
        assert_eq!(
            (view.lighting_level, view.lighting_color.as_str()),
            (0, "#ffffff")
        );
        assert_eq!((view.lighting_effect, view.lighting_speed), (3, 2));
        // The page names one effect per entry of LightingEffect::ALL.
        let page = include_str!("../ui/hardware.slint");
        let effects = page
            .split("effect-names: [")
            .nth(1)
            .and_then(|rest| rest.split("];").next())
            .unwrap();
        assert_eq!(effects.matches("@tr(").count(), LightingEffect::ALL.len());
        // The page names one target per entry of ButtonTarget::ALL.
        let page = include_str!("../ui/hardware.slint");
        let list = page
            .split("target-names: [")
            .nth(1)
            .and_then(|rest| rest.split("];").next())
            .unwrap();
        let names = list.lines().filter(|l| l.trim().ends_with(',')).count();
        assert_eq!(names, ButtonTarget::ALL.len());
    }

    #[test]
    fn bench_view_formats_rows_and_recommendations() {
        let point = |watts, fps_avg| BenchPoint {
            watts,
            fps_avg,
            fps_low: fps_avg - 5.0,
            gpu_avg: None,
            temp_max: Some(71.4),
            samples: 15,
        };
        let report = BenchReport {
            exe: Some("game.exe".into()),
            started_unix: 0,
            plan: BenchPlan::default(),
            points: vec![point(10, 44.0), point(16, 61.0), point(22, 64.0)],
            completed: true,
        };
        let (view, rows) = bench_view(Some(&report), 60);
        assert!(view.available && view.completed);
        assert_eq!(view.sweet_spot.watts, 16);
        assert_eq!((view.for_target.watts, view.for_target.fps), (16, 61));
        assert_eq!(view.most_efficient.watts, 10);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].fps_per_watt_tenths, 44);
        assert_eq!(rows[0].temp, 71);
        assert_eq!(rows[2].share, 1.0);

        let (view, _) = bench_view(Some(&report), 90);
        assert!(!view.for_target.available);
        assert!(!bench_view(None, 60).0.available);
    }

    #[test]
    fn own_accent_echo_is_ignored_once_then_followed_again() {
        let mut accent = AccentState {
            local: "#ff3d5e".into(),
            sent: Some("#ff3d5e".into()),
        };
        assert!(!accent.adopt("#ff3d5e"), "echo of our own send");
        assert!(accent.adopt("#f59e0b"), "changed elsewhere");
        assert!(accent.adopt("#ff3d5e"), "set back elsewhere");
        assert!(!accent.adopt("#ff3d5e"), "already shown");
    }
}
