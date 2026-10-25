//! The shortcuts of the Start menu and the desktop, offered as games to add
//! to `games.json` ([`crate::external`]): a list to pick from with the
//! controller, instead of a file picker.
//!
//! A `.lnk` file is read directly (the Shell Link binary format): its
//! target, arguments and working folder. Shortcuts that do not start a
//! program, or start one that is plainly not a game (uninstallers, setup
//! and update tools, Windows' own programs, the stores' clients), are left
//! out.

use std::path::{Path, PathBuf};

use crate::external::Entry;

/// What a `.lnk` file starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    /// Target path; may hold `%VARIABLES%` (see [`expand`]).
    pub target: String,
    pub args: String,
    pub dir: String,
}

/// A shortcut that could be a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// The shortcut's file name, without `.lnk`.
    pub name: String,
    pub link: Link,
}

impl Shortcut {
    /// The `games.json` entry that launches what the shortcut does.
    pub fn entry(&self) -> Entry {
        Entry {
            title: self.name.clone(),
            exe: self.link.target.clone(),
            args: self.link.args.clone(),
            dir: self.link.dir.clone(),
            cover: String::new(),
        }
    }
}

const HEADER_SIZE: u32 = 0x4c;
const LINK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46,
];
const HAS_ID_LIST: u32 = 1 << 0;
const HAS_LINK_INFO: u32 = 1 << 1;
const HAS_NAME: u32 = 1 << 2;
const HAS_RELATIVE_PATH: u32 = 1 << 3;
const HAS_WORKING_DIR: u32 = 1 << 4;
const HAS_ARGUMENTS: u32 = 1 << 5;
const HAS_ICON_LOCATION: u32 = 1 << 6;
const IS_UNICODE: u32 = 1 << 7;
/// The link info holds a volume and a local base path.
const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 1 << 0;
/// Extra data block: the target with environment variables.
const ENVIRONMENT_BLOCK: u32 = 0xa000_0001;
const ENVIRONMENT_BLOCK_SIZE: usize = 0x314;

fn u16_at(data: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Bytes in the system code page: ASCII is exact, the rest read as Latin-1.
fn ansi(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// A NUL-terminated string at `at`.
fn c_string(data: &[u8], at: usize, wide: bool) -> Option<String> {
    let rest = data.get(at..)?;
    if wide {
        let end = rest.chunks_exact(2).position(|c| c == [0, 0])? * 2;
        Some(utf16(&rest[..end]))
    } else {
        let end = rest.iter().position(|&b| b == 0)?;
        Some(ansi(&rest[..end]))
    }
}

/// The local path of a LinkInfo structure.
fn link_info_path(info: &[u8]) -> Option<String> {
    let header = u32_at(info, 4)? as usize;
    if u32_at(info, 8)? & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None;
    }
    let wide_offsets = (header >= 0x24)
        .then(|| Some((u32_at(info, 0x1c)? as usize, u32_at(info, 0x20)? as usize)))
        .flatten()
        .filter(|(base, _)| *base != 0);
    let (base, suffix) = match wide_offsets {
        Some((base, suffix)) => (
            c_string(info, base, true)?,
            c_string(info, suffix, true).unwrap_or_default(),
        ),
        None => (
            c_string(info, u32_at(info, 0x10)? as usize, false)?,
            c_string(info, u32_at(info, 0x18)? as usize, false).unwrap_or_default(),
        ),
    };
    let path = match (base.ends_with('\\'), suffix.is_empty()) {
        (_, true) | (true, false) => base + &suffix,
        (false, false) => format!("{base}\\{suffix}"),
    };
    (!path.is_empty()).then_some(path)
}

/// Reads a `.lnk` file. None if it is not one, or names no file path (a
/// shortcut to a Windows app or an installer-managed one).
pub fn parse(data: &[u8]) -> Option<Link> {
    if u32_at(data, 0)? != HEADER_SIZE || data.get(4..20)? != LINK_CLSID {
        return None;
    }
    let flags = u32_at(data, 0x14)?;
    let wide = flags & IS_UNICODE != 0;
    let mut at = HEADER_SIZE as usize;
    if flags & HAS_ID_LIST != 0 {
        at += 2 + u16_at(data, at)?;
    }
    let mut target = None;
    if flags & HAS_LINK_INFO != 0 {
        let size = u32_at(data, at)? as usize;
        target = link_info_path(data.get(at..at + size)?);
        at += size;
    }
    // Name, relative path, working folder, arguments, icon, in this order.
    let mut strings: [String; 5] = Default::default();
    let bits = [
        HAS_NAME,
        HAS_RELATIVE_PATH,
        HAS_WORKING_DIR,
        HAS_ARGUMENTS,
        HAS_ICON_LOCATION,
    ];
    for (slot, bit) in strings.iter_mut().zip(bits) {
        if flags & bit == 0 {
            continue;
        }
        let count = u16_at(data, at)?;
        let len = if wide { count * 2 } else { count };
        let raw = data.get(at + 2..at + 2 + len)?;
        *slot = if wide { utf16(raw) } else { ansi(raw) };
        at += 2 + len;
    }
    // The extra data: the target with its variables, when the shortcut
    // was made with one (`%ProgramFiles%\…`).
    while let Some(size) = u32_at(data, at).map(|s| s as usize) {
        if size < 8 {
            break;
        }
        let Some(block) = data.get(at..at + size) else {
            break;
        };
        if u32_at(block, 4) == Some(ENVIRONMENT_BLOCK) && size >= ENVIRONMENT_BLOCK_SIZE {
            let unicode = c_string(&block[8 + 260..8 + 260 + 520], 0, true).unwrap_or_default();
            let found = if unicode.is_empty() {
                c_string(&block[8..8 + 260], 0, false).unwrap_or_default()
            } else {
                unicode
            };
            if target.is_none() && !found.is_empty() {
                target = Some(found);
            }
        }
        at += size;
    }
    let [_, _, dir, args, _] = strings;
    Some(Link {
        target: target?,
        args: args.trim().to_owned(),
        dir,
    })
}

/// Replaces each `%NAME%` that `lookup` knows; the others stay.
pub fn expand(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => match lookup(&after[..end]) {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Words of programs that come with a game but are not one.
const NOT_GAMES: &[&str] = &[
    "uninst",
    "unins0",
    "setup",
    "install",
    "update",
    "readme",
    "manual",
    "website",
    "support",
    "crashreport",
    "crashhandler",
    "redist",
    "dxsetup",
    "vcredist",
];

/// Folders of programs that are never games: office, browsers, developer
/// tools, Windows' own apps and shared components.
const NOT_GAME_FOLDERS: &[&str] = &[
    "\\microsoft office\\",
    "\\microsoft\\edge",
    "\\microsoft\\office",
    "\\microsoft visual studio",
    "\\microsoft vs code\\",
    "\\windows kits\\",
    "\\windows media player\\",
    "\\windowsapps\\",
    "\\common files\\",
    "\\git\\",
    "\\nodejs\\",
    "\\python",
    "\\java\\",
];

/// Programs that are never games, by file name.
const NOT_GAME_FILES: &[&str] = &[
    "excel.exe",
    "winword.exe",
    "powerpnt.exe",
    "msaccess.exe",
    "mspub.exe",
    "onenote.exe",
    "outlook.exe",
    "setlang.exe",
    "msedge.exe",
    "code.exe",
    "notepad++.exe",
    "stikynot.exe",
    "wmplayer.exe",
    "appcertui.exe",
    "rtss.exe",
];

/// Whether the shortcut `name` to `target` could be a game: a program
/// (`.exe`, or a `.bat` some games start with), outside `windows_dir`, that
/// is not an uninstaller, a setup or an update tool, an office, browser or
/// developer tool, a store's client or Canapé itself.
pub fn could_be_game(name: &str, target: &str, windows_dir: &str) -> bool {
    let lower = target.to_lowercase();
    let file = lower.rsplit(['\\', '/']).next().unwrap_or(&lower);
    if !(file.ends_with(".exe") || file.ends_with(".bat"))
        || crate::is_store_program(file)
        || file.starts_with("canape")
        || NOT_GAME_FILES.contains(&file)
        || NOT_GAME_FOLDERS.iter().any(|folder| lower.contains(folder))
    {
        return false;
    }
    let windows = windows_dir.trim_end_matches('\\').to_lowercase();
    if !windows.is_empty() && lower.starts_with(&format!("{windows}\\")) {
        return false;
    }
    let name = name.to_lowercase();
    !NOT_GAMES
        .iter()
        .any(|word| file.contains(word) || name.contains(word))
}

/// Every `.lnk` under `dirs` (subfolders included) that could be a game and
/// whose target `exists`, once per target and arguments, by name.
pub fn find(dirs: &[PathBuf], windows_dir: &str, exists: impl Fn(&str) -> bool) -> Vec<Shortcut> {
    let mut files = Vec::new();
    for dir in dirs {
        collect(dir, 0, &mut files);
    }
    let mut found: Vec<Shortcut> = Vec::new();
    for path in files {
        let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let Some(mut link) = std::fs::read(&path).ok().and_then(|data| parse(&data)) else {
            continue;
        };
        let env = |var: &str| std::env::var(var).ok();
        link.target = expand(&link.target, env);
        link.dir = expand(&link.dir, env);
        if !could_be_game(&name, &link.target, windows_dir) || !exists(&link.target) {
            continue;
        }
        let same = |s: &Shortcut| {
            s.link.target.eq_ignore_ascii_case(&link.target) && s.link.args == link.args
        };
        if !found.iter().any(same) {
            found.push(Shortcut { name, link });
        }
    }
    found.sort_by_key(|s| s.name.to_lowercase());
    found
}

/// Files and folders read at most, and folder depth.
const MAX_FILES: usize = 4000;
const MAX_DEPTH: usize = 4;

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && depth < MAX_DEPTH {
            collect(&path, depth + 1, out);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            out.push(path);
        }
    }
}

/// The Start menu's program folders (the user's and everyone's) and both
/// desktops. Empty off Windows.
pub fn folders() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop,
            SHGetKnownFolderPath,
        };
        let mut out = Vec::new();
        for id in [
            FOLDERID_Programs,
            FOLDERID_CommonPrograms,
            FOLDERID_Desktop,
            FOLDERID_PublicDesktop,
        ] {
            let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
            let hr = unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut raw) };
            if hr >= 0 && !raw.is_null() {
                let len = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
                let wide = unsafe { std::slice::from_raw_parts(raw, len) };
                out.push(PathBuf::from(String::from_utf16_lossy(wide)));
            }
            if !raw.is_null() {
                unsafe { CoTaskMemFree(raw as *const _) };
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Windows' own folder, whose programs are never games.
pub fn windows_dir() -> String {
    std::env::var("SystemRoot")
        .or_else(|_| std::env::var("windir"))
        .unwrap_or_else(|_| r"C:\Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A `.lnk` file as Windows writes one: an empty ID list, a link info
    /// with an ANSI and a Unicode path, then Unicode strings.
    pub fn lnk(target: &str, dir: &str, args: &str) -> Vec<u8> {
        let mut flags = HAS_ID_LIST | HAS_LINK_INFO | IS_UNICODE | HAS_NAME;
        if !dir.is_empty() {
            flags |= HAS_WORKING_DIR;
        }
        if !args.is_empty() {
            flags |= HAS_ARGUMENTS;
        }
        let mut data = Vec::new();
        data.extend(HEADER_SIZE.to_le_bytes());
        data.extend(LINK_CLSID);
        data.extend(flags.to_le_bytes());
        data.resize(HEADER_SIZE as usize, 0);
        // An ID list of one item (skipped), then its terminator.
        data.extend(6u16.to_le_bytes());
        data.extend([4, 0, 0xaa, 0xbb, 0, 0]);
        // Link info: header of 0x24 bytes, a volume id, the paths.
        let volume = [0x10u8, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0x10, 0, 0, 0];
        let ansi_path: Vec<u8> = target
            .bytes()
            .map(|b| if b < 0x80 { b } else { b'?' })
            .collect();
        let volume_at = 0x24u32;
        let base_at = volume_at + volume.len() as u32;
        let suffix_at = base_at + ansi_path.len() as u32 + 1;
        let wide_base_at = suffix_at + 1;
        let wide_suffix_at = wide_base_at + (target.encode_utf16().count() as u32 + 1) * 2;
        let size = wide_suffix_at + 2;
        let mut info = Vec::new();
        for v in [
            size,
            0x24,
            VOLUME_ID_AND_LOCAL_BASE_PATH,
            volume_at,
            base_at,
            0,
            suffix_at,
            wide_base_at,
            wide_suffix_at,
        ] {
            info.extend(v.to_le_bytes());
        }
        info.extend(volume);
        info.extend(&ansi_path);
        info.extend([0, 0]);
        info.extend(wide(target));
        info.extend([0, 0, 0, 0]);
        data.extend(info);
        for s in ["A comment", dir, args] {
            if s.is_empty() {
                continue;
            }
            data.extend((s.encode_utf16().count() as u16).to_le_bytes());
            data.extend(wide(s));
        }
        data.extend(0u32.to_le_bytes());
        data
    }

    #[test]
    fn links_are_read() {
        let data = lnk(
            r"D:\Jeux\Célèste\Celeste.exe",
            r"D:\Jeux\Célèste",
            "--fullscreen ",
        );
        assert_eq!(
            parse(&data),
            Some(Link {
                target: r"D:\Jeux\Célèste\Celeste.exe".into(),
                args: "--fullscreen".into(),
                dir: r"D:\Jeux\Célèste".into(),
            })
        );
        // Truncated or foreign files are refused, never a panic.
        for len in 0..data.len() {
            let _ = parse(&data[..len]);
        }
        assert_eq!(
            parse(b"[InternetShortcut]\r\nURL=steam://rungameid/400"),
            None
        );
    }

    #[test]
    fn ansi_paths_and_environment_targets() {
        // No Unicode offsets in the link info, ANSI strings; no link info
        // at all but an environment block.
        let mut data = Vec::new();
        data.extend(HEADER_SIZE.to_le_bytes());
        data.extend(LINK_CLSID);
        data.extend(HAS_ARGUMENTS.to_le_bytes());
        data.resize(HEADER_SIZE as usize, 0);
        data.extend(2u16.to_le_bytes());
        data.extend(b"-x");
        let mut block = vec![0u8; ENVIRONMENT_BLOCK_SIZE];
        block[..4].copy_from_slice(&(ENVIRONMENT_BLOCK_SIZE as u32).to_le_bytes());
        block[4..8].copy_from_slice(&ENVIRONMENT_BLOCK.to_le_bytes());
        let target = r"%ProgramFiles%\Game\game.exe";
        block[8..8 + target.len()].copy_from_slice(target.as_bytes());
        data.extend(block);
        data.extend(0u32.to_le_bytes());
        let link = parse(&data).unwrap();
        assert_eq!(link.target, target);
        assert_eq!(link.args, "-x");
        let expanded = expand(&link.target, |name| {
            (name == "ProgramFiles").then(|| r"C:\Program Files".to_owned())
        });
        assert_eq!(expanded, r"C:\Program Files\Game\game.exe");
        assert_eq!(expand("100% %UNKNOWN% %", |_| None), "100% %UNKNOWN% %");
    }

    #[test]
    fn only_games_are_offered() {
        let win = r"C:\Windows";
        assert!(could_be_game(
            "Celeste",
            r"D:\Games\Celeste\Celeste.exe",
            win
        ));
        assert!(!could_be_game(
            "Notepad",
            r"C:\WINDOWS\system32\notepad.exe",
            win
        ));
        assert!(!could_be_game(
            "Uninstall Celeste",
            r"D:\Games\Celeste\unins000.exe",
            win
        ));
        assert!(!could_be_game(
            "Celeste Manual",
            r"D:\Games\Celeste\manual.pdf",
            win
        ));
        assert!(!could_be_game(
            "Steam",
            r"C:\Program Files (x86)\Steam\steam.exe",
            win
        ));
        assert!(!could_be_game("Canapé", r"C:\Canape\canape-shell.exe", win));
        assert!(!could_be_game("Game updater", r"D:\Game\Updater.exe", win));
        assert!(!could_be_game(
            "Excel",
            r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE",
            win
        ));
        assert!(!could_be_game(
            "Git Bash",
            r"C:\Program Files\Git\git-bash.exe",
            win
        ));
        assert!(!could_be_game(
            "Microsoft Edge",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            win
        ));
        assert!(could_be_game(
            "Heaven Benchmark 4.0",
            r"C:\Program Files (x86)\Unigine\Heaven Benchmark 4.0\heaven.bat",
            win
        ));
    }

    #[test]
    fn shortcuts_are_found_once_by_name() {
        let dir = std::env::temp_dir().join(format!("canape-shortcuts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (menu, desktop) = (dir.join("menu").join("Celeste"), dir.join("desktop"));
        std::fs::create_dir_all(&menu).unwrap();
        std::fs::create_dir_all(&desktop).unwrap();
        let celeste = lnk(r"D:\Celeste\Celeste.exe", r"D:\Celeste", "");
        std::fs::write(menu.join("Celeste.lnk"), &celeste).unwrap();
        std::fs::write(desktop.join("Celeste.lnk"), &celeste).unwrap();
        std::fs::write(
            menu.join("Uninstall Celeste.lnk"),
            lnk(r"D:\Celeste\unins000.exe", "", ""),
        )
        .unwrap();
        std::fs::write(
            desktop.join("anodyne.LNK"),
            lnk(r"D:\Anodyne\Anodyne.exe", "", ""),
        )
        .unwrap();
        std::fs::write(desktop.join("Gone.lnk"), lnk(r"D:\Gone\gone.exe", "", "")).unwrap();
        std::fs::write(desktop.join("notes.txt"), b"hello").unwrap();
        let found = find(&[dir.join("menu"), desktop], r"C:\Windows", |t| {
            !t.contains("Gone")
        });
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["anodyne", "Celeste"]);
        let entry = found[1].entry();
        assert_eq!(
            (entry.title.as_str(), entry.exe.as_str(), entry.dir.as_str()),
            ("Celeste", r"D:\Celeste\Celeste.exe", r"D:\Celeste")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
