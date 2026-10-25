//! Queuing a download in the user's Steam client without any Steam window.
//!
//! Steam keeps one `appmanifest_<appid>.acf` per app in a library folder's
//! `steamapps`. A manifest that says "update required" for an app with no
//! files makes Steam download the app into `steamapps/common/<installdir>`
//! as an update, with no dialog. Steam reads the manifests when it starts:
//! when it is already running, it is restarted (`-shutdown`, then
//! `-silent`), never while a game runs.
//!
//! This is how the client behaves, not a documented interface: the
//! caller falls back to `steam://install/<appid>` (Steam's dialog) when
//! anything here fails or Steam does not take the manifest over in time.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::appinfo;
use crate::steam;

/// `StateFlags` of a queued download: update required (2) and update
/// started (1024).
pub const QUEUED: u32 = 1026;

/// The text of the manifest that queues `appid`, in the layout Steam
/// writes (tabs, one key per line).
pub fn manifest_text(appid: u32, name: &str, installdir: &str) -> String {
    let field = |key: &str, value: &str| format!("\t\"{key}\"\t\t\"{}\"\n", escape(value));
    let mut text = String::from("\"AppState\"\n{\n");
    text += &field("appid", &appid.to_string());
    text += &field("Universe", "1");
    text += &field("name", name);
    text += &field("StateFlags", &QUEUED.to_string());
    text += &field("installdir", installdir);
    for key in [
        "LastUpdated",
        "SizeOnDisk",
        "buildid",
        "BytesToDownload",
        "BytesDownloaded",
        "AutoUpdateBehavior",
        "AllowOtherDownloadsWhileRunning",
    ] {
        text += &field(key, "0");
    }
    text += "\t\"UserConfig\"\n\t{\n\t}\n\t\"MountedConfig\"\n\t{\n\t}\n}\n";
    text
}

/// A string as KeyValues quote it.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A folder name for a game Steam has no `installdir` for: its name
/// without the characters Windows refuses in a file name, nor trailing dots
/// and spaces; the app id when nothing is left.
pub fn installdir_from_name(name: &str, appid: u32) -> String {
    let kept: String = name
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
        .collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    let dir = words.join(" ");
    let dir = dir.trim_end_matches(['.', ' ']);
    // CON, NUL, COM1…: names Windows keeps for devices.
    let stem = dir.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if dir.is_empty() || reserved {
        appid.to_string()
    } else {
        dir.to_owned()
    }
}

/// The library folder to download into: the one with the most free space,
/// the first when no free space is known (Steam's own folder comes first).
pub fn choose_folder(folders: &[(PathBuf, Option<u64>)]) -> Option<PathBuf> {
    folders
        .iter()
        .enumerate()
        .max_by_key(|(i, (_, free))| (free.unwrap_or(0), std::cmp::Reverse(*i)))
        .map(|(_, (folder, _))| folder.clone())
}

/// A manifest Canapé wrote, until Steam takes it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub path: PathBuf,
    pub text: String,
    /// `steamapps/downloading/<appid>`, where Steam stages the files.
    pub staging: PathBuf,
}

impl Queued {
    /// Steam rewrote the manifest or started staging files.
    pub fn taken(&self) -> bool {
        self.staging.exists()
            || std::fs::read(&self.path).is_ok_and(|bytes| bytes != self.text.as_bytes())
    }

    /// Removes the manifest if Steam never touched it.
    pub fn withdraw(&self) {
        if std::fs::read(&self.path).is_ok_and(|bytes| bytes == self.text.as_bytes()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Writes the manifest that queues `appid` in the library folder with the
/// most free space. `fallback_name` is used when Steam's cache does not
/// have the app. Fails when any library folder already has a manifest for
/// it (a paused or broken install: Steam's dialog handles those).
pub fn write(steam: &Path, appid: u32, fallback_name: &str) -> io::Result<Queued> {
    let file = format!("appmanifest_{appid}.acf");
    let vdf_text = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")).ok();
    let folders: Vec<PathBuf> = steam::library_folders(steam, vdf_text.as_deref())
        .into_iter()
        .filter(|f| f.join("steamapps").is_dir())
        .collect();
    if folders
        .iter()
        .any(|f| f.join("steamapps").join(&file).exists())
    {
        return Err(io::Error::other("Steam already has a manifest for it"));
    }
    let spaces: Vec<(PathBuf, Option<u64>)> = folders
        .into_iter()
        .map(|f| {
            let free = crate::disk::free_space(&f);
            (f, free)
        })
        .collect();
    let folder =
        choose_folder(&spaces).ok_or_else(|| io::Error::other("no Steam library folder"))?;
    let cached = appinfo::app(steam, appid);
    let name = cached
        .as_ref()
        .and_then(|a| a.name.clone())
        .unwrap_or_else(|| fallback_name.trim().to_owned());
    if name.is_empty() {
        return Err(io::Error::other("no name for the game"));
    }
    let installdir = cached
        .and_then(|a| a.installdir)
        .unwrap_or_else(|| installdir_from_name(&name, appid));
    let steamapps = folder.join("steamapps");
    let queued = Queued {
        path: steamapps.join(&file),
        text: manifest_text(appid, &name, &installdir),
        staging: steamapps.join("downloading").join(appid.to_string()),
    };
    // create_new: never replaces a manifest written in the meantime.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&queued.path)?
        .write_all(queued.text.as_bytes())?;
    Ok(queued)
}

/// What the Steam client is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    NotRunning,
    /// Running, no game launched through it.
    Idle {
        pid: u32,
    },
    /// A game launched through Steam is running.
    Playing,
    /// Cannot tell (off Windows).
    Unknown,
}

/// Steam's state from the registry: `ActiveProcess\pid` (0 once Steam has
/// quit; checked against the live processes) and `RunningAppID` (the game
/// running, 0 for none).
#[cfg(windows)]
pub fn client() -> Client {
    use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
    let pid = crate::win::reg_dword(
        HKEY_CURRENT_USER,
        r"Software\Valve\Steam\ActiveProcess",
        "pid",
    )
    .unwrap_or(0);
    if pid == 0 || !crate::win::process_named(pid, "steam.exe") {
        return Client::NotRunning;
    }
    // Some games leave `RunningAppID` at 0: a program running from a
    // Steam library folder counts as a game too.
    if crate::win::any_process_under(r"\steamapps\common\") {
        return Client::Playing;
    }
    match crate::win::reg_dword(HKEY_CURRENT_USER, r"Software\Valve\Steam", "RunningAppID") {
        Some(0) => Client::Idle { pid },
        // A game, or a key that cannot be read: do not restart.
        _ => Client::Playing,
    }
}

#[cfg(not(windows))]
pub fn client() -> Client {
    Client::Unknown
}

/// Waits up to `timeout` for the process `pid` to end; true when it has.
#[cfg(windows)]
pub fn wait_exit(pid: u32, timeout: std::time::Duration) -> bool {
    crate::win::wait_process(pid, timeout)
}

#[cfg(not(windows))]
pub fn wait_exit(_pid: u32, _timeout: std::time::Duration) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_reads_back_as_a_queued_download() {
        let text = manifest_text(105600, "Terraria", "Terraria");
        assert_eq!(
            text,
            "\"AppState\"\n{\n\
             \t\"appid\"\t\t\"105600\"\n\
             \t\"Universe\"\t\t\"1\"\n\
             \t\"name\"\t\t\"Terraria\"\n\
             \t\"StateFlags\"\t\t\"1026\"\n\
             \t\"installdir\"\t\t\"Terraria\"\n\
             \t\"LastUpdated\"\t\t\"0\"\n\
             \t\"SizeOnDisk\"\t\t\"0\"\n\
             \t\"buildid\"\t\t\"0\"\n\
             \t\"BytesToDownload\"\t\t\"0\"\n\
             \t\"BytesDownloaded\"\t\t\"0\"\n\
             \t\"AutoUpdateBehavior\"\t\t\"0\"\n\
             \t\"AllowOtherDownloadsWhileRunning\"\t\t\"0\"\n\
             \t\"UserConfig\"\n\t{\n\t}\n\
             \t\"MountedConfig\"\n\t{\n\t}\n\
             }\n"
        );
        // Not installed yet, and nothing downloaded.
        assert_eq!(steam::parse_manifest(&text), None);
        assert_eq!(
            steam::parse_download(&text),
            Some(steam::Download::Progress(0.0))
        );
        let odd = manifest_text(1, r#"Say "hi" \o/"#, "x");
        let root = crate::vdf::parse(&odd);
        assert_eq!(
            root.get("AppState").unwrap().str("name"),
            Some(r#"Say "hi" \o/"#)
        );
    }

    #[test]
    fn installdir_drops_what_windows_refuses() {
        assert_eq!(installdir_from_name("Terraria", 105600), "Terraria");
        assert_eq!(
            installdir_from_name("Half-Life 2: Episode One", 380),
            "Half-Life 2 Episode One"
        );
        assert_eq!(installdir_from_name("What?  Now*  ", 1), "What Now");
        assert_eq!(installdir_from_name("Dots...", 1), "Dots");
        assert_eq!(installdir_from_name("???", 42), "42");
        assert_eq!(installdir_from_name("CON", 7), "7");
        assert_eq!(installdir_from_name("Console", 7), "Console");
        assert_eq!(
            installdir_from_name("NieR:Automata™", 524220),
            "NieRAutomata™"
        );
    }

    #[test]
    fn folder_with_most_space_wins() {
        let (c, d, e) = (
            PathBuf::from(r"C:\Steam"),
            PathBuf::from(r"D:\SteamLibrary"),
            PathBuf::from(r"E:\SteamLibrary"),
        );
        assert_eq!(
            choose_folder(&[
                (c.clone(), Some(10)),
                (d.clone(), Some(50)),
                (e.clone(), None)
            ]),
            Some(d.clone())
        );
        // Ties and unknown space: the first, Steam's own folder.
        assert_eq!(
            choose_folder(&[(c.clone(), None), (d.clone(), None)]),
            Some(c.clone())
        );
        assert_eq!(
            choose_folder(&[(c.clone(), Some(5)), (d, Some(5))]),
            Some(c)
        );
        assert_eq!(choose_folder(&[]), None);
    }

    #[test]
    fn writes_once_and_withdraws_only_its_own_manifest() {
        let steam = std::env::temp_dir().join(format!("canape-queue-{}", std::process::id()));
        let steamapps = steam.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        let queued = write(&steam, 105600, "Terraria").unwrap();
        assert_eq!(queued.path, steamapps.join("appmanifest_105600.acf"));
        assert_eq!(
            std::fs::read_to_string(&queued.path).unwrap(),
            manifest_text(105600, "Terraria", "Terraria")
        );
        assert!(!queued.taken());
        // A manifest is there: never another one.
        assert!(write(&steam, 105600, "Terraria").is_err());
        // Steam rewrote it: it is Steam's now.
        std::fs::write(&queued.path, "\"AppState\" { \"StateFlags\" \"1026\" }").unwrap();
        assert!(queued.taken());
        queued.withdraw();
        assert!(queued.path.exists());
        std::fs::write(&queued.path, &queued.text).unwrap();
        queued.withdraw();
        assert!(!queued.path.exists());
        // Staged files count as taken.
        std::fs::create_dir_all(&queued.staging).unwrap();
        assert!(queued.taken());
        assert!(write(&steam, 620, " ").is_err());
        std::fs::remove_dir_all(&steam).unwrap();
    }
}
