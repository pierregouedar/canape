//! Which executable draws the shell.
//!
//! Two executables are installed side by side, built from the same crate:
//! `canape-shell.exe` draws with Slint's software renderer and links no
//! OpenGL; `canape-shell-gpu.exe` draws with FemtoVG on OpenGL. Linking both
//! in one executable costs memory even when the software one is picked, so
//! the choice is made by starting the right executable, before any window
//! exists.
//!
//! `canape-shell.exe` stays the one registered for the full-screen
//! experience. When shell.json asks for the graphics card, it starts
//! `canape-shell-gpu.exe` with the same arguments and exits once the latter
//! has shown its window (a named event). The GPU executable never starts the
//! software one by itself at startup: if its renderer fails it exits with an
//! error, and the software executable, still waiting, resets the setting.

use std::path::PathBuf;
use std::process::Command;
#[cfg(windows)]
use std::time::Duration;

use crate::i18n::{Renderer, ShellSettings};

/// What `canape-shell.exe` does once the GPU executable is up. `false`: it
/// exits, the GPU executable carries on alone. `true`: it stays waiting,
/// without a window, until the GPU executable ends, in case the full-screen
/// experience watches the process it activated.
#[cfg(windows)]
pub const PARENT_STAYS_WAITING: bool = false;

/// How long the GPU executable has to show its window.
#[cfg(windows)]
pub const READY_TIMEOUT: Duration = Duration::from_secs(5);

const SOFTWARE_EXE: &str = "canape-shell.exe";
const GPU_EXE: &str = "canape-shell-gpu.exe";

/// Name of the event the GPU executable sets once its window is shown.
#[cfg(windows)]
const READY_EVENT: &str = "Local\\Canape.Shell.GpuReady";

/// Page of the settings, as `AppWindow.page` numbers it.
pub const SETTINGS_PAGE: u32 = 3;

/// The renderer this executable was built for.
pub fn built() -> Renderer {
    if option_env!("CARGO_BIN_NAME") == Some("canape-shell-gpu") {
        Renderer::Gpu
    } else {
        Renderer::Software
    }
}

/// How the renderer is named in the log.
pub fn describe(renderer: Renderer) -> &'static str {
    match renderer {
        Renderer::Software => "software (processor, GDI copy)",
        Renderer::Gpu => "femtovg (OpenGL)",
    }
}

/// Full name of the package this process runs under, or the error code of
/// the lookup (15700: no package identity).
#[cfg(windows)]
pub fn package_identity() -> Result<String, u32> {
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    let mut len = 0u32;
    let code = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    if code != ERROR_INSUFFICIENT_BUFFER {
        return Err(code);
    }
    let mut buffer = vec![0u16; len as usize];
    let code = unsafe { GetCurrentPackageFullName(&mut len, buffer.as_mut_ptr()) };
    if code != 0 {
        return Err(code);
    }
    buffer.truncate(len.saturating_sub(1) as usize);
    Ok(String::from_utf16_lossy(&buffer))
}

#[cfg(not(windows))]
pub fn package_identity() -> Result<String, u32> {
    Err(0)
}

/// Writes the renderer and the package identity to the log.
pub fn log_startup() {
    crate::log(&format!("renderer: {}", describe(built())));
    match package_identity() {
        Ok(name) => crate::log(&format!("package identity: {name}")),
        Err(code) => crate::log(&format!("package identity: none (error {code})")),
    }
}

/// Path of the executable that draws with `renderer`, next to this one.
fn sibling(renderer: Renderer) -> Option<PathBuf> {
    let name = match renderer {
        Renderer::Software => SOFTWARE_EXE,
        Renderer::Gpu => GPU_EXE,
    };
    let path = std::env::current_exe().ok()?.with_file_name(name);
    path.is_file().then_some(path)
}

fn command(path: &PathBuf, args: &[String], page: Option<u32>) -> Command {
    let mut command = Command::new(path);
    command.args(args);
    if let Some(page) = page {
        command.env("CANAPE_PAGE", page.to_string());
    }
    command
}

/// Saves `renderer` in shell.json.
fn remember(renderer: Renderer) {
    let mut settings = ShellSettings::load();
    settings.renderer = renderer;
    if let Err(err) = settings.save() {
        crate::log(&format!("saving shell.json: {err}"));
    }
}

/// Startup of `canape-shell.exe`: if shell.json asks for the graphics card,
/// starts the GPU executable and waits for it to show its window.
///
/// Returns true when this process is done and should exit. Returns false
/// when it carries on with the software renderer, the instance lock being
/// held again, after resetting the setting if the hand-over failed. Call it
/// before any window or Slint object exists.
pub fn hand_over_at_startup(args: &[String]) -> bool {
    if built() == Renderer::Gpu || ShellSettings::load().renderer != Renderer::Gpu {
        return false;
    }
    match start_gpu(args, None) {
        Ok(()) => true,
        Err(reason) => {
            crate::log(&format!(
                "gpu renderer: {reason}; back to the software renderer"
            ));
            remember(Renderer::Software);
            // The lock was released for the child, or never was (the
            // executable is missing): let go of it either way, then take it
            // back, unless a later activation got it meanwhile.
            crate::instance::release();
            !crate::instance::acquire()
        }
    }
}

/// Starts the GPU executable and waits until it has shown its window. The
/// instance lock is released first so that it can take it; on failure it is
/// not taken back here.
#[cfg(windows)]
fn start_gpu(args: &[String], page: Option<u32>) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, INFINITE, WaitForMultipleObjects, WaitForSingleObject,
    };

    let Some(path) = sibling(Renderer::Gpu) else {
        return Err(format!("{GPU_EXE} not found next to this executable"));
    };
    let name: Vec<u16> = READY_EVENT.encode_utf16().chain(Some(0)).collect();
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    if event.is_null() {
        return Err("cannot create the ready event".into());
    }

    crate::instance::release();
    let mut child = match command(&path, args, page).spawn() {
        Ok(child) => child,
        Err(err) => {
            unsafe { CloseHandle(event) };
            return Err(format!("cannot start {GPU_EXE}: {err}"));
        }
    };
    let handles = [event, child.as_raw_handle()];
    let waited =
        unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, READY_TIMEOUT.as_millis() as u32) };
    unsafe { CloseHandle(event) };
    if waited == WAIT_OBJECT_0 {
        crate::log(&format!("gpu renderer: {GPU_EXE} is ready"));
        if PARENT_STAYS_WAITING {
            unsafe { WaitForSingleObject(child.as_raw_handle(), INFINITE) };
        }
        return Ok(());
    }
    let reason = if waited == WAIT_OBJECT_0 + 1 {
        match child.wait() {
            Ok(status) => format!("{GPU_EXE} ended before showing a window ({status})"),
            Err(err) => format!("{GPU_EXE} ended before showing a window ({err})"),
        }
    } else {
        // Not up in time: it must not take the screen later.
        let _ = child.kill();
        let _ = child.wait();
        format!("{GPU_EXE} showed no window within {READY_TIMEOUT:?}")
    };
    Err(reason)
}

#[cfg(not(windows))]
fn start_gpu(_args: &[String], _page: Option<u32>) -> Result<(), String> {
    Err("switching renderers needs Windows".into())
}

/// Called by the GPU executable once its window is shown.
#[cfg(windows)]
fn signal_ready() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};

    let name: Vec<u16> = READY_EVENT.encode_utf16().chain(Some(0)).collect();
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if event.is_null() {
        // Started by hand, or its parent gave up: nobody is waiting.
        return;
    }
    unsafe {
        SetEvent(event);
        CloseHandle(event);
    }
}

#[cfg(not(windows))]
fn signal_ready() {}

/// In the GPU executable: signals the parent after the first frame is on
/// screen (or at once when the renderer cannot tell).
pub fn signal_when_shown(window: &slint::Window) {
    if built() != Renderer::Gpu {
        return;
    }
    let mut signalled = false;
    let hooked = window.set_rendering_notifier(move |state, _| {
        if !signalled && matches!(state, slint::RenderingState::AfterRendering) {
            signalled = true;
            signal_ready();
        }
    });
    if let Err(err) = hooked {
        crate::log(&format!("renderer notifier: {err}"));
        signal_ready();
    }
}

/// Runtime switch (Settings, tour): saves `target`, starts the other
/// executable on `page` and quits this one. On a failed start the setting
/// goes back, `failed` runs on the event loop and this shell keeps running.
pub fn switch(target: Renderer, page: Option<u32>, failed: impl FnOnce() + Send + 'static) {
    if target == built() {
        remember(target);
        return;
    }
    remember(target);
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::thread::spawn(move || {
        let started = match target {
            Renderer::Gpu => start_gpu(&args, page),
            Renderer::Software => start_software(&args, page),
        };
        match started {
            Ok(()) => {
                crate::log(&format!("renderer: restarting as {}", describe(target)));
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
                });
            }
            Err(reason) => {
                crate::log(&format!("renderer switch: {reason}"));
                remember(built());
                // The lock was released for the other executable.
                crate::instance::acquire();
                let _ = slint::invoke_from_event_loop(failed);
            }
        }
    });
}

/// Starts the software executable; the lock is released first. It does not
/// signal, so there is nothing to wait for.
fn start_software(args: &[String], page: Option<u32>) -> Result<(), String> {
    let Some(path) = sibling(Renderer::Software) else {
        return Err(format!("{SOFTWARE_EXE} not found next to this executable"));
    };
    crate::instance::release();
    command(&path, args, page)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("cannot start {SOFTWARE_EXE}: {err}"))
}

/// The setting as a bool for the interface (true: graphics card).
pub fn is_gpu(settings: &ShellSettings) -> bool {
    settings.renderer == Renderer::Gpu
}
