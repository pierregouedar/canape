//! Single instance: later activations bring the running window to the front
//! and exit.

/// True when this process is the only shell running.
#[cfg(windows)]
pub fn acquire() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    let name: Vec<u16> = "Local\\Canape.Shell"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // The handle is kept for the lifetime of the process on purpose.
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if mutex.is_null() || unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return true;
    }

    let title: Vec<u16> = "Canapé".encode_utf16().chain(Some(0)).collect();
    let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if !window.is_null() {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
    }
    false
}

#[cfg(not(windows))]
pub fn acquire() -> bool {
    true
}

/// True when the shell window has the focus, so gamepad input meant for a
/// game is not acted upon.
#[cfg(windows)]
pub fn is_foreground() -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    pid == unsafe { GetCurrentProcessId() }
}
