//! Epic games without the Epic Games Launcher, through Legendary (an
//! open-source client, run as a separate program): the games it installed
//! (`installed.json` in its config folder), the games the account owns
//! (`legendary list --json`), installs and their progress.
//!
//! Legendary keeps its config in `%USERPROFILE%\.config\legendary`. Games
//! it installs are launched with `legendary launch <app>`, which signs in
//! to Epic for games that need it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::install::Install;
use crate::{Game, Launch, Owned, Source};

/// Legendary's config folder for the current user.
pub fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".config").join("legendary"))
}

#[derive(Deserialize)]
struct Installed {
    app_name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    install_path: String,
    #[serde(default)]
    is_dlc: bool,
    #[serde(default)]
    install_size: u64,
}

/// Games listed in an `installed.json`, launched through `exe`.
pub fn parse_installed(json: &str, exe: &Path) -> Vec<Game> {
    let Ok(map) = serde_json::from_str::<HashMap<String, Installed>>(json) else {
        return Vec::new();
    };
    let mut games: Vec<Game> = map
        .into_values()
        .filter(|g| !g.is_dlc && !g.title.is_empty())
        .map(|g| Game {
            id: format!("epic:{}", g.app_name),
            title: g.title,
            source: Source::Epic,
            cover: None,
            launch: Launch::Exe {
                path: exe.to_path_buf(),
                args: format!("launch {}", g.app_name),
                cwd: PathBuf::from(g.install_path),
            },
            last_played: None,
            size: Some(g.install_size).filter(|&s| s > 0),
            playtime: None,
        })
        .collect();
    games.sort_by(|a, b| a.id.cmp(&b.id));
    games
}

/// The folders of the games installed with Legendary.
pub fn install_paths(config: &Path) -> Vec<PathBuf> {
    let Ok(json) = std::fs::read_to_string(config.join("installed.json")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = serde_json::from_str::<HashMap<String, Installed>>(&json)
        .unwrap_or_default()
        .into_values()
        .filter(|g| !g.is_dlc && !g.install_path.is_empty())
        .map(|g| PathBuf::from(g.install_path))
        .collect();
    paths.sort();
    paths
}

/// Games installed with Legendary, from its config folder.
pub fn scan(config: &Path, exe: &Path) -> Vec<Game> {
    std::fs::read_to_string(config.join("installed.json"))
        .map(|json| parse_installed(&json, exe))
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct Listed {
    app_name: String,
    #[serde(default)]
    app_title: String,
    #[serde(default)]
    metadata: Option<Metadata>,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default, rename = "keyImages")]
    key_images: Vec<KeyImage>,
    #[serde(default, rename = "customAttributes")]
    custom_attributes: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct KeyImage {
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

/// Games of a `legendary list --json` answer, as entries to install.
/// Games installed through another store (Ubisoft, EA) are left out:
/// Legendary cannot install them.
pub fn parse_list(json: &[u8]) -> Option<Vec<Owned>> {
    let listed: Vec<Listed> = serde_json::from_slice(json).ok()?;
    Some(
        listed
            .into_iter()
            .filter(|g| !g.app_title.is_empty())
            .filter(|g| {
                g.metadata.as_ref().is_none_or(|m| {
                    !m.custom_attributes.contains_key("ThirdPartyManagedApp")
                        && !m
                            .custom_attributes
                            .contains_key("ThirdPartyManagedProvider")
                })
            })
            .map(|g| {
                let art_url = g.metadata.as_ref().and_then(|m| {
                    m.key_images
                        .iter()
                        .find(|i| i.kind == "DieselGameBoxTall")
                        .filter(|i| i.url.starts_with("https://"))
                        // Epic's image server scales on request.
                        .map(|i| format!("{}?w=600&h=900&resize=1", i.url))
                });
                Owned {
                    game: Game {
                        id: format!("epic:{}", g.app_name),
                        title: g.app_title,
                        source: Source::Epic,
                        cover: None,
                        launch: Launch::Install {
                            install: Install::Epic { app: g.app_name },
                        },
                        last_played: None,
                        size: None,
                        playtime: None,
                    },
                    art_url,
                }
            })
            .collect(),
    )
}

/// Share done, 0 to 1, from one line of `legendary install` output:
/// `[DLManager] INFO: = Progress: 12.34% (123/1000), Running for …`.
pub fn parse_progress(line: &str) -> Option<f32> {
    let rest = &line[line.find("= Progress: ")? + "= Progress: ".len()..];
    let percent: f32 = rest[..rest.find('%')?].trim().parse().ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

#[derive(Deserialize)]
struct Info {
    manifest: Option<InfoManifest>,
}

#[derive(Deserialize)]
struct InfoManifest {
    #[serde(default)]
    disk_size: u64,
}

/// Bytes on disk once installed, from `legendary info <app> --json`.
pub fn parse_info_size(json: &[u8]) -> Option<u64> {
    let info: Info = serde_json::from_slice(json).ok()?;
    info.manifest.map(|m| m.disk_size).filter(|&s| s > 0)
}

#[derive(Deserialize)]
struct Status {
    #[serde(default)]
    account: String,
}

/// The signed-in Epic account from `legendary status --json`, if any.
pub fn parse_account(json: &[u8]) -> Option<String> {
    let status: Status = serde_json::from_slice(json).ok()?;
    let account = status.account.trim();
    (!account.is_empty() && !account.starts_with('<')).then(|| account.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_games_launch_through_legendary() {
        let json = r#"{
            "Anemone": {"app_name": "Anemone", "title": "World of Goo", "install_path": "C:\\Games\\WorldOfGoo", "is_dlc": false},
            "AnemoneDLC": {"app_name": "AnemoneDLC", "title": "Extra", "install_path": "C:\\Games\\WorldOfGoo", "is_dlc": true}
        }"#;
        let games = parse_installed(json, Path::new(r"C:\Canape\tools\legendary.exe"));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, "epic:Anemone");
        assert_eq!(
            games[0].launch,
            Launch::Exe {
                path: PathBuf::from(r"C:\Canape\tools\legendary.exe"),
                args: "launch Anemone".into(),
                cwd: PathBuf::from(r"C:\Games\WorldOfGoo"),
            }
        );
        assert!(parse_installed("not json", Path::new("x")).is_empty());
    }

    #[test]
    fn owned_list_keeps_installable_games() {
        let json = br#"[
            {"app_name": "Anemone", "app_title": "World of Goo", "metadata": {
                "keyImages": [
                    {"type": "DieselGameBox", "url": "https://cdn1.epicgames.com/wide.jpg"},
                    {"type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/tall.jpg"}],
                "customAttributes": {}}},
            {"app_name": "Ubi", "app_title": "A Ubisoft game", "metadata": {
                "customAttributes": {"ThirdPartyManagedApp": {"type": "STRING", "value": "Uplay"}}}},
            {"app_name": "Bare", "app_title": "No metadata"}
        ]"#;
        let owned = parse_list(json).unwrap();
        assert_eq!(owned.len(), 2);
        assert_eq!(owned[0].game.id, "epic:Anemone");
        assert_eq!(
            owned[0].art_url.as_deref(),
            Some("https://cdn1.epicgames.com/tall.jpg?w=600&h=900&resize=1")
        );
        assert_eq!(
            owned[1].game.launch,
            Launch::Install {
                install: Install::Epic { app: "Bare".into() }
            }
        );
        assert!(parse_list(b"{}").is_none());
    }

    #[test]
    fn info_gives_the_size_on_disk() {
        let json = br#"{"game": {"app_name": "Anemone", "title": "World of Goo"},
            "install": null,
            "manifest": {"size": 123456, "type": "binary", "disk_size": 1073741824,
                "download_size": 536870912, "launch_exe": "WorldOfGoo.exe"}}"#;
        assert_eq!(parse_info_size(json), Some(1_073_741_824));
        assert_eq!(
            parse_info_size(br#"{"game": null, "install": null, "manifest": null}"#),
            None
        );
        assert_eq!(parse_info_size(b"no"), None);
    }

    #[test]
    fn progress_and_account() {
        let line =
            "[DLManager] INFO: = Progress: 12.34% (123/1000), Running for 00:00:10, ETA: 00:01:00";
        assert!((parse_progress(line).unwrap() - 0.1234).abs() < 1e-6);
        assert_eq!(parse_progress("[cli] INFO: Download size: 1.2 GiB"), None);
        assert_eq!(
            parse_account(br#"{"account": "Pierre", "games_available": 12}"#).as_deref(),
            Some("Pierre")
        );
        assert_eq!(parse_account(br#"{"account": "<not logged in>"}"#), None);
    }
}
