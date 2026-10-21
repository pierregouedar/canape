//! Canape FSE home app.
//!
//! M1 goal: this binary, wrapped by the sparse identity package in
//! `packaging/identity`, is activated by the Windows full-screen experience at
//! medium integrity. It must never require elevation: everything privileged
//! goes through the service.
//!
//! Until the UI exists (Slint candidate, see docs/architecture.md), it only
//! proves the activation: it appends a line to `%LOCALAPPDATA%\Canape\shell.log`
//! and shows a message box.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let line = format!(
        "{started} canape-shell {} started, args={args:?}, service pipe {}\n",
        env!("CARGO_PKG_VERSION"),
        canape_core::protocol::PIPE_NAME
    );
    log(&line);

    #[cfg(windows)]
    win::message_box("Canapé", "Canapé a démarré.");
    #[cfg(not(windows))]
    print!("{line}");
}

fn log(line: &str) {
    use std::io::Write;
    let Some(dir) =
        std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Canape"))
    else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("shell.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(windows)]
mod win {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn message_box(caption: &str, text: &str) {
        let (caption, text) = (wide(caption), wide(text));
        // MB_OK | MB_SETFOREGROUND
        unsafe {
            MessageBoxW(
                core::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                0x0001_0000,
            )
        };
    }
}
