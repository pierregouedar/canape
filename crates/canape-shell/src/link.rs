//! Connection to the service: one subscription feeding the UI, and one
//! connection for commands, both reconnecting on their own.

use std::io;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use canape_core::{PerformanceMode, Request, Response, Status, protocol};
use canape_ipc::Connection;
use slint::Weak;

use crate::{AppWindow, StatusView};

const RETRY: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Link {
    commands: Sender<Request>,
    status: Arc<Mutex<Status>>,
}

impl Link {
    pub fn start(ui: Weak<AppWindow>) -> Self {
        let status = Arc::new(Mutex::new(Status::default()));
        let (commands, rx) = mpsc::channel::<Request>();

        let (ui_status, last) = (ui.clone(), status.clone());
        thread::spawn(move || {
            loop {
                if let Err(err) = follow(&ui_status, &last) {
                    crate::log(&format!("service link: {err}"));
                }
                show(&ui_status, None);
                thread::sleep(RETRY);
            }
        });

        thread::spawn(move || {
            let mut conn: Option<Connection> = None;
            for request in rx {
                let message = match send(&mut conn, &request) {
                    Ok(Response::Error { message }) => message,
                    Ok(_) => String::new(),
                    Err(err) => {
                        conn = None;
                        format!("Service injoignable : {err}")
                    }
                };
                let _ = ui.upgrade_in_event_loop(move |ui| ui.set_message(message.into()));
            }
        });

        Self { commands, status }
    }

    pub fn send(&self, request: Request) {
        let _ = self.commands.send(request);
    }

    /// Last status pushed by the service.
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
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
fn follow(ui: &Weak<AppWindow>, last: &Mutex<Status>) -> io::Result<()> {
    let mut conn = open()?;
    conn.send(&Request::Subscribe)?;
    while let Some(response) = conn.recv::<Response>()? {
        if let Response::Status(status) = response {
            *last.lock().unwrap() = status.clone();
            show(ui, Some(status));
        }
    }
    Ok(())
}

fn show(ui: &Weak<AppWindow>, status: Option<Status>) {
    let view = view(status.as_ref());
    let _ = ui.upgrade_in_event_loop(move |ui| ui.set_status(view));
}

fn view(status: Option<&Status>) -> StatusView {
    let dash = || "—".into();
    let Some(s) = status else {
        return StatusView {
            connected: false,
            tdp: dash(),
            mode: "".into(),
            fps: dash(),
            temp: dash(),
            battery: dash(),
            auto_tdp: false,
            target_fps: 0,
        };
    };
    StatusView {
        connected: true,
        tdp: s.tdp_watts.map_or_else(dash, |w| format!("{w} W").into()),
        mode: match s.performance_mode {
            Some(PerformanceMode::Silent) => "silent",
            Some(PerformanceMode::Performance) => "performance",
            Some(PerformanceMode::Turbo) => "turbo",
            Some(PerformanceMode::Manual) => "manual",
            None => "",
        }
        .into(),
        fps: s.fps.map_or_else(dash, |f| format!("{f:.0}").into()),
        temp: s
            .cpu_temp_c
            .map_or_else(dash, |t| format!("{t:.0} °C").into()),
        battery: s
            .battery_percent
            .map_or_else(dash, |b| format!("{b} %").into()),
        auto_tdp: s.auto_tdp.enabled,
        target_fps: s.auto_tdp.target_fps as i32,
    }
}
