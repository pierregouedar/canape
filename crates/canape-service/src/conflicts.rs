//! Other programs that write the same firmware settings as Canape.

/// Process name (lowercase) → name shown to the user.
const KNOWN: &[(&str, &str)] = &[
    ("whservice.exe", "Winhanced"),
    ("armourycrate.service.exe", "Armoury Crate"),
    ("ghelper.exe", "G-Helper"),
    ("handheldcompanion.exe", "Handheld Companion"),
];

/// Display names of the conflicting programs among `processes`, deduplicated.
pub fn find<'a>(processes: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut found: Vec<String> = processes
        .into_iter()
        .filter_map(|process| {
            let process = process.to_ascii_lowercase();
            KNOWN
                .iter()
                .find(|(exe, _)| *exe == process)
                .map(|(_, name)| name.to_string())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

#[cfg(windows)]
pub fn detect() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut names = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
        ok = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    find(names.iter().map(String::as_str))
}

/// No conflicting programs exist off Windows.
#[cfg(not(windows))]
pub fn detect() -> Vec<String> {
    find(std::iter::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_programs_once() {
        let found = find([
            "explorer.exe",
            "WHService.exe",
            "ghelper.exe",
            "whservice.exe",
        ]);
        assert_eq!(found, vec!["G-Helper", "Winhanced"]);
    }
}
