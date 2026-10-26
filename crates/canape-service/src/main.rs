//! Canape background service.
//!
//! - no argument: started by the Windows service manager (falls back to
//!   `--console` when launched by hand);
//! - `--console`: serve the pipe in the foreground, on any OS;
//! - `--stdio`: answer JSON requests read on stdin, for quick tests;
//! - `install` / `uninstall` (Windows, elevated): register the service.

mod bench_run;
mod conflicts;
mod handler;
mod server;
mod tweaks;
mod ui_power;
#[cfg(windows)]
mod winsvc;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use canape_core::{Request, Response, protocol};

fn data_dir() -> PathBuf {
    canape_core::log::dir("ProgramData")
}

/// Last settings applied, re-applied at startup.
pub fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

fn log_path() -> PathBuf {
    data_dir().join("service.log")
}

pub fn log(message: &str) {
    canape_core::log::append(&log_path(), message);
}

fn main() -> io::Result<()> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--console") => console(),
        Some("--stdio") => stdio(),
        #[cfg(windows)]
        Some("install") => winsvc::install().map_err(io::Error::other),
        #[cfg(windows)]
        Some("uninstall") => winsvc::uninstall().map_err(io::Error::other),
        #[cfg(windows)]
        None => match winsvc::run_dispatcher() {
            Ok(()) => Ok(()),
            // Not started by the service manager: run in the foreground.
            Err(_) => console(),
        },
        #[cfg(not(windows))]
        None => console(),
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown argument {other:?}"),
        )),
    }
}

fn console() -> io::Result<()> {
    let device = canape_hw::detect();
    eprintln!(
        "canape-service: device = {}, serving {}",
        device.name(),
        protocol::PIPE_NAME
    );
    server::spawn(device, tweaks::system(), state_path(), |message| {
        eprintln!("canape-service: {message}")
    })?;
    loop {
        std::thread::park();
    }
}

fn stdio() -> io::Result<()> {
    let device = canape_hw::detect();
    eprintln!("canape-service: device = {}", device.name());
    let mut handler = handler::Handler::with_system(device, tweaks::system());

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match protocol::decode::<Request>(&line) {
            Ok(request) => handler.handle(request),
            Err(err) => Response::Error {
                message: err.to_string(),
            },
        };
        let encoded = protocol::encode(&response).expect("responses always serialize");
        out.write_all(encoded.as_bytes())?;
        out.flush()?;
    }
    Ok(())
}
