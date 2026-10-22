//! Small Windows helpers: registry reads, drive letters, time zone.

use std::collections::HashMap;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
    RegEnumValueW, RegGetValueW, RegOpenKeyExW,
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
