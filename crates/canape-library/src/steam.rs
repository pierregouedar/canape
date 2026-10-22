//! Steam: library folders from `steamapps/libraryfolders.vdf`, one
//! `appmanifest_<appid>.acf` per installed app, last played times from the
//! manifests and each user's `localconfig.vdf`, covers from Steam's own
//! cache. Launched with `steam://rungameid/<appid>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::vdf::{self, Value};
use crate::{Game, Launch, Source};

/// Steam's install folder: `HKCU\Software\Valve\Steam\SteamPath`, else the
/// default location.
#[cfg(windows)]
pub fn install_dir() -> PathBuf {
    crate::win::reg_string(
        windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
        r"Software\Valve\Steam",
        "SteamPath",
    )
    // Written with forward slashes: `c:/program files (x86)/steam`.
    .map(|path| PathBuf::from(path.replace('/', "\\")))
    .filter(|path| path.is_dir())
    .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam"))
}

/// Apps that Steam installs next to games but that are not games.
const NOT_GAMES: &[u32] = &[
    228980,  // Steamworks Common Redistributables
    250820,  // SteamVR
    1007,    // Steam client
    1070560, // Steam Linux Runtime 1.0 (scout)
    1391110, // Steam Linux Runtime 2.0 (soldier)
    1628350, // Steam Linux Runtime 3.0 (sniper)
    1493710, // Proton Experimental
    1826330, // Proton EasyAntiCheat Runtime
    1161040, // Proton BattlEye Runtime
    2180100, // Proton Hotfix
];

fn is_tool(appid: u32, name: &str) -> bool {
    NOT_GAMES.contains(&appid)
        || name.starts_with("Proton ")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Steamworks ")
        || name == "SteamVR"
}

/// Library folders listed in `libraryfolders.vdf`, `steam` itself first.
pub fn library_folders(steam: &Path, vdf_text: Option<&str>) -> Vec<PathBuf> {
    let mut folders = vec![steam.to_path_buf()];
    let Some(text) = vdf_text else {
        return folders;
    };
    let root = vdf::parse(text);
    // "libraryfolders" (current) or "LibraryFolders" (before 2021).
    let Some(list) = root.get("libraryfolders") else {
        return folders;
    };
    for (key, value) in list.entries() {
        if !key.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // Current: "0" { "path" "…" }; before 2021: "1" "D:\\SteamLibrary".
        let path = match value {
            Value::Str(path) => Some(path.as_str()),
            Value::Obj(_) => value.str("path"),
        };
        if let Some(path) = path.filter(|p| !p.is_empty()) {
            let path = PathBuf::from(path);
            if !folders.contains(&path) {
                folders.push(path);
            }
        }
    }
    folders
}

/// What an `appmanifest_<appid>.acf` says about an installed game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub appid: u32,
    pub name: String,
    pub installdir: String,
    pub last_played: Option<u64>,
}

/// A manifest, if it describes a fully installed game.
pub fn parse_manifest(text: &str) -> Option<Manifest> {
    let root = vdf::parse(text);
    let app = root.get("AppState")?;
    let appid: u32 = app.str("appid")?.trim().parse().ok()?;
    let name = app.str("name").unwrap_or_default().trim().to_owned();
    let flags: u32 = app.str("StateFlags")?.trim().parse().ok()?;
    // Bit 2: fully installed (an update in progress keeps it set).
    if flags & 4 == 0 || name.is_empty() || is_tool(appid, &name) {
        return None;
    }
    Some(Manifest {
        appid,
        name,
        installdir: app.str("installdir").unwrap_or_default().to_owned(),
        last_played: last_played(app),
    })
}

fn last_played(app: &Value) -> Option<u64> {
    app.str("LastPlayed")
        .and_then(|t| t.trim().parse().ok())
        .filter(|&t| t > 0)
}

/// Last played times (appid → unix seconds) from a user's
/// `userdata/<id>/config/localconfig.vdf`.
pub fn parse_localconfig(text: &str) -> HashMap<u32, u64> {
    let root = vdf::parse(text);
    let apps = root.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]);
    apps.map(|apps| {
        apps.entries()
            .iter()
            .filter_map(|(id, app)| Some((id.parse().ok()?, last_played(app)?)))
            .collect()
    })
    .unwrap_or_default()
}

fn read(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn dirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Best cover in Steam's cache: custom grid art, then the 600x900 portrait,
/// then the hero and header images (landscape, framed by the shell).
pub fn cover(steam: &Path, appid: u32) -> Option<PathBuf> {
    let cache = steam.join("appcache").join("librarycache");
    let mut candidates = Vec::new();
    // Art the user picked in Steam: userdata/<user>/config/grid/<appid>p.*
    for user in dirs(&steam.join("userdata")) {
        let grid = user.join("config").join("grid");
        for ext in ["png", "jpg", "jpeg"] {
            candidates.push(grid.join(format!("{appid}p.{ext}")));
        }
    }
    // Older cache: librarycache/<appid>_library_600x900.jpg; current cache:
    // librarycache/<appid>/library_600x900.jpg, sometimes one folder deeper.
    let per_app = cache.join(appid.to_string());
    let deeper = dirs(&per_app);
    for name in ["library_600x900.jpg", "library_hero.jpg", "header.jpg"] {
        candidates.push(cache.join(format!("{appid}_{name}")));
        candidates.push(per_app.join(name));
        candidates.extend(deeper.iter().map(|d| d.join(name)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// SteamID64 of the account last signed in, from
/// `config/loginusers.vdf`: the one marked `MostRecent`, else the latest
/// `Timestamp`.
pub fn steam_id(loginusers: &str) -> Option<u64> {
    let root = vdf::parse(loginusers);
    let users = root.get("users")?;
    users
        .entries()
        .iter()
        .filter_map(|(id, user)| {
            let id: u64 = id.parse().ok()?;
            let recent = user.str("MostRecent").or(user.str("mostrecent")) == Some("1");
            let at: u64 = user
                .str("Timestamp")
                .and_then(|t| t.trim().parse().ok())
                .unwrap_or(0);
            Some(((recent, at), id))
        })
        .max()
        .map(|(_, id)| id)
}

/// The account's SteamID64, read from the Steam folder.
pub fn local_steam_id(steam: &Path) -> Option<u64> {
    steam_id(&read(&steam.join("config").join("loginusers.vdf"))?)
}

/// Steam Web API: every game the account owns, names included.
pub fn owned_url(key: &str, steam_id: u64) -> String {
    format!(
        "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key={key}&steamid={steam_id}&include_appinfo=1&include_played_free_games=1&format=json"
    )
}

#[derive(serde::Deserialize)]
struct OwnedAnswer {
    response: OwnedList,
}

#[derive(serde::Deserialize)]
struct OwnedList {
    #[serde(default)]
    games: Vec<OwnedGame>,
}

#[derive(serde::Deserialize)]
struct OwnedGame {
    appid: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    rtime_last_played: u64,
}

/// The owned games of a `GetOwnedGames` answer, as entries to install.
/// Tools are left out.
pub fn parse_owned(json: &[u8]) -> Option<Vec<Game>> {
    let answer: OwnedAnswer = serde_json::from_slice(json).ok()?;
    Some(
        answer
            .response
            .games
            .into_iter()
            .filter(|g| !g.name.is_empty() && !is_tool(g.appid, &g.name))
            .map(|g| Game {
                id: format!("steam:{}", g.appid),
                title: g.name,
                source: Source::Steam,
                cover: None,
                launch: Launch::Install {
                    install: crate::install::Install::Steam { appid: g.appid },
                },
                last_played: Some(g.rtime_last_played).filter(|&t| t > 0),
            })
            .collect(),
    )
}

/// Where a Steam download stands, from its manifest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Download {
    /// Fully installed.
    Installed,
    /// Share downloaded, 0 to 1 (0 while Steam prepares).
    Progress(f32),
}

/// The state of an install from the text of its manifest.
pub fn parse_download(text: &str) -> Option<Download> {
    let root = vdf::parse(text);
    let app = root.get("AppState")?;
    let flags: u32 = app.str("StateFlags")?.trim().parse().ok()?;
    let number = |key| -> u64 {
        app.str(key)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    let (done, total) = (number("BytesDownloaded"), number("BytesToDownload"));
    // Bit 2 alone: installed and nothing left to fetch.
    if flags == 4 || (flags & 4 != 0 && total > 0 && done >= total) {
        return Some(Download::Installed);
    }
    let share = if total > 0 {
        (done as f32 / total as f32).min(1.0)
    } else {
        0.0
    };
    Some(Download::Progress(share))
}

/// The state of `appid`'s install, in any library folder.
pub fn download(steam: &Path, appid: u32) -> Option<Download> {
    let vdf_text = read(&steam.join("steamapps").join("libraryfolders.vdf"));
    library_folders(steam, vdf_text.as_deref())
        .into_iter()
        .find_map(|folder| {
            let path = folder
                .join("steamapps")
                .join(format!("appmanifest_{appid}.acf"));
            parse_download(&read(&path)?)
        })
}

/// Installed Steam games under the Steam folder `steam`.
pub fn scan(steam: &Path) -> Vec<Game> {
    let steamapps = steam.join("steamapps");
    let vdf_text = read(&steamapps.join("libraryfolders.vdf"));
    let mut played: HashMap<u32, u64> = HashMap::new();
    for user in dirs(&steam.join("userdata")) {
        if let Some(text) = read(&user.join("config").join("localconfig.vdf")) {
            for (appid, at) in parse_localconfig(&text) {
                let entry = played.entry(appid).or_default();
                *entry = (*entry).max(at);
            }
        }
    }

    let mut games = Vec::new();
    for folder in library_folders(steam, vdf_text.as_deref()) {
        let mut manifests: Vec<PathBuf> = std::fs::read_dir(folder.join("steamapps"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
            })
            .collect();
        manifests.sort();
        for path in manifests {
            let Some(m) = read(&path).as_deref().and_then(parse_manifest) else {
                continue;
            };
            games.push(Game {
                id: format!("steam:{}", m.appid),
                title: m.name,
                source: Source::Steam,
                cover: cover(steam, m.appid),
                launch: Launch::Uri {
                    uri: format!("steam://rungameid/{}", m.appid),
                },
                last_played: m.last_played.max(played.get(&m.appid).copied()),
            });
        }
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    #[test]
    fn steam_id_prefers_the_most_recent_account() {
        let text = r#""users"
        {
            "76561197960287930" { "AccountName" "old" "MostRecent" "0" "Timestamp" "1700000000" }
            "76561198000000001" { "AccountName" "me" "MostRecent" "1" "Timestamp" "1600000000" }
        }"#;
        assert_eq!(steam_id(text), Some(76561198000000001));
        let no_flag = r#""users" { "1" { "Timestamp" "5" } "2" { "Timestamp" "9" } }"#;
        assert_eq!(steam_id(no_flag), Some(2));
        assert_eq!(steam_id("\"users\" { }"), None);
    }

    #[test]
    fn owned_games_become_installs() {
        let json = br#"{"response":{"game_count":3,"games":[
            {"appid":400,"name":"Portal","playtime_forever":12,"rtime_last_played":1700000000},
            {"appid":228980,"name":"Steamworks Common Redistributables"},
            {"appid":620,"name":"Portal 2"}]}}"#;
        let games = parse_owned(json).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].id, "steam:400");
        assert_eq!(games[0].last_played, Some(1700000000));
        assert_eq!(
            games[1].launch,
            Launch::Install {
                install: crate::install::Install::Steam { appid: 620 }
            }
        );
        assert!(parse_owned(br#"{"response":{}}"#).unwrap().is_empty());
        assert!(parse_owned(b"<html>Forbidden</html>").is_none());
    }

    #[test]
    fn download_states() {
        let manifest = |flags: &str, done: &str, total: &str| {
            format!(
                r#""AppState" {{ "appid" "620" "StateFlags" "{flags}" "BytesDownloaded" "{done}" "BytesToDownload" "{total}" }}"#
            )
        };
        assert_eq!(
            parse_download(&manifest("4", "100", "100")),
            Some(Download::Installed)
        );
        assert_eq!(
            parse_download(&manifest("1026", "25", "100")),
            Some(Download::Progress(0.25))
        );
        assert_eq!(
            parse_download(&manifest("1026", "0", "0")),
            Some(Download::Progress(0.0))
        );
        assert_eq!(parse_download("nothing"), None);
    }

    #[test]
    fn library_folders_current_and_old_formats() {
        let steam = Path::new(r"C:\Steam");
        let text = std::fs::read_to_string(fixture("steam/steamapps/libraryfolders.vdf")).unwrap();
        let folders = library_folders(steam, Some(&text));
        assert_eq!(
            folders,
            [
                PathBuf::from(r"C:\Steam"),
                PathBuf::from(r"Q:\Fixture\Steam"),
                PathBuf::from(r"Q:\Fixture\SteamLibrary"),
            ]
        );
        let old = "\"LibraryFolders\" { \"TimeNextStatsReport\" \"1\" \"1\" \"E:\\\\Games\" }";
        assert_eq!(
            library_folders(steam, Some(old)),
            [PathBuf::from(r"C:\Steam"), PathBuf::from(r"E:\Games")]
        );
        assert_eq!(library_folders(steam, None), [PathBuf::from(r"C:\Steam")]);
    }

    #[test]
    fn manifests_keep_installed_games_only() {
        let read = |name: &str| {
            std::fs::read_to_string(fixture(&format!("steam/steamapps/{name}"))).unwrap()
        };
        assert_eq!(
            parse_manifest(&read("appmanifest_1245620.acf")),
            Some(Manifest {
                appid: 1245620,
                name: "ELDEN RING".into(),
                installdir: "ELDEN RING".into(),
                last_played: Some(1_792_500_000),
            })
        );
        // Redistributables, an unfinished download.
        assert_eq!(parse_manifest(&read("appmanifest_228980.acf")), None);
        assert_eq!(parse_manifest(&read("appmanifest_400.acf")), None);
        assert_eq!(
            parse_manifest(
                r#""AppState" { "appid" "1493710" "name" "Proton Experimental" "StateFlags" "4" }"#
            ),
            None
        );
        assert_eq!(parse_manifest("garbage"), None);
    }

    #[test]
    fn localconfig_last_played() {
        let text = std::fs::read_to_string(fixture("steam/userdata/12345/config/localconfig.vdf"))
            .unwrap();
        let played = parse_localconfig(&text);
        assert_eq!(played.get(&1145360), Some(&1_792_600_000));
        assert_eq!(played.get(&1245620), Some(&1_792_400_000));
        // LastPlayed "0": never.
        assert_eq!(played.get(&620), None);
    }

    #[test]
    fn scan_fixture() {
        let games = scan(&fixture("steam"));
        let titles: Vec<_> = games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Hades", "ELDEN RING"]);
        let hades = &games[0];
        assert_eq!(hades.id, "steam:1145360");
        assert_eq!(
            hades.launch,
            Launch::Uri {
                uri: "steam://rungameid/1145360".into()
            }
        );
        // From localconfig.vdf: the manifest has no LastPlayed.
        assert_eq!(hades.last_played, Some(1_792_600_000));
        // Current cache layout, one folder deeper.
        assert!(
            hades
                .cover
                .as_ref()
                .unwrap()
                .ends_with("1145360/abc123/library_600x900.jpg")
        );
        // The manifest's time is newer than localconfig's.
        assert_eq!(games[1].last_played, Some(1_792_500_000));
        // Older cache layout.
        assert!(
            games[1]
                .cover
                .as_ref()
                .unwrap()
                .ends_with("1245620_library_600x900.jpg")
        );
    }
}
