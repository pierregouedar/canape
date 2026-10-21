//! Canape FSE home app.
//!
//! Activated by the Windows full-screen experience at medium integrity, with
//! `windows.gaming:///home` as its argument (see docs/m1-report.md). It must
//! never require elevation: everything privileged goes through the service.
//!
//! The FSE may activate the home app several times in a row at boot, so only
//! one instance runs and the window goes full screen right away.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod gamepad;
mod instance;
mod link;
#[cfg(any(windows, test))]
mod nav;

use canape_core::{AutoTdpSettings, PerformanceMode, Request};

slint::include_modules!();

fn log(message: &str) {
    canape_core::log::append(
        &canape_core::log::dir("LOCALAPPDATA").join("shell.log"),
        message,
    );
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

    let link = link::Link::start(ui.as_weak());

    ui.on_set_mode({
        let link = link.clone();
        move |mode| {
            let mode = match mode.as_str() {
                "silent" => PerformanceMode::Silent,
                "turbo" => PerformanceMode::Turbo,
                _ => PerformanceMode::Performance,
            };
            link.send(Request::SetPerformanceMode { mode });
        }
    });
    ui.on_change_tdp({
        let link = link.clone();
        move |delta| {
            let current = link.status().tdp_watts.unwrap_or(15);
            let watts = current.saturating_add_signed(delta);
            link.send(Request::SetTdp { watts });
        }
    });
    ui.on_toggle_auto_tdp({
        let link = link.clone();
        move || {
            let mut settings = link.status().auto_tdp;
            settings.enabled = !settings.enabled;
            link.send(Request::SetAutoTdp { settings });
        }
    });
    ui.on_set_target_fps({
        let link = link.clone();
        move |fps| {
            let settings = AutoTdpSettings {
                enabled: true,
                target_fps: fps.max(1) as u32,
                ..link.status().auto_tdp
            };
            link.send(Request::SetAutoTdp { settings });
        }
    });

    #[cfg(windows)]
    {
        gamepad::spawn(ui.as_weak());
        std::thread::spawn(|| log(&canape_hw::arm_frame_metrics()));
    }
    ui.run()
}
