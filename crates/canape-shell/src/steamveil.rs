//! Keeps Steam's "Launching…" window off the screen when a game starts from
//! Canapé.
//!
//! Steam shows a small dialog (class `SDL_app`, about 600 x 286, owned by
//! `steamwebhelper`) for a few seconds while the game starts, and it takes
//! the foreground from the shell. Observed on the Ally: minimizing it right
//! away does not stop the game from starting. Only windows of that kind are
//! touched: small, of Steam's own processes, and only for a short time after
//! a launch from the shell, so the Steam client the user opens is left alone.

// The watch itself only runs on Windows; the rules are tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
use std::time::Duration;

/// How long after a launch the watch stays on: a cold Steam needs about
/// seven seconds before the dialog shows, then it stays about three.
#[cfg(windows)]
pub const WATCH: Duration = Duration::from_secs(40);

/// The window class of Steam's dialogs.
const CLASS: &str = "SDL_app";
const OWNERS: [&str; 2] = ["steamwebhelper.exe", "steam.exe"];
/// Steam's launch dialog is small; its client window is not.
const MAX_SIZE: (i32, i32) = (800, 400);

/// True for Steam's launch dialog: its class, one of its processes, small.
pub fn is_launch_dialog(class: &str, exe: &str, width: i32, height: i32) -> bool {
    class == CLASS
        && OWNERS.iter().any(|owner| owner.eq_ignore_ascii_case(exe))
        && (1..=MAX_SIZE.0).contains(&width)
        && (1..=MAX_SIZE.1).contains(&height)
}

/// True when a launch goes through Steam.
pub fn through_steam(launch: &canape_library::Launch) -> bool {
    match launch {
        canape_library::Launch::Uri { uri } => uri.starts_with("steam://"),
        canape_library::Launch::Exe { args, .. } => args.contains("steam://"),
        _ => false,
    }
}

/// Starts the watch: for [`WATCH`], Steam's launch dialog is minimized as
/// soon as it shows.
#[cfg(windows)]
pub fn watch() {
    windows::watch();
}

#[cfg(not(windows))]
pub fn watch() {}

#[cfg(windows)]
mod windows {
    use std::thread;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SW_MINIMIZE, ShowWindow,
    };

    use super::{WATCH, is_launch_dialog};

    const POLL: Duration = Duration::from_millis(50);

    pub fn watch() {
        thread::spawn(|| {
            let end = Instant::now() + WATCH;
            while Instant::now() < end {
                unsafe { EnumWindows(Some(visit), 0) };
                thread::sleep(POLL);
            }
        });
    }

    fn exe_of(pid: u32) -> Option<String> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
        };
        unsafe { CloseHandle(handle) };
        (ok != 0).then(|| {
            let path = String::from_utf16_lossy(&buffer[..len as usize]);
            path.rsplit(['\\', '/']).next().unwrap_or("").to_owned()
        })
    }

    unsafe extern "system" fn visit(window: HWND, _: LPARAM) -> i32 {
        if unsafe { IsWindowVisible(window) } == 0 || unsafe { IsIconic(window) } != 0 {
            return 1;
        }
        let mut class = [0u16; 64];
        let len = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
        if len <= 0 {
            return 1;
        }
        let class = String::from_utf16_lossy(&class[..len as usize]);
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        if unsafe { GetWindowRect(window, &mut rect) } == 0 {
            return 1;
        }
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if class != super::CLASS || width > super::MAX_SIZE.0 || height > super::MAX_SIZE.1 {
            return 1;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if let Some(exe) = exe_of(pid)
            && is_launch_dialog(&class, &exe, width, height)
        {
            crate::log(&format!("steam: launch dialog minimized ({exe})"));
            unsafe { ShowWindow(window, SW_MINIMIZE) };
        }
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_library::Launch;

    #[test]
    fn only_steams_small_launch_dialog_matches() {
        assert!(is_launch_dialog("SDL_app", "steamwebhelper.exe", 600, 286));
        assert!(is_launch_dialog("SDL_app", "Steam.exe", 600, 286));
        // The client window is large; other programs may share the class.
        assert!(!is_launch_dialog(
            "SDL_app",
            "steamwebhelper.exe",
            1280,
            720
        ));
        assert!(!is_launch_dialog("SDL_app", "game.exe", 600, 286));
        assert!(!is_launch_dialog(
            "UnityWndClass",
            "steamwebhelper.exe",
            600,
            286
        ));
        assert!(!is_launch_dialog("SDL_app", "steam.exe", 0, 0));
    }

    #[test]
    fn steam_launches_are_recognised() {
        assert!(through_steam(&Launch::Uri {
            uri: "steam://rungameid/1".into()
        }));
        assert!(through_steam(&Launch::Exe {
            path: "steam.exe".into(),
            args: "-silent steam://rungameid/1".into(),
            cwd: ".".into(),
        }));
        assert!(!through_steam(&Launch::Uri {
            uri: "com.epicgames.launcher://x".into()
        }));
    }
}
