use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::time::{Duration, Instant};

use canape_core::protocol::PIPE_NAME;
use windows_sys::Win32::Foundation::{
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::Connection;

pub type Stream = File;

/// Full control for SYSTEM and administrators; read/write for interactive
/// users (the FSE shell) and AppContainer apps (the Game Bar widget). The
/// low-integrity label lets the AppContainer widget write to the pipe.
/// Clients cannot create pipe instances, so no one else can serve this name.
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)(A;;GRGW;;;AC)S:(ML;;NW;;;LW)";

const BUFFER_SIZE: u32 = 64 * 1024;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub struct Listener {
    name: Vec<u16>,
    descriptor: PSECURITY_DESCRIPTOR,
    first: bool,
}

// The descriptor is only read after creation.
unsafe impl Send for Listener {}

impl Listener {
    pub fn bind() -> io::Result<Self> {
        let sddl = wide(PIPE_SDDL);
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            name: wide(PIPE_NAME),
            descriptor,
            first: true,
        })
    }

    /// Creates a pipe instance and blocks until a client connects to it.
    pub fn accept(&mut self) -> io::Result<Connection> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        };
        // The first instance fails if another process already owns the name.
        let open_mode = if self.first {
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            PIPE_ACCESS_DUPLEX
        };
        let handle = unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER_SIZE,
                BUFFER_SIZE,
                0,
                &attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        self.first = false;
        let file = unsafe { File::from_raw_handle(handle) };
        let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0;
        if !connected && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            return Err(io::Error::last_os_error());
        }
        Connection::new(file)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        unsafe { LocalFree(self.descriptor) };
    }
}

pub fn connect() -> io::Result<Stream> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_NAME)
        {
            Err(err)
                if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}
