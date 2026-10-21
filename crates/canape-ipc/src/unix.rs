use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use crate::Connection;

pub type Stream = UnixStream;

/// `$CANAPE_SOCKET`, else `$XDG_RUNTIME_DIR/canape.sock`, else the temp dir.
fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CANAPE_SOCKET") {
        return path.into();
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("canape.sock")
}

pub struct Listener(UnixListener);

impl Listener {
    pub fn bind() -> io::Result<Self> {
        Self::bind_at(&socket_path())
    }

    pub fn bind_at(path: &Path) -> io::Result<Self> {
        // A socket file left by a previous run would make bind fail.
        let _ = std::fs::remove_file(path);
        Ok(Self(UnixListener::bind(path)?))
    }

    pub fn accept(&mut self) -> io::Result<Connection> {
        let (stream, _) = self.0.accept()?;
        Connection::new(stream)
    }
}

pub fn connect() -> io::Result<Stream> {
    connect_at(&socket_path())
}

pub fn connect_at(path: &Path) -> io::Result<Stream> {
    UnixStream::connect(path)
}
