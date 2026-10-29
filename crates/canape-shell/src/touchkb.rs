//! The touch keyboard, for typing a name with the controller: shown while
//! one of Canapé's text fields has the focus, hidden after. Windows only
//! shows it by itself after a touch on the field, so Canapé asks for it:
//! the touch keyboard's host (`TabTip.exe`) is started if it is not
//! running, then toggled through its COM interface (`ITipInvocation`). It
//! is only hidden again when it is seen open (Windows 10's `IPTip_Main_Window`,
//! Windows 11's text input host window not cloaked), or when Canapé opened
//! it and cannot tell (the window of a localised Windows is not found by its
//! title, and the old one can stay around invisible). Runs on a short-lived thread: never on the UI thread.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, GWL_STYLE, GetDesktopWindow, GetWindowLongW, GetWindowRect,
        WS_DISABLED, WS_VISIBLE,
    };
    use windows_sys::core::GUID;

    /// `UIHostNoLaunch`: the touch keyboard's host, already running.
    const CLSID_UI_HOST_NO_LAUNCH: GUID = GUID::from_u128(0x4ce576fa_83dc_4f88_951c_9d0782b4e376);
    const IID_ITIP_INVOCATION: GUID = GUID::from_u128(0x37c994e7_432b_4834_a2f7_dce1f13b834b);

    /// `ITipInvocation`: IUnknown, then `Toggle(HWND)`.
    #[repr(C)]
    struct Vtbl {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(*mut Object) -> u32,
        toggle: unsafe extern "system" fn(*mut Object, HWND) -> i32,
    }

    #[repr(C)]
    struct Object {
        vtbl: *const Vtbl,
    }

    /// Canapé opened the keyboard and has not closed it.
    static OPENED: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Whether the touch keyboard is on screen, when that can be told.
    fn visible() -> Option<bool> {
        unsafe {
            let class = wide("IPTip_Main_Window");
            let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
            if !hwnd.is_null() {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                // Windows 11 keeps this window around, invisible and
                // disabled, while the keyboard shows in another one: only a
                // visible one proves anything.
                if style & WS_VISIBLE != 0 && style & WS_DISABLED == 0 {
                    return Some(true);
                }
            }
            let class = wide("Windows.UI.Core.CoreWindow");
            let title = wide("Microsoft Text Input Application");
            let hwnd = FindWindowExW(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                class.as_ptr(),
                title.as_ptr(),
            );
            if !hwnd.is_null() {
                let mut cloaked: u32 = 0;
                let ok = DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED as u32,
                    (&mut cloaked as *mut u32).cast(),
                    size_of::<u32>() as u32,
                ) >= 0;
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(hwnd, &mut rect);
                return ok.then_some(cloaked == 0 && rect.bottom > rect.top);
            }
            None
        }
    }

    /// Toggles the keyboard; false if its host could not be reached.
    fn toggle() -> bool {
        unsafe {
            let mut object: *mut Object = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_UI_HOST_NO_LAUNCH,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER,
                &IID_ITIP_INVOCATION,
                (&mut object as *mut *mut Object).cast(),
            );
            if hr < 0 || object.is_null() {
                crate::log(&format!("touch keyboard: no host ({hr:#x})"));
                return false;
            }
            let hr = ((*(*object).vtbl).toggle)(object, GetDesktopWindow());
            ((*(*object).vtbl).release)(object);
            if hr < 0 {
                crate::log(&format!("touch keyboard: toggle failed ({hr:#x})"));
            }
            hr >= 0
        }
    }

    /// Starts the touch keyboard's host.
    fn start_host() {
        let common = std::env::var("CommonProgramW6432")
            .or_else(|_| std::env::var("CommonProgramFiles"))
            .unwrap_or_else(|_| r"C:\Program Files\Common Files".into());
        let exe = std::path::PathBuf::from(common).join(r"microsoft shared\ink\TabTip.exe");
        if let Err(err) = canape_library::launch(&canape_library::Launch::Exe {
            path: exe.clone(),
            args: String::new(),
            cwd: std::path::PathBuf::new(),
        }) {
            crate::log(&format!("touch keyboard: {}: {err}", exe.display()));
        }
    }

    fn with_com(f: impl FnOnce()) {
        let com = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        f();
        if com >= 0 {
            unsafe { CoUninitialize() };
        }
    }

    pub fn show() {
        std::thread::spawn(|| {
            with_com(|| {
                // Opened by Canapé and not closed yet: a second toggle would
                // close it.
                if OPENED.load(Ordering::Relaxed) || visible() == Some(true) {
                    return;
                }
                let shown = toggle() || {
                    start_host();
                    std::thread::sleep(Duration::from_millis(400));
                    toggle()
                };
                OPENED.store(shown, Ordering::Relaxed);
                crate::log(&format!(
                    "touch keyboard: {}",
                    if shown { "shown" } else { "not available" }
                ));
            })
        });
    }

    pub fn hide() {
        if !OPENED.swap(false, Ordering::Relaxed) {
            return;
        }
        std::thread::spawn(|| {
            with_com(|| match visible() {
                Some(false) => {}
                Some(true) | None => {
                    toggle();
                }
            })
        });
    }
}

#[cfg(windows)]
pub use imp::{hide, show};

/// Development off Windows: nothing to show.
#[cfg(not(windows))]
pub fn show() {}

#[cfg(not(windows))]
pub fn hide() {}
