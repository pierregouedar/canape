//! Single instance: later activations bring the running window to the front
//! and exit.

/// The lock held by this process (a mutex handle), 0 when none.
#[cfg(windows)]
static LOCK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// True when this process is the only shell running.
#[cfg(windows)]
pub fn acquire() -> bool {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\Canape.Shell"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // The handle is kept until `release`, or the end of the process.
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if mutex.is_null() {
        return true;
    }
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        LOCK.store(mutex as usize, Ordering::SeqCst);
        return true;
    }
    unsafe { windows_sys::Win32::Foundation::CloseHandle(mutex) };

    // This process was started by an activation (the FSE, canape:// from
    // the widget) but may not take the foreground for the running shell:
    // bring_to_front gets it the right first.
    bring_to_front();
    false
}

#[cfg(not(windows))]
pub fn acquire() -> bool {
    true
}

/// Gives the lock up, so that the shell this one starts in its place can
/// take it.
#[cfg(windows)]
pub fn release() {
    use std::sync::atomic::Ordering;

    let mutex = LOCK.swap(0, Ordering::SeqCst);
    if mutex != 0 {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(mutex as _) };
    }
}

#[cfg(not(windows))]
pub fn release() {}

/// Brings the shell window to the front from the background (a button
/// press while a game runs).
#[cfg(windows)]
pub fn bring_to_front() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    let title: Vec<u16> = "Canapé".encode_utf16().chain(Some(0)).collect();
    let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if window.is_null() {
        return;
    }
    // Without input of its own, a background process may not take the
    // foreground: the window would only flash in the taskbar.
    crate::buttons::tap_alt();
    unsafe {
        ShowWindow(window, SW_RESTORE);
        SetForegroundWindow(window);
    }
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
