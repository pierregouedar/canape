//! IPC server: one thread per client, plus a tick thread that runs auto TDP
//! and pushes the status to subscribed clients once per second.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use canape_core::{Request, Response};
use canape_hw::Device;
use canape_ipc::{Connection, Listener, Writer};

use crate::conflicts;
use crate::handler::{Handler, Saved};

const TICK: Duration = Duration::from_secs(1);
/// Conflicting programs are looked for every this many ticks.
const CONFLICT_SCAN_TICKS: u32 = 5;

type Log = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
struct Shared {
    handler: Arc<Mutex<Handler>>,
    subscribers: Arc<Mutex<Vec<Writer>>>,
    state_path: PathBuf,
    log: Log,
}

impl Shared {
    fn broadcast_status(&self) {
        let mut subscribers = self.subscribers.lock().unwrap();
        if subscribers.is_empty() {
            return;
        }
        let status = Response::Status(self.handler.lock().unwrap().status());
        // Drop the clients that went away.
        subscribers.retain_mut(|writer| writer.send(&status).is_ok());
    }

    fn save(&self) {
        let saved = self.handler.lock().unwrap().saved();
        let json = serde_json::to_string_pretty(&saved).expect("state always serializes");
        if let Err(err) = std::fs::write(&self.state_path, json) {
            (self.log)(&format!("cannot save {}: {err}", self.state_path.display()));
        }
    }
}

/// Re-applies the saved state, binds the pipe and starts serving in
/// background threads.
pub fn spawn(
    device: Box<dyn Device>,
    state_path: PathBuf,
    log: impl Fn(&str) + Send + Sync + 'static,
) -> io::Result<()> {
    let log: Log = Arc::new(log);
    let mut handler = Handler::new(device);
    if let Ok(json) = std::fs::read_to_string(&state_path) {
        match serde_json::from_str::<Saved>(&json) {
            Ok(saved) => {
                for error in handler.restore(&saved) {
                    log(&format!("restore: {error}"));
                }
                log(&format!("restored {saved:?}"));
            }
            Err(err) => log(&format!("ignoring {}: {err}", state_path.display())),
        }
    }

    let mut listener = Listener::bind()?;
    #[cfg(windows)]
    log(&format!("pipe: {}", listener.security()));
    let shared = Shared {
        handler: Arc::new(Mutex::new(handler)),
        subscribers: Arc::default(),
        state_path,
        log,
    };

    let ticker = shared.clone();
    thread::Builder::new().name("tick".into()).spawn(move || {
        let mut tick = 0u32;
        loop {
            thread::sleep(TICK);
            let mut handler = ticker.handler.lock().unwrap();
            handler.tick();
            if tick % CONFLICT_SCAN_TICKS == 0 {
                let found = conflicts::detect();
                let message = format!("conflicting programs: {found:?}");
                if handler.set_conflicts(found) {
                    (ticker.log)(&message);
                }
            }
            tick = tick.wrapping_add(1);
            let dirty = handler.take_dirty();
            drop(handler);
            if dirty {
                ticker.save();
            }
            ticker.broadcast_status();
        }
    })?;

    thread::Builder::new()
        .name("accept".into())
        .spawn(move || {
            loop {
                match listener.accept() {
                    Ok(conn) => {
                        let shared = shared.clone();
                        let _ = thread::Builder::new().name("client".into()).spawn(move || {
                            if let Err(err) = serve(conn, &shared) {
                                (shared.log)(&format!("client error: {err}"));
                            }
                        });
                    }
                    Err(err) => {
                        (shared.log)(&format!("accept failed: {err}"));
                        thread::sleep(TICK);
                    }
                }
            }
        })?;
    Ok(())
}

fn serve(mut conn: Connection, shared: &Shared) -> io::Result<()> {
    while let Some(request) = conn.recv::<Request>()? {
        let changes_state = !matches!(
            request,
            Request::Hello { .. }
                | Request::GetStatus
                | Request::Subscribe
                | Request::ListProfiles
                | Request::GetBenchReport
                | Request::GetFanCurves
        );
        let subscribe = request == Request::Subscribe;
        let described = changes_state.then(|| format!("{request:?}"));
        let response = shared.handler.lock().unwrap().handle(request);
        conn.send(&response)?;
        if subscribe {
            // Windows serializes I/O on a synchronous pipe handle: a pending
            // read here would block every push. Stop reading instead.
            shared.subscribers.lock().unwrap().push(conn.writer()?);
            return Ok(());
        }
        if let Some(request) = described {
            (shared.log)(&format!("{request} -> {response:?}"));
            if response == Response::Ok {
                shared.save();
            }
            shared.broadcast_status();
        }
    }
    Ok(())
}
