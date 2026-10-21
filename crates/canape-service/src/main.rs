//! Canape background service.
//!
//! For now it runs in console mode: it reads JSON requests from stdin, one per
//! line, and answers on stdout. M2 adds the Windows service entry point and the
//! `\\.\pipe\canape` named pipe server, on top of the same [`handler::Handler`].

mod handler;

use std::io::{self, BufRead, Write};

use canape_core::{Request, Response, protocol};

fn main() -> io::Result<()> {
    let device = canape_hw::detect();
    eprintln!("canape-service: device = {}", device.name());
    let mut handler = handler::Handler::new(device);

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
