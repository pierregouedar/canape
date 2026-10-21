//! IPC server: one thread per client, plus a tick thread that runs auto TDP
//! and pushes the status to subscribed clients once per second.

use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use canape_core::{Request, Response};
use canape_hw::Device;
use canape_ipc::{Connection, Listener, Writer};

use crate::handler::Handler;

const TICK: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct Shared {
    handler: Arc<Mutex<Handler>>,
    subscribers: Arc<Mutex<Vec<Writer>>>,
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
}

/// Binds the pipe and starts serving in background threads.
pub fn spawn(
    device: Box<dyn Device>,
    log: impl Fn(&str) + Send + Sync + 'static,
) -> io::Result<()> {
    let mut listener = Listener::bind()?;
    let shared = Shared {
        handler: Arc::new(Mutex::new(Handler::new(device))),
        subscribers: Arc::default(),
    };
    let log = Arc::new(log);

    let ticker = shared.clone();
    thread::Builder::new().name("tick".into()).spawn(move || {
        loop {
            thread::sleep(TICK);
            // FPS comes from the ETW frame-time source in M4.
            ticker.handler.lock().unwrap().tick(None);
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
                        let log = log.clone();
                        let _ = thread::Builder::new().name("client".into()).spawn(move || {
                            if let Err(err) = serve(conn, &shared) {
                                log(&format!("client error: {err}"));
                            }
                        });
                    }
                    Err(err) => {
                        log(&format!("accept failed: {err}"));
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
            Request::Hello { .. } | Request::GetStatus | Request::Subscribe
        );
        let subscribe = request == Request::Subscribe;
        let response = shared.handler.lock().unwrap().handle(request);
        conn.send(&response)?;
        if subscribe {
            // Windows serializes I/O on a synchronous pipe handle: a pending
            // read here would block every push. Stop reading instead.
            shared.subscribers.lock().unwrap().push(conn.writer()?);
            return Ok(());
        }
        if changes_state {
            shared.broadcast_status();
        }
    }
    Ok(())
}
