//! Canape FSE home app.
//!
//! Activated by the Windows full-screen experience at medium integrity, with
//! `windows.gaming:///home` as its argument (see docs/m1-report.md). It must
//! never require elevation: everything privileged goes through the service.
//!
//! The FSE may activate the home app several times in a row at boot, so only
//! one instance runs and the window goes full screen right away.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod accent;
#[cfg(any(windows, test))]
mod foreground;
#[cfg(windows)]
mod gamepad;
mod history;
mod i18n;
mod instance;
mod link;
#[cfg(any(windows, test))]
mod nav;
mod report;

use std::rc::Rc;
use std::time::Duration;

use canape_core::{AutoTdpSettings, BenchPlan, PerformanceMode, Request};

use slint::{ComponentHandle, Timer, TimerMode};

use crate::link::Link;

slint::include_modules!();

/// Bounds of the wattage steppers; the service clamps to the model's range.
const MIN_WATTS: u32 = 5;
const MAX_WATTS: u32 = 35;
/// Starting point of the manual TDP when leaving a firmware mode, which
/// cannot report its own limits. Valid on every Ally model.
const MANUAL_START_WATTS: u32 = 15;
/// Quiet time after the last accent change before it is saved and shared:
/// a held direction on a slider changes it several times per second.
const ACCENT_SETTLE: Duration = Duration::from_millis(600);

fn log(message: &str) {
    canape_core::log::append(
        &canape_core::log::dir("LOCALAPPDATA").join("shell.log"),
        message,
    );
}

/// Saves the accent in shell.json.
pub(crate) fn save_accent(hex: &str) {
    let mut settings = i18n::ShellSettings::load();
    settings.accent = hex.to_owned();
    if let Err(err) = settings.save() {
        log(&format!("saving shell.json: {err}"));
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    log(&format!(
        "canape-shell {} started, args={args:?}",
        env!("CARGO_PKG_VERSION")
    ));

    if !instance::acquire() {
        log("already running: activation forwarded");
        return Ok(());
    }

    let ui = AppWindow::new()?;
    if std::env::var_os("CANAPE_WINDOWED").is_none() {
        ui.window().set_fullscreen(true);
    }
    ui.global::<App>()
        .set_version(env!("CARGO_PKG_VERSION").into());

    // Translations can only be selected once a component exists.
    let settings = i18n::ShellSettings::load();
    let language = i18n::apply(&settings.language);
    log(&format!("language: {} -> {language}", settings.language));
    ui.global::<App>()
        .set_language(settings.language.as_str().into());
    let accent = accent::normalize(&settings.accent);
    if accent != settings.accent {
        // A preset name from an earlier version, or an unreadable value.
        save_accent(&accent);
    }
    if let Some(color) = accent::parse(&accent) {
        accent::show(&ui, color, None);
    }
    if let Ok(page) = std::env::var("CANAPE_PAGE") {
        // Development aid: open a given page (0 = home).
        ui.set_page(page.parse().unwrap_or(0));
    }

    let link = Link::start(ui.as_weak(), accent);
    connect(&ui, &link);

    #[cfg(windows)]
    {
        gamepad::spawn(ui.as_weak());
        foreground::spawn(link.clone());
        std::thread::spawn(|| log(&canape_hw::arm_frame_metrics()));
    }
    ui.run()
}

/// Changes the auto TDP settings from the last known ones.
fn update_auto(link: &Link, change: impl FnOnce(&mut AutoTdpSettings)) {
    let mut settings = link.status().auto_tdp;
    change(&mut settings);
    link.send(Request::SetAutoTdp { settings });
}

fn step(value: u32, delta: i32, lo: u32, hi: u32) -> u32 {
    value.saturating_add_signed(delta).clamp(lo, hi.max(lo))
}

/// Wires the UI callbacks to service requests.
fn connect(ui: &AppWindow, link: &Link) {
    let app = ui.global::<App>();
    app.on_set_mode({
        let link = link.clone();
        move |mode| {
            let mode = match mode.as_str() {
                "silent" => PerformanceMode::Silent,
                "turbo" => PerformanceMode::Turbo,
                "manual" => {
                    let watts = link.status().tdp_watts.unwrap_or(MANUAL_START_WATTS);
                    link.send(Request::SetTdp { watts });
                    return;
                }
                _ => PerformanceMode::Performance,
            };
            link.send(Request::SetPerformanceMode { mode });
        }
    });
    app.on_change_tdp({
        let link = link.clone();
        move |delta| {
            // Disabled in the UI while the firmware picks the limits.
            if let Some(current) = link.status().tdp_watts {
                let watts = step(current, delta, MIN_WATTS, MAX_WATTS);
                link.send(Request::SetTdp { watts });
            }
        }
    });
    app.on_apply_tdp({
        let link = link.clone();
        move |watts| {
            link.send(Request::SetTdp {
                watts: watts.max(1) as u32,
            })
        }
    });
    app.on_toggle_auto_tdp({
        let link = link.clone();
        move || update_auto(&link, |s| s.enabled = !s.enabled)
    });
    app.on_set_target_fps({
        let link = link.clone();
        move |fps| {
            update_auto(&link, |s| {
                s.enabled = true;
                s.target_fps = fps.max(1) as u32;
            })
        }
    });
    app.on_change_min_watts({
        let link = link.clone();
        move |delta| {
            update_auto(&link, |s| {
                s.min_watts = step(s.min_watts, delta, MIN_WATTS, s.max_watts)
            })
        }
    });
    app.on_change_max_watts({
        let link = link.clone();
        move |delta| {
            update_auto(&link, |s| {
                s.max_watts = step(s.max_watts, delta, s.min_watts.max(MIN_WATTS), MAX_WATTS)
            })
        }
    });
    app.on_set_battery_cap({
        let link = link.clone();
        move |watts| {
            update_auto(&link, |s| {
                s.battery_max_watts = (watts > 0).then_some(watts as u32)
            })
        }
    });
    app.on_set_temp_limit({
        let link = link.clone();
        move |celsius| {
            update_auto(&link, |s| {
                s.max_temp_c = (celsius > 0).then_some(celsius as u32)
            })
        }
    });

    app.on_save_profile({
        let link = link.clone();
        move || {
            let status = link.status();
            if let Some(exe) = status.foreground {
                let learned_watts = status.tdp_watts.filter(|_| status.auto_tdp.enabled);
                link.send(Request::SaveProfile {
                    profile: canape_core::Profile {
                        exe,
                        auto_tdp: status.auto_tdp,
                        learned_watts,
                        ..Default::default()
                    },
                });
            }
        }
    });
    app.on_delete_profile({
        let link = link.clone();
        move |exe| link.send(Request::DeleteProfile { exe: exe.into() })
    });
    app.on_refresh({
        let link = link.clone();
        move || {
            link.send(Request::ListProfiles);
            link.send(Request::GetBenchReport);
        }
    });

    app.on_start_bench({
        let link = link.clone();
        move |step_watts, measure_secs| {
            let plan = BenchPlan {
                step_watts: step_watts.max(1) as u32,
                measure_secs: measure_secs.max(1) as u32,
                ..BenchPlan::default()
            };
            link.send(Request::StartBench { plan });
        }
    });
    app.on_cancel_bench({
        let link = link.clone();
        move || link.send(Request::CancelBench)
    });
    app.on_bench_duration(|step_watts, measure_secs| {
        let plan = BenchPlan {
            step_watts: step_watts.max(1) as u32,
            measure_secs: measure_secs.max(1) as u32,
            ..BenchPlan::default()
        };
        plan.total_secs() as i32
    });
    app.on_apply_profile({
        let link = link.clone();
        move |watts| {
            let Some(report) = link.report() else { return };
            let status = link.status();
            let Some(exe) = report.exe.clone().or(status.foreground) else {
                return;
            };
            let Some(point) = report.points.iter().find(|p| p.watts == watts as u32) else {
                return;
            };
            let rec = report::Recommendation {
                watts: point.watts,
                fps: point.fps_avg,
            };
            let profile = report::profile_for(&report, rec, &exe, status.auto_tdp);
            link.send(Request::SaveProfile { profile });
        }
    });

    app.on_set_language({
        let ui = ui.as_weak();
        move |choice| {
            let mut settings = i18n::ShellSettings::load();
            settings.language = choice.to_string();
            if let Err(err) = settings.save() {
                log(&format!("saving shell.json: {err}"));
            }
            i18n::apply(&settings.language);
            if let Some(ui) = ui.upgrade() {
                ui.global::<App>().set_language(choice);
            }
        }
    });

    // The accent shows at once; it is saved and shared once it settles.
    let settle = Rc::new(Timer::default());
    let picked = {
        let (ui, link) = (ui.as_weak(), link.clone());
        move |color: slint::Color, hsv: Option<(f32, f32, f32)>| {
            let Some(ui) = ui.upgrade() else { return };
            accent::show(&ui, color, hsv);
            link.accent_picked(&accent::to_hex(color));
            let (ui, link) = (ui.as_weak(), link.clone());
            settle.start(TimerMode::SingleShot, ACCENT_SETTLE, move || {
                if let Some(ui) = ui.upgrade() {
                    let hex = ui.global::<Theme>().get_accent_hex();
                    save_accent(&hex);
                    link.send_accent(&hex);
                }
            });
        }
    };
    app.on_pick_accent_hsv({
        let picked = picked.clone();
        move |hue, saturation, value| {
            let (hue, saturation, value) = (
                hue.rem_euclid(360.0),
                saturation.clamp(0.0, 1.0),
                value.clamp(0.0, 1.0),
            );
            let color = slint::Color::from_hsva(hue, saturation, value, 1.0);
            picked(color, Some((hue, saturation, value)));
        }
    });
    app.on_pick_accent(move |hex| {
        if let Some(color) = accent::parse(&hex) {
            picked(color, None);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn steppers_stay_in_bounds() {
        assert_eq!(step(15, 1, 5, 35), 16);
        assert_eq!(step(5, -1, 5, 35), 5);
        assert_eq!(step(35, 1, 5, 35), 35);
        // Min watts cannot pass max watts, and the reverse.
        assert_eq!(step(20, 1, 5, 20), 20);
        assert_eq!(step(7, -1, 7, 35), 7);
    }
}
