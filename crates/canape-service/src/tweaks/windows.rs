//! The real system: the registry (the console user's hive for per-user
//! values), Microsoft Defender through its PowerShell module, and the game
//! folders of the stores.
//!
//! PowerShell only runs the constant scripts below; folder lists reach it
//! through an environment variable, never inside the script text.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, REG_DWORD, REG_MULTI_SZ, REG_SZ, RRF_NOEXPAND,
    RRF_RT_ANY, RegCloseKey, RegDeleteKeyValueW, RegEnumKeyExW, RegEnumValueW, RegGetValueW,
    RegOpenKeyExW, RegSetKeyValueW,
};
use windows_sys::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use windows_sys::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, GetLastError};
use windows_sys::Win32::System::Power::{
    PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerSetActiveScheme,
    PowerWriteACValueIndex, PowerWriteDCValueIndex,
};
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, ControlService,
    OpenSCManagerW, OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW,
    QueryServiceStatus, SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
    SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONTROL_STOP, SERVICE_DELAYED_AUTO_START_INFO,
    SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_START, SERVICE_STATUS,
    SERVICE_STOP, SERVICE_STOPPED, StartServiceW,
};
use windows_sys::core::GUID;

use super::power::{PROCESSOR_SUBGROUP, parse_guid};
use super::{PowerSetting, PowerSource, RegData, Scope, ServiceState, System};

const DEFENDER: &str = r"SOFTWARE\Microsoft\Windows Defender";
const DEFENDER_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows Defender";
/// SID of LocalSystem: its hive is not the user's.
const SYSTEM_SID: &str = "S-1-5-18";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const ADD_EXCLUSIONS: &str = "$ErrorActionPreference = 'Stop'; \
    Add-MpPreference -ExclusionPath ($env:CANAPE_FOLDERS -split [char]10)";
const REMOVE_EXCLUSIONS: &str = "$ErrorActionPreference = 'Stop'; \
    Remove-MpPreference -ExclusionPath ($env:CANAPE_FOLDERS -split [char]10)";
const REALTIME_OFF: &str =
    "$ErrorActionPreference = 'Stop'; Set-MpPreference -DisableRealtimeMonitoring $true";
const REALTIME_ON: &str =
    "$ErrorActionPreference = 'Stop'; Set-MpPreference -DisableRealtimeMonitoring $false";

#[derive(Default)]
pub struct WinSystem;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(s: &[u16]) -> String {
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..end])
}

/// SID of a token's user, as `S-1-5-…`.
fn token_sid(token: HANDLE) -> Option<String> {
    let mut len = 0u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
    if len == 0 {
        return None;
    }
    // u64 words keep the TOKEN_USER aligned.
    let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
    let ok =
        unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len) };
    if ok == 0 {
        return None;
    }
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return None;
    }
    let mut n = 0;
    while unsafe { *text.add(n) } != 0 {
        n += 1;
    }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, n) });
    unsafe { LocalFree(text.cast()) };
    Some(sid)
}

/// The user signed in at the console: the service (LocalSystem) asks for
/// that session's token; launched by hand, it is the user running it.
fn console_user() -> Result<String, String> {
    let mut token: HANDLE = std::ptr::null_mut();
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    let from_session =
        session != u32::MAX && unsafe { WTSQueryUserToken(session, &mut token) } != 0;
    if !from_session
        && unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0
    {
        return Err("cannot read the signed-in user".into());
    }
    let sid = token_sid(token);
    unsafe { CloseHandle(token) };
    match sid {
        Some(sid) if sid != SYSTEM_SID => Ok(sid),
        _ => Err("no user is signed in".into()),
    }
}

fn root(scope: Scope, key: &str) -> Result<(HKEY, String), String> {
    match scope {
        Scope::Machine => Ok((HKEY_LOCAL_MACHINE, key.to_owned())),
        Scope::User => Ok((HKEY_USERS, format!(r"{}\{key}", console_user()?))),
    }
}

fn get(hkey: HKEY, key: &str, name: &str) -> Result<Option<RegData>, String> {
    let (k, n) = (wide(key), wide(name));
    let mut kind = 0u32;
    let mut size = 0u32;
    let flags = RRF_RT_ANY | RRF_NOEXPAND;
    let status = unsafe {
        RegGetValueW(
            hkey,
            k.as_ptr(),
            n.as_ptr(),
            flags,
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return Err(format!("error {status}"));
    }
    let mut bytes = vec![0u8; size as usize];
    let status = unsafe {
        RegGetValueW(
            hkey,
            k.as_ptr(),
            n.as_ptr(),
            flags,
            &mut kind,
            bytes.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("error {status}"));
    }
    bytes.truncate(size as usize);
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(Some(match kind {
        REG_DWORD if bytes.len() == 4 => {
            RegData::Dword(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        REG_SZ => RegData::Sz(from_wide(&words)),
        REG_MULTI_SZ => RegData::MultiSz(
            String::from_utf16_lossy(&words)
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        kind => RegData::Raw { kind, bytes },
    }))
}

fn encode(data: &RegData) -> (u32, Vec<u8>) {
    let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    match data {
        RegData::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
        RegData::Sz(s) => (REG_SZ, utf16(&format!("{s}\0"))),
        RegData::MultiSz(list) => {
            let mut text: String = list.iter().map(|s| format!("{s}\0")).collect();
            text.push('\0');
            (REG_MULTI_SZ, utf16(&text))
        }
        RegData::Raw { kind, bytes } => (*kind, bytes.clone()),
    }
}

fn open(hkey: HKEY, key: &str) -> Option<HKEY> {
    let k = wide(key);
    let mut handle: HKEY = std::ptr::null_mut();
    (unsafe { RegOpenKeyExW(hkey, k.as_ptr(), 0, KEY_READ, &mut handle) } == ERROR_SUCCESS)
        .then_some(handle)
}

/// Names of the values of a key.
fn value_names(hkey: HKEY, key: &str) -> Vec<String> {
    let Some(handle) = open(hkey, key) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut buffer = vec![0u16; 32_768];
    for index in 0.. {
        let mut len = buffer.len() as u32;
        let status = unsafe {
            RegEnumValueW(
                handle,
                index,
                buffer.as_mut_ptr(),
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
        names.push(String::from_utf16_lossy(&buffer[..len as usize]));
    }
    unsafe { RegCloseKey(handle) };
    names
}

fn subkeys(hkey: HKEY, key: &str) -> Vec<String> {
    let Some(handle) = open(hkey, key) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut buffer = vec![0u16; 256];
    for index in 0.. {
        let mut len = buffer.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                handle,
                index,
                buffer.as_mut_ptr(),
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
        names.push(String::from_utf16_lossy(&buffer[..len as usize]));
    }
    unsafe { RegCloseKey(handle) };
    names
}

fn machine_dword(key: &str, name: &str) -> Option<u32> {
    match get(HKEY_LOCAL_MACHINE, key, name) {
        Ok(Some(RegData::Dword(v))) => Some(v),
        _ => None,
    }
}

fn machine_string(key: &str, name: &str) -> Option<String> {
    match get(HKEY_LOCAL_MACHINE, key, name) {
        Ok(Some(RegData::Sz(s))) => Some(s),
        _ => None,
    }
}

/// Runs one of the constant scripts above, hidden, with the folders (if
/// any) in `CANAPE_FOLDERS`, one per line.
fn powershell(script: &'static str, folders: &[String]) -> Result<(), String> {
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let exe = PathBuf::from(windows).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = Command::new(exe)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("CANAPE_FOLDERS", folders.join("\n"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("cannot start PowerShell: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    let first = message.lines().map(str::trim).find(|l| !l.is_empty());
    Err(format!(
        "Microsoft Defender refused: {}",
        first.unwrap_or("no message")
    ))
}

fn guid(text: &str) -> Result<GUID, String> {
    let (data1, data2, data3, data4) = parse_guid(text).ok_or("invalid GUID")?;
    Ok(GUID {
        data1,
        data2,
        data3,
        data4,
    })
}

fn guid_text(g: &GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7]
    )
}

fn win32(status: u32) -> Result<(), String> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("error {status}"))
    }
}

/// A service handle, closed on drop.
struct Service(SC_HANDLE);

impl Drop for Service {
    fn drop(&mut self) {
        unsafe { CloseServiceHandle(self.0) };
    }
}

/// Opens a service; `Ok(None)` when it is not installed.
fn open_service(name: &str, access: u32) -> Result<Option<Service>, String> {
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(format!("error {}", unsafe { GetLastError() }));
    }
    let manager = Service(manager);
    let name = wide(name);
    let handle = unsafe { OpenServiceW(manager.0, name.as_ptr(), access) };
    if handle.is_null() {
        return match unsafe { GetLastError() } {
            ERROR_SERVICE_DOES_NOT_EXIST => Ok(None),
            error => Err(format!("error {error}")),
        };
    }
    Ok(Some(Service(handle)))
}

fn service_error() -> String {
    format!("error {}", unsafe { GetLastError() })
}

fn service_running(service: &Service) -> Option<bool> {
    let mut status = SERVICE_STATUS::default();
    (unsafe { QueryServiceStatus(service.0, &mut status) } != 0)
        .then_some(status.dwCurrentState != SERVICE_STOPPED)
}

fn service_state(name: &str) -> Option<ServiceState> {
    let service = open_service(name, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)
        .ok()
        .flatten()?;
    let mut needed = 0u32;
    unsafe { QueryServiceConfigW(service.0, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return None;
    }
    // u64 words keep the structure aligned.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    let config = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    if unsafe { QueryServiceConfigW(service.0, config, needed, &mut needed) } == 0 {
        return None;
    }
    let start = unsafe { (*config).dwStartType };
    let mut delayed = SERVICE_DELAYED_AUTO_START_INFO {
        fDelayedAutostart: 0,
    };
    let ok = unsafe {
        QueryServiceConfig2W(
            service.0,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            (&mut delayed as *mut SERVICE_DELAYED_AUTO_START_INFO).cast(),
            std::mem::size_of::<SERVICE_DELAYED_AUTO_START_INFO>() as u32,
            &mut needed,
        )
    };
    Some(ServiceState {
        start,
        delayed: ok != 0 && delayed.fDelayedAutostart != 0,
        running: service_running(&service)?,
    })
}

impl System for WinSystem {
    fn read(&self, scope: Scope, key: &str, name: &str) -> Result<Option<RegData>, String> {
        let (hkey, key) = root(scope, key)?;
        get(hkey, &key, name)
    }

    fn write(
        &mut self,
        scope: Scope,
        key: &str,
        name: &str,
        data: Option<&RegData>,
    ) -> Result<(), String> {
        let (hkey, key) = root(scope, key)?;
        let (k, n) = (wide(&key), wide(name));
        let status = match data {
            Some(data) => {
                let (kind, bytes) = encode(data);
                unsafe {
                    RegSetKeyValueW(
                        hkey,
                        k.as_ptr(),
                        n.as_ptr(),
                        kind,
                        bytes.as_ptr().cast(),
                        bytes.len() as u32,
                    )
                }
            }
            None => match unsafe { RegDeleteKeyValueW(hkey, k.as_ptr(), n.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                status => status,
            },
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("error {status}"))
        }
    }

    fn tamper_protected(&self) -> Option<bool> {
        // 5 when on, 4 (or 0) when off.
        machine_dword(&format!(r"{DEFENDER}\Features"), "TamperProtection").map(|v| v & 1 == 1)
    }

    fn game_folders(&self) -> Vec<PathBuf> {
        use canape_library::{disk, epic, steam};
        let mut libraries = Vec::new();
        if let Some(path) = machine_string(r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath") {
            let steam_dir = PathBuf::from(path);
            let vdf = std::fs::read_to_string(steam_dir.join(r"steamapps\libraryfolders.vdf")).ok();
            libraries = steam::library_folders(&steam_dir, vdf.as_deref());
        }
        let mut installs: Vec<PathBuf> = epic::manifest_dir()
            .map(|dir| epic::installs(&dir).into_iter().map(|(_, p)| p).collect())
            .unwrap_or_default();
        let gog = r"SOFTWARE\WOW6432Node\GOG.com\Games";
        for id in subkeys(HKEY_LOCAL_MACHINE, gog) {
            if let Some(path) = machine_string(&format!(r"{gog}\{id}"), "path") {
                installs.push(PathBuf::from(path));
            }
        }
        super::find_game_folders(&libraries, &installs, &disk::fixed_drives())
    }

    fn defender_exclusions(&self) -> Vec<String> {
        value_names(HKEY_LOCAL_MACHINE, &format!(r"{DEFENDER}\Exclusions\Paths"))
    }

    fn add_defender_exclusions(&mut self, paths: &[String]) -> Result<(), String> {
        powershell(ADD_EXCLUSIONS, paths)
    }

    fn remove_defender_exclusions(&mut self, paths: &[String]) -> Result<(), String> {
        powershell(REMOVE_EXCLUSIONS, paths)
    }

    fn realtime_disabled(&self) -> bool {
        if self.tamper_protected() == Some(true) {
            return false;
        }
        let key = r"Real-Time Protection";
        machine_dword(&format!(r"{DEFENDER}\{key}"), "DisableRealtimeMonitoring") == Some(1)
            || machine_dword(
                &format!(r"{DEFENDER_POLICY}\{key}"),
                "DisableRealtimeMonitoring",
            ) == Some(1)
    }

    fn set_realtime_disabled(&mut self, disabled: bool) -> Result<(), String> {
        powershell(if disabled { REALTIME_OFF } else { REALTIME_ON }, &[])
    }

    fn power_scheme(&self) -> Result<String, String> {
        let mut scheme: *mut GUID = std::ptr::null_mut();
        win32(unsafe { PowerGetActiveScheme(std::ptr::null_mut(), &mut scheme) })?;
        if scheme.is_null() {
            return Err("no active power plan".into());
        }
        let text = guid_text(unsafe { &*scheme });
        unsafe { LocalFree(scheme.cast()) };
        Ok(text)
    }

    fn power_get(
        &self,
        scheme: &str,
        setting: PowerSetting,
        source: PowerSource,
    ) -> Result<u32, String> {
        let (scheme, subgroup, setting) = (
            guid(scheme)?,
            guid(PROCESSOR_SUBGROUP)?,
            guid(setting.guid())?,
        );
        let mut value = 0u32;
        let status = unsafe {
            match source {
                PowerSource::Ac => PowerReadACValueIndex(
                    std::ptr::null_mut(),
                    &scheme,
                    &subgroup,
                    &setting,
                    &mut value,
                ),
                PowerSource::Dc => PowerReadDCValueIndex(
                    std::ptr::null_mut(),
                    &scheme,
                    &subgroup,
                    &setting,
                    &mut value,
                ),
            }
        };
        win32(status).map(|()| value)
    }

    fn power_set(
        &mut self,
        scheme: &str,
        setting: PowerSetting,
        source: PowerSource,
        value: u32,
    ) -> Result<(), String> {
        let (scheme, subgroup, setting) = (
            guid(scheme)?,
            guid(PROCESSOR_SUBGROUP)?,
            guid(setting.guid())?,
        );
        let status = unsafe {
            match source {
                PowerSource::Ac => PowerWriteACValueIndex(
                    std::ptr::null_mut(),
                    &scheme,
                    &subgroup,
                    &setting,
                    value,
                ),
                PowerSource::Dc => PowerWriteDCValueIndex(
                    std::ptr::null_mut(),
                    &scheme,
                    &subgroup,
                    &setting,
                    value,
                ),
            }
        };
        win32(status)
    }

    fn power_commit(&mut self, scheme: &str) -> Result<(), String> {
        let scheme = guid(scheme)?;
        win32(unsafe { PowerSetActiveScheme(std::ptr::null_mut(), &scheme) })
    }

    fn service(&self, name: &str) -> Option<ServiceState> {
        service_state(name)
    }

    fn set_service_start(&mut self, name: &str, start: u32, delayed: bool) -> Result<(), String> {
        let service = open_service(name, SERVICE_CHANGE_CONFIG)?.ok_or("no such service")?;
        let changed = unsafe {
            ChangeServiceConfigW(
                service.0,
                SERVICE_NO_CHANGE,
                start,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if changed == 0 {
            return Err(service_error());
        }
        // Only an automatic service has a delayed flag.
        if start == SERVICE_AUTO_START {
            let info = SERVICE_DELAYED_AUTO_START_INFO {
                fDelayedAutostart: i32::from(delayed),
            };
            let ok = unsafe {
                ChangeServiceConfig2W(
                    service.0,
                    SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                    (&info as *const SERVICE_DELAYED_AUTO_START_INFO).cast(),
                )
            };
            if ok == 0 {
                return Err(service_error());
            }
        }
        Ok(())
    }

    fn stop_service(&mut self, name: &str) -> Result<(), String> {
        let service =
            open_service(name, SERVICE_STOP | SERVICE_QUERY_STATUS)?.ok_or("no such service")?;
        if service_running(&service) == Some(false) {
            return Ok(());
        }
        let mut status = SERVICE_STATUS::default();
        if unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) } == 0 {
            return Err(service_error());
        }
        // A service takes a moment to stop; give it a few seconds.
        for _ in 0..40 {
            if service_running(&service) == Some(false) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        Err("still running".into())
    }

    fn start_service(&mut self, name: &str) -> Result<(), String> {
        let service = open_service(name, SERVICE_START)?.ok_or("no such service")?;
        if unsafe { StartServiceW(service.0, 0, std::ptr::null()) } == 0 {
            return Err(service_error());
        }
        Ok(())
    }
}
