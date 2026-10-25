//! Small Windows helpers: registry reads, drive letters, time zone.

use std::collections::HashMap;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
    RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A `REG_SZ` value.
pub fn reg_string(root: HKEY, path: &str, name: &str) -> Option<String> {
    let (path, name) = (wide(path), wide(name));
    let mut len = 0u32;
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut len,
        )
    };
    if status != ERROR_SUCCESS || len == 0 {
        return None;
    }
    let mut buf = vec![0u16; (len as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    (status == ERROR_SUCCESS).then(|| from_wide(&buf))
}

/// A `REG_DWORD` value.
pub fn reg_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
    let (path, name) = (wide(path), wide(name));
    let mut value = 0u32;
    let mut len = 4u32;
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut len,
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

/// An open process handle, closed on drop.
struct Process(windows_sys::Win32::Foundation::HANDLE);

impl Process {
    fn open(pid: u32) -> Option<Process> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        (!handle.is_null()).then_some(Process(handle))
    }

    /// Waits up to `ms` milliseconds; true once the process has ended.
    fn wait(&self, ms: u32) -> bool {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        unsafe { WaitForSingleObject(self.0, ms) == WAIT_OBJECT_0 }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// The full path of the process `pid`'s executable, if it can be read.
fn image_path(process: &Process) -> Option<String> {
    use windows_sys::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
    };
    (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

/// True when a running program's executable path contains `fragment`
/// (compared without case), e.g. a game under `\steamapps\common\`.
pub fn any_process_under(fragment: &str) -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let fragment = fragment.to_lowercase();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while ok && !found {
        found = Process::open(entry.th32ProcessID)
            .and_then(|p| image_path(&p))
            .is_some_and(|path| path.to_lowercase().contains(&fragment));
        ok = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { windows_sys::Win32::Foundation::CloseHandle(snapshot) };
    found
}

/// True when the process `pid` runs and its executable is named `exe`.
pub fn process_named(pid: u32, exe: &str) -> bool {
    use windows_sys::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    let Some(process) = Process::open(pid) else {
        return false;
    };
    if process.wait(0) {
        return false;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
    };
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    ok != 0
        && path
            .rsplit(['\\', '/'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(exe))
}

/// Waits up to `timeout` for the process `pid` to end; true when it has
/// (or is already gone).
pub fn wait_process(pid: u32, timeout: std::time::Duration) -> bool {
    match Process::open(pid) {
        Some(process) => process.wait(u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX)),
        None => true,
    }
}

struct Key(HKEY);

impl Key {
    fn open(root: HKEY, path: &str) -> Option<Key> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut key) };
        (status == ERROR_SUCCESS).then_some(Key(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

/// Names of the subkeys of `path`.
pub fn reg_subkeys(root: HKEY, path: &str) -> Vec<String> {
    let Some(key) = Key::open(root, path) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for index in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        names.push(from_wide(&buf[..len as usize]));
    }
    names
}

/// String values (`REG_SZ`, `REG_EXPAND_SZ` unexpanded) of the key `path`.
pub fn reg_values(root: HKEY, path: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let Some(key) = Key::open(root, path) else {
        return values;
    };
    for index in 0.. {
        let mut name = [0u16; 256];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u16; 2048];
        let mut data_len = (data.len() * 2) as u32;
        let mut kind = 0u32;
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                &mut kind,
                data.as_mut_ptr().cast(),
                &mut data_len,
            )
        };
        if status != ERROR_SUCCESS {
            // ERROR_MORE_DATA (a value over 4 KB) also ends here: GOG keys
            // hold short paths and names.
            if status == windows_sys::Win32::Foundation::ERROR_MORE_DATA {
                continue;
            }
            break;
        }
        if kind == REG_SZ || kind == REG_EXPAND_SZ {
            let len = (data_len as usize / 2).min(data.len());
            values.insert(
                from_wide(&name[..name_len as usize]),
                from_wide(&data[..len]),
            );
        }
    }
    values
}

/// Letters of the existing drives.
pub fn drives() -> Vec<char> {
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| char::from(b'A' + i))
        .collect()
}

/// Seconds east of UTC, daylight saving included.
pub fn utc_offset() -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // windows-sys keeps it under SystemServices, a large feature.
    const TIME_ZONE_ID_DAYLIGHT: u32 = 2;
    let mut info: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    // Bias is in minutes, UTC = local + bias.
    let mut bias = i64::from(info.Bias);
    if id == TIME_ZONE_ID_DAYLIGHT {
        bias += i64::from(info.DaylightBias);
    }
    -bias * 60
}

/// A visible top-level window: its handle, its title and the file name of
/// the program that owns it.
pub struct Window {
    handle: windows_sys::Win32::Foundation::HWND,
    pub title: String,
    pub exe: String,
}

/// The visible top-level windows that have a title.
pub fn windows() -> Vec<Window> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    unsafe extern "system" fn collect(handle: HWND, data: LPARAM) -> i32 {
        let found = unsafe { &mut *(data as *mut Vec<(HWND, String)>) };
        if unsafe { IsWindowVisible(handle) } != 0 {
            let mut buf = [0u16; 512];
            let len = unsafe { GetWindowTextW(handle, buf.as_mut_ptr(), buf.len() as i32) };
            if len > 0 {
                found.push((handle, String::from_utf16_lossy(&buf[..len as usize])));
            }
        }
        1
    }

    let mut found: Vec<(HWND, String)> = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut found as *mut _ as LPARAM) };
    found
        .into_iter()
        .map(|(handle, title)| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(handle, &mut pid) };
            let exe = Process::open(pid)
                .and_then(|p| image_path(&p))
                .and_then(|path| path.rsplit(['\\', '/']).next().map(str::to_owned))
                .unwrap_or_default();
            Window { handle, title, exe }
        })
        .collect()
}

/// Restores `window` and gives it the foreground. The caller's process
/// needs recent input of its own for Windows to allow it.
pub fn raise(window: &Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    unsafe {
        ShowWindow(window.handle, SW_RESTORE);
        SetForegroundWindow(window.handle);
    }
}
