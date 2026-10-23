//! Paces the frames on the screen's refresh (Windows).
//!
//! The software renderer draws while the window handles WM_PAINT and copies
//! the frame to the window at once, while a timer of whole milliseconds
//! (8 ms at 120 Hz) asks for the next one: frames drift against the
//! screen, so an animation now and then shows a frame twice or skips one.
//! After each WM_PAINT, waiting for the next composition of the desktop
//! puts every frame on its own refresh. Without an animation nothing is
//! painted, so nothing waits. `CANAPE_NO_VSYNC` turns it off, to compare.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, FindWindowW, GWLP_WNDPROC, GetWindowThreadProcessId, SetWindowLongPtrW,
        WM_PAINT, WNDPROC,
    };

    /// The window procedure replaced, as an address.
    static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn paced(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: PREVIOUS holds the procedure SetWindowLongPtrW returned
        // for this window, set before this procedure was installed.
        let previous: WNDPROC = unsafe { std::mem::transmute(PREVIOUS.load(Ordering::Relaxed)) };
        let result = unsafe { CallWindowProcW(previous, window, message, wparam, lparam) };
        if message == WM_PAINT {
            unsafe { DwmFlush() };
        }
        result
    }

    /// Installs the pacing on the shell window; false if it is not found.
    pub fn install() -> bool {
        let title: Vec<u16> = "Canapé".encode_utf16().chain(Some(0)).collect();
        let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if window.is_null() {
            return false;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid != unsafe { GetCurrentProcessId() } {
            return false;
        }
        let previous =
            unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, paced as *const () as isize) };
        PREVIOUS.store(previous, Ordering::Relaxed);
        previous != 0
    }
}

/// Paces the shell window's frames on the screen, unless `CANAPE_NO_VSYNC`
/// is set. Call once the window exists.
pub fn install() {
    if std::env::var_os("CANAPE_NO_VSYNC").is_some() {
        crate::log("vsync: off (CANAPE_NO_VSYNC)");
    } else if gpu_renderer() {
        // The GPU renderers of the test build wait for the screen themselves.
    } else {
        #[cfg(windows)]
        crate::log(if imp::install() {
            "vsync: frames paced on the screen"
        } else {
            "vsync: shell window not found"
        });
    }
}

/// The test build runs one of its GPU renderers.
fn gpu_renderer() -> bool {
    cfg!(feature = "gpu-renderers")
        && std::env::var("CANAPE_RENDERER").is_ok_and(|name| name != "software")
}
