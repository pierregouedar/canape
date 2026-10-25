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

use super::{RegData, Scope, System};

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
}
