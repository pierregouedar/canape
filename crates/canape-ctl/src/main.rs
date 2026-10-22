//! `canape-ctl [--pretty] <json request>` sends one request and prints the response.
//! `canape-ctl [--pretty] watch` subscribes and prints every status push.
//!
//! Examples:
//! - `canape-ctl '{"type":"set_tdp","watts":12}'`
//! - `canape-ctl '{"type":"set_fps_limit","fps":40}'`
//! - `canape-ctl --pretty '{"type":"get_fan_curves"}'`

use std::io;
use std::process::ExitCode;

use canape_core::{Request, Response, protocol};

const USAGE: &str = "usage: canape-ctl [--pretty] <json request> | watch";

fn main() -> ExitCode {
    let mut pretty = false;
    let mut request = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if request.is_none() => request = Some(arg),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(arg) = request else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(&arg, pretty) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("canape-ctl: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Returns false when the service answered with an error.
fn run(arg: &str, pretty: bool) -> io::Result<bool> {
    let watch = arg == "watch";
    // Parse before connecting: a typo is reported without touching the service.
    let request = if watch {
        Request::Subscribe
    } else {
        protocol::decode::<Request>(arg)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?
    };

    let mut conn = canape_ipc::connect()?;
    conn.send(&Request::Hello {
        protocol_version: protocol::PROTOCOL_VERSION,
    })?;
    conn.recv::<Response>()?;

    conn.send(&request)?;
    while let Some(response) = conn.recv::<Response>()? {
        println!("{}", format(&response, pretty));
        if !watch {
            return Ok(!matches!(response, Response::Error { .. }));
        }
    }
    Ok(true)
}

fn format(response: &Response, pretty: bool) -> String {
    let line = if pretty {
        serde_json::to_string_pretty(response).ok()
    } else {
        protocol::encode(response).ok()
    };
    line.unwrap_or_default().trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_compact_or_pretty() {
        assert_eq!(format(&Response::Ok, false), r#"{"type":"ok"}"#);
        let pretty = format(
            &Response::Error {
                message: "x".into(),
            },
            true,
        );
        assert_eq!(pretty, "{\n  \"type\": \"error\",\n  \"message\": \"x\"\n}");
    }
}
