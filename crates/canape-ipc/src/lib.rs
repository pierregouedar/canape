//! Local transport for the Canape protocol.
//!
//! One JSON message per line ([`canape_core::protocol`]), over the
//! `\\.\pipe\canape` named pipe on Windows. Elsewhere a Unix socket stands in,
//! so the service and the shell can be run together during development.

use std::io::{self, BufRead, BufReader, Write};

use canape_core::protocol;
use serde::{Serialize, de::DeserializeOwned};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;

pub use sys::Listener;

/// A bidirectional connection; both ends speak through it.
pub struct Connection {
    reader: BufReader<sys::Stream>,
    writer: Writer,
}

impl Connection {
    fn new(stream: sys::Stream) -> io::Result<Self> {
        let writer = Writer(stream.try_clone()?);
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }

    pub fn send<T: Serialize>(&mut self, message: &T) -> io::Result<()> {
        self.writer.send(message)
    }

    /// Next message, or `None` once the peer has disconnected.
    pub fn recv<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            if !line.trim().is_empty() {
                break;
            }
        }
        protocol::decode(&line)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    /// A second handle for pushing messages from another thread.
    pub fn writer(&self) -> io::Result<Writer> {
        Ok(Writer(self.writer.0.try_clone()?))
    }
}

pub struct Writer(sys::Stream);

impl Writer {
    pub fn send<T: Serialize>(&mut self, message: &T) -> io::Result<()> {
        let line = protocol::encode(message)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
        self.0.write_all(line.as_bytes())?;
        self.0.flush()
    }
}

/// Connects to the running service.
pub fn connect() -> io::Result<Connection> {
    Connection::new(sys::connect()?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use canape_core::{Request, Response};

    #[test]
    fn request_and_response_cross_the_socket() {
        let path = std::env::temp_dir().join(format!("canape-test-{}.sock", std::process::id()));
        let mut listener = Listener::bind_at(&path).unwrap();
        let server = std::thread::spawn(move || {
            let mut conn = listener.accept().unwrap();
            let request: Request = conn.recv().unwrap().unwrap();
            assert_eq!(request, Request::GetStatus);
            conn.send(&Response::Ok).unwrap();
            assert!(conn.recv::<Request>().unwrap().is_none());
        });

        let mut client = Connection::new(unix::connect_at(&path).unwrap()).unwrap();
        client.send(&Request::GetStatus).unwrap();
        assert_eq!(client.recv::<Response>().unwrap(), Some(Response::Ok));
        drop(client);
        server.join().unwrap();
    }
}
