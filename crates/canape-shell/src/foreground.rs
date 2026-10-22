//! Foreground game reporting: the service runs in session 0 and cannot see
//! the user's windows, so the shell tells it which program is in front.
//!
//! A program counts as the game when its window fills the screen (full
//! screen or borderless) or when it has a profile: a browser or a chat app
//! in an ordinary window is not a game, and leaves the game behind it in
//! place.
//!
//! The decision logic is pure ([`Tracker`]); the Windows polling is at the
//! bottom.

/// Windows shell hosts and overlays: when one of them is in front, the game
/// behind it is still the one being played.
const HOSTS: &[&str] = &[
    "explorer.exe",
    "applicationframehost.exe",
    "shellexperiencehost.exe",
    "shellhost.exe",
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "textinputhost.exe",
    "lockapp.exe",
    "gamebar.exe",
    "gamebarftserver.exe",
    "xboxpcappft.exe",
    "taskmgr.exe",
];

/// True for programs that never count as the foreground game.
pub fn is_host(exe: &str) -> bool {
    HOSTS.iter().any(|host| host.eq_ignore_ascii_case(exe))
}

/// What the foreground poll saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// The shell itself, a Windows host, or nothing identifiable.
    Ignored,
    /// A full-screen program, or one with a profile.
    Program(String),
    /// Any other program, in an ordinary window.
    Windowed(String),
}

/// Remembers the last program reported and decides when to report again.
#[derive(Debug, Default)]
pub struct Tracker {
    current: Option<String>,
}

impl Tracker {
    /// New value to send, if the foreground game changed. `alive` tells
    /// whether the last reported program is still running: when the user
    /// comes back to the shell after quitting a game, the game is gone.
    pub fn update(&mut self, seen: Seen, alive: bool) -> Option<Option<String>> {
        let next = match seen {
            Seen::Program(exe) if !is_host(&exe) => Some(exe),
            _ if alive => self.current.clone(),
            _ => None,
        };
        let changed = !match (&next, &self.current) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a.is_none() && b.is_none(),
        };
        changed.then(|| {
            self.current = next.clone();
            next
        })
    }
}

/// File name of a full image path: `C:\Games\Foo\foo.exe` -> `foo.exe`.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

#[cfg(windows)]
pub use self::windows::spawn;

#[cfg(windows)]
mod windows {
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    };

    use super::{Seen, Tracker, file_name};
    use crate::link::Link;

    const POLL: Duration = Duration::from_secs(2);

    /// Owned process handle, closed on drop.
    struct Process(HANDLE);

    impl Drop for Process {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Process {
        fn open(pid: u32) -> Option<Self> {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            (!handle.is_null()).then_some(Self(handle))
        }

        fn exe(&self) -> Option<String> {
            let mut buffer = [0u16; 1024];
            let mut len = buffer.len() as u32;
            let ok = unsafe {
                QueryFullProcessImageNameW(
                    self.0,
                    PROCESS_NAME_WIN32,
                    buffer.as_mut_ptr(),
                    &mut len,
                )
            };
            (ok != 0)
                .then(|| file_name(&String::from_utf16_lossy(&buffer[..len as usize])).to_owned())
        }

        fn alive(&self) -> bool {
            unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
        }
    }

    /// True when the window covers its whole monitor: full screen or
    /// borderless, as games run.
    fn fills_screen(window: HWND) -> bool {
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        if unsafe { GetWindowRect(window, &mut rect) } == 0 {
            return false;
        }
        let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return false;
        }
        let screen = info.rcMonitor;
        rect.left <= screen.left
            && rect.top <= screen.top
            && rect.right >= screen.right
            && rect.bottom >= screen.bottom
    }

    /// The foreground process, unless it is the shell itself, and whether
    /// its window fills the screen.
    fn foreground() -> Option<(Process, String, bool)> {
        let window = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
            return None;
        }
        let process = Process::open(pid)?;
        let exe = process.exe()?;
        Some((process, exe, fills_screen(window)))
    }

    pub fn spawn(link: Link) {
        thread::spawn(move || {
            let mut tracker = Tracker::default();
            // Kept open so a quit game is noticed even if its pid is reused.
            let mut game: Option<Process> = None;
            loop {
                let (seen, process) = match foreground() {
                    Some((process, exe, full)) if full || link.has_profile(&exe) => {
                        (Seen::Program(exe), Some(process))
                    }
                    Some((_, exe, _)) => (Seen::Windowed(exe), None),
                    None => (Seen::Ignored, None),
                };
                let alive = game.as_ref().is_some_and(Process::alive);
                let program = matches!(&seen, Seen::Program(exe) if !super::is_host(exe));
                if let Some(exe) = tracker.update(seen, alive) {
                    link.set_foreground(exe);
                }
                if program {
                    game = process;
                } else if !alive {
                    game = None;
                }
                thread::sleep(POLL);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(exe: &str) -> Seen {
        Seen::Program(exe.into())
    }

    #[test]
    fn reports_changes_only() {
        let mut t = Tracker::default();
        assert_eq!(
            t.update(program("game.exe"), false),
            Some(Some("game.exe".into()))
        );
        assert_eq!(t.update(program("game.exe"), true), None);
        assert_eq!(t.update(program("GAME.EXE"), true), None);
        assert_eq!(
            t.update(program("other.exe"), true),
            Some(Some("other.exe".into()))
        );
    }

    #[test]
    fn hosts_and_the_shell_keep_the_running_game() {
        let mut t = Tracker::default();
        t.update(program("game.exe"), false);
        assert_eq!(t.update(program("explorer.exe"), true), None);
        assert_eq!(t.update(program("GameBar.exe"), true), None);
        assert_eq!(t.update(Seen::Ignored, true), None);
    }

    #[test]
    fn a_quit_game_is_cleared() {
        let mut t = Tracker::default();
        t.update(program("game.exe"), false);
        assert_eq!(t.update(Seen::Ignored, false), Some(None));
        assert_eq!(t.update(Seen::Ignored, false), None);
    }

    #[test]
    fn nothing_is_sent_before_a_game_shows_up() {
        let mut t = Tracker::default();
        assert_eq!(t.update(Seen::Ignored, false), None);
        assert_eq!(t.update(program("explorer.exe"), false), None);
    }

    #[test]
    fn a_windowed_program_is_not_a_game() {
        let mut t = Tracker::default();
        assert_eq!(t.update(Seen::Windowed("msedge.exe".into()), false), None);
        t.update(program("game.exe"), false);
        // Discord in a window over the game: the game stays.
        assert_eq!(t.update(Seen::Windowed("discord.exe".into()), true), None);
        // The game quit, the browser is in front: no game.
        assert_eq!(
            t.update(Seen::Windowed("msedge.exe".into()), false),
            Some(None)
        );
    }

    #[test]
    fn file_name_strips_the_directory() {
        assert_eq!(
            file_name(r"C:\Games\Elden Ring\eldenring.exe"),
            "eldenring.exe"
        );
        assert_eq!(file_name("game.exe"), "game.exe");
    }
}
