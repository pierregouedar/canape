use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::time::{Duration, Instant};

use canape_core::protocol::PIPE_NAME;
use windows_sys::Win32::Foundation::{
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Isolation::DeriveAppContainerSidFromAppContainerName;
use windows_sys::Win32::Security::{FreeSid, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::core::PWSTR;

use crate::Connection;
use crate::acl;

pub type Stream = File;

const BUFFER_SIZE: u32 = 64 * 1024;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The widget's AppContainer SID, as a string (`S-1-15-2-…`): derived by
/// Windows from its package family name, itself computed from the package
/// name and publisher in `widget/CanapeWidget/Package.appxmanifest`.
fn widget_sid() -> Result<String, String> {
    let family = acl::package_family_name(acl::WIDGET_PACKAGE_NAME, acl::WIDGET_PUBLISHER);
    let name = wide(&family);
    let mut sid: PSID = std::ptr::null_mut();
    let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
    if hr < 0 || sid.is_null() {
        return Err(format!(
            "DeriveAppContainerSidFromAppContainerName({family}) returned 0x{hr:08X}"
        ));
    }
    let mut text: PWSTR = std::ptr::null_mut();
    let ok = unsafe { ConvertSidToStringSidW(sid, &mut text) } != 0;
    let result = if ok && !text.is_null() {
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let chars = unsafe { std::slice::from_raw_parts(text, len) };
        let sid = String::from_utf16_lossy(chars);
        unsafe { LocalFree(text.cast()) };
        Ok(sid)
    } else {
        Err(format!(
            "ConvertSidToStringSidW failed: {}",
            io::Error::last_os_error()
        ))
    };
    unsafe { FreeSid(sid) };
    result
}

pub struct Listener {
    name: Vec<u16>,
    descriptor: PSECURITY_DESCRIPTOR,
    first: bool,
    /// Who the widget ACE names, for the service log.
    security: String,
}

// The descriptor is only read after creation.
unsafe impl Send for Listener {}

impl Listener {
    /// The pipe admits the Canape widget's AppContainer only; if its SID
    /// cannot be derived, every AppContainer (`AC`), as before.
    pub fn bind() -> io::Result<Self> {
        let (app, security) = match widget_sid() {
            Ok(sid) => (
                sid.clone(),
                format!("AppContainer access for the widget only ({sid})"),
            ),
            Err(err) => (
                "AC".to_owned(),
                format!("AppContainer access for every app (AC): {err}"),
            ),
        };
        let sddl = wide(&acl::pipe_sddl(&app));
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
            security,
        })
    }

    /// One line for the service log: which AppContainer the pipe admits.
    pub fn security(&self) -> &str {
        &self.security
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
