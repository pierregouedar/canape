//! Where Legendary and gogdl install games, and whether a game fits: the
//! free space of a folder's drive, the fixed drives, and the default games
//! folder (the folder most games installed by Canapé or the Epic launcher
//! already sit in, else `Games` on the drive with the most free space).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Name of the games folder made on a drive.
pub const GAMES: &str = "Games";

/// Bytes free for the user on the drive of `path` (or of its nearest
/// existing parent). `None` off Windows or when Windows cannot tell.
#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut dir = path;
    while !dir.exists() {
        dir = dir.parent()?;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
pub fn free_space(_path: &Path) -> Option<u64> {
    None
}

/// The roots of the fixed drives (`C:\`, `D:\`…): no removable, network or
/// optical drive.
#[cfg(windows)]
pub fn fixed_drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    /// `DRIVE_FIXED`.
    const FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            unsafe { GetDriveTypeW(wide.as_ptr()) == FIXED }
        })
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
pub fn fixed_drives() -> Vec<PathBuf> {
    Vec::new()
}

/// The folders holding the game folders `installs`, the one holding most
/// first (ties: the order seen).
pub fn parents(installs: &[PathBuf]) -> Vec<PathBuf> {
    let mut counts: Vec<(PathBuf, usize)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for parent in installs.iter().filter_map(|p| p.parent()) {
        if parent.as_os_str().is_empty() {
            continue;
        }
        let key = parent.to_string_lossy().to_lowercase();
        match index.get(&key) {
            Some(&i) => counts[i].1 += 1,
            None => {
                index.insert(key, counts.len());
                counts.push((parent.to_path_buf(), 1));
            }
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts.into_iter().map(|(p, _)| p).collect()
}

/// The games folder when none is set: the folder that already holds most
/// games (`existing`, see [`parents`]), else `Games` on the drive with the
/// most free space (`drives`: root and bytes free).
pub fn default_games_dir(existing: &[PathBuf], drives: &[(PathBuf, u64)]) -> Option<PathBuf> {
    existing.first().cloned().or_else(|| {
        drives
            .iter()
            .max_by_key(|(_, free)| *free)
            .map(|(root, _)| root.join(GAMES))
    })
}

/// The folders Settings offers, in order: the folders already holding
/// games, then `Games` on each fixed drive, no folder twice; `current`
/// first if it is none of them.
pub fn choices(current: &Path, existing: &[PathBuf], drives: &[PathBuf]) -> Vec<PathBuf> {
    let key = |p: &Path| p.to_string_lossy().to_lowercase();
    let mut out: Vec<PathBuf> = Vec::new();
    let candidates = existing
        .iter()
        .cloned()
        .chain(drives.iter().map(|d| d.join(GAMES)));
    for path in candidates {
        if !path.as_os_str().is_empty() && !out.iter().any(|p| key(p) == key(&path)) {
            out.push(path);
        }
    }
    if !out.iter().any(|p| key(p) == key(current)) {
        out.insert(0, current.to_path_buf());
    }
    out
}

/// The folder `step` places after (or before, when negative) `current`
/// among [`choices`], round the list; `None` when there is no other.
pub fn next_choice(
    current: &Path,
    existing: &[PathBuf],
    drives: &[PathBuf],
    step: i32,
) -> Option<PathBuf> {
    let list = choices(current, existing, drives);
    let key = current.to_string_lossy().to_lowercase();
    let at = list
        .iter()
        .position(|p| p.to_string_lossy().to_lowercase() == key)?;
    (list.len() > 1).then(|| {
        let next = (at as i64 + step as i64).rem_euclid(list.len() as i64) as usize;
        list[next].clone()
    })
}

/// Why `need` bytes do not fit in `dir` with `free` bytes free, or `None`
/// when they do (or the free space is unknown).
pub fn shortage(need: u64, free: Option<u64>, dir: &Path) -> Option<String> {
    let free = free?;
    (need > free).then(|| {
        format!(
            "not enough space in {}: {:.1} GB needed, {:.1} GB free",
            dir.display(),
            need as f64 / 1e9,
            free as f64 / 1e9
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn the_folder_holding_most_games_comes_first() {
        let installs = [
            p("C:/Program Files/Epic Games/Fortnite"),
            p("D:/Games/A"),
            p("d:/games/B"),
            p("C:/Program Files/Epic Games/Hades"),
            p("D:/Games/C"),
        ];
        assert_eq!(
            parents(&installs),
            [p("D:/Games"), p("C:/Program Files/Epic Games")]
        );
        assert!(parents(&[p("A")]).is_empty());
    }

    #[test]
    fn the_default_is_the_existing_folder_else_the_roomiest_drive() {
        let drives = [(p("C:/"), 20), (p("D:/"), 300), (p("E:/"), 100)];
        assert_eq!(
            default_games_dir(&[p("E:/Epic")], &drives),
            Some(p("E:/Epic"))
        );
        assert_eq!(default_games_dir(&[], &drives), Some(p("D:/Games")));
        assert_eq!(default_games_dir(&[], &[]), None);
    }

    #[test]
    fn choices_go_round_in_a_stable_order() {
        let existing = [p("C:/Epic"), p("d:/games")];
        let drives = [p("C:/"), p("D:/")];
        assert_eq!(
            choices(&p("D:/Games"), &existing, &drives),
            [p("C:/Epic"), p("d:/games"), p("C:/").join(GAMES)]
        );
        // Each step moves on, whatever folder was set last.
        let mut at = p("C:/Epic");
        let mut seen = Vec::new();
        for _ in 0..3 {
            at = next_choice(&at, &existing, &drives, 1).unwrap();
            seen.push(at.clone());
        }
        assert_eq!(seen, [p("d:/games"), p("C:/").join(GAMES), p("C:/Epic")]);
        assert_eq!(
            next_choice(&p("C:/Epic"), &existing, &drives, -1),
            Some(p("C:/").join(GAMES))
        );
        // A folder set by hand comes first, until another is picked.
        assert_eq!(
            next_choice(&p("F:/Mine"), &existing, &drives, 1),
            Some(p("C:/Epic"))
        );
        assert_eq!(next_choice(&p("C:/Epic"), &[], &[], 1), None);
    }

    #[test]
    fn shortage_says_what_is_missing() {
        let dir = p("D:/Games");
        assert_eq!(shortage(10, Some(10), &dir), None);
        assert_eq!(shortage(10, None, &dir), None);
        assert_eq!(
            shortage(52_300_000_000, Some(48_240_000_000), &dir).as_deref(),
            Some("not enough space in D:/Games: 52.3 GB needed, 48.2 GB free")
        );
    }
}
