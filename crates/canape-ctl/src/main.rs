//! `canape-ctl <json request>` sends one request and prints the response.
//! `canape-ctl watch` subscribes and prints every status push.
//!
//! Example: `canape-ctl '{"type":"set_tdp","watts":12}'`

use std::io;
use std::process::ExitCode;

use canape_core::{Request, Response, protocol};

fn main() -> ExitCode {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: canape-ctl <json request> | watch");
        return ExitCode::from(2);
    };
    match run(&arg) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("canape-ctl: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Returns false when the service answered with an error.
fn run(arg: &str) -> io::Result<bool> {
    let mut conn = canape_ipc::connect()?;
    conn.send(&Request::Hello {
        protocol_version: protocol::PROTOCOL_VERSION,
    })?;
    conn.recv::<Response>()?;

    let watch = arg == "watch";
    let request = if watch {
        Request::Subscribe
    } else {
        protocol::decode::<Request>(arg)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?
    };
    conn.send(&request)?;
    while let Some(response) = conn.recv::<Response>()? {
        println!(
            "{}",
            protocol::encode(&response).unwrap_or_default().trim_end()
        );
        if !watch {
            return Ok(!matches!(response, Response::Error { .. }));
        }
    }
    Ok(true)
}
