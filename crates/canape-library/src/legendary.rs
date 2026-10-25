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

/// The folder Legendary installed the game `app` in.
pub fn install_path(config: &Path, app: &str) -> Option<PathBuf> {
    let json = std::fs::read_to_string(config.join("installed.json")).ok()?;
    serde_json::from_str::<HashMap<String, Installed>>(&json)
        .ok()?
        .into_values()
        .find(|g| g.app_name == app && !g.install_path.is_empty())
        .map(|g| PathBuf::from(g.install_path))
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

/// Epic's art for a game, as links sized for Canapé (Epic's image server
/// scales on request).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Art {
    /// Landscape key art (16:9), for the game page.
    pub wide: Option<String>,
    /// Portrait box art (3:4), for the cover.
    pub tall: Option<String>,
}

/// The first image of `images` of one of `kinds`, in that order.
fn image<'a>(images: &'a [KeyImage], kinds: &[&str]) -> Option<&'a str> {
    kinds.iter().find_map(|kind| {
        images
            .iter()
            .find(|i| i.kind == *kind && i.url.starts_with("https://"))
            .map(|i| i.url.as_str())
    })
}

fn art_of(images: &[KeyImage]) -> Art {
    Art {
        wide: image(
            images,
            &["DieselGameBox", "OfferImageWide", "DieselStoreFrontWide"],
        )
        .map(|url| format!("{url}?w=1920&h=1080&resize=1")),
        tall: image(
            images,
            &["DieselGameBoxTall", "OfferImageTall", "Thumbnail"],
        )
        .map(|url| format!("{url}?w=600&h=900&resize=1")),
    }
}

#[derive(Deserialize)]
struct MetadataFile {
    #[serde(default)]
    metadata: Option<Metadata>,
}

/// The art of a game's metadata file, as Legendary keeps it for every
/// owned game (`metadata/<app>.json` in its config folder).
pub fn parse_metadata_art(json: &[u8]) -> Art {
    serde_json::from_slice::<MetadataFile>(json)
        .ok()
        .and_then(|f| f.metadata)
        .map(|m| art_of(&m.key_images))
        .unwrap_or_default()
}

/// Epic's art for the game `app`, from Legendary's metadata.
pub fn metadata_art(config: &Path, app: &str) -> Art {
    // App names are Epic's ids; anything else is not a file name of ours.
    if app.is_empty()
        || !app
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Art::default();
    }
    std::fs::read(config.join("metadata").join(format!("{app}.json")))
        .map(|json| parse_metadata_art(&json))
        .unwrap_or_default()
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
                let art_url = g.metadata.as_ref().and_then(|m| art_of(&m.key_images).tall);
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

/// Epic's sign-in page for Legendary's client: once signed in, the page
/// shows a small JSON answer whose `authorizationCode` signs Legendary in.
pub const LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

/// The sign-in code in `text`: the JSON answer Epic's page shows (or any
/// text holding its `"authorizationCode":"…"`), or the code alone.
pub fn parse_auth_code(text: &str) -> Option<String> {
    let text = text.trim();
    let is_code = |c: &str| c.len() == 32 && c.bytes().all(|b| b.is_ascii_hexdigit());
    if is_code(text) {
        return Some(text.to_ascii_lowercase());
    }
    let key = "\"authorizationCode\"";
    let rest = &text[text.find(key)? + key.len()..];
    let rest = rest
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    let code = &rest[..rest.find('"')?];
    is_code(code).then(|| code.to_ascii_lowercase())
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
    fn metadata_gives_wide_and_tall_art() {
        let json = br#"{"app_name": "Anemone", "app_title": "World of Goo",
            "asset_infos": {}, "base_urls": [],
            "metadata": {"id": "item", "namespace": "ns", "keyImages": [
                {"type": "Thumbnail", "url": "https://cdn1.epicgames.com/thumb.jpg"},
                {"type": "DieselGameBox", "url": "https://cdn1.epicgames.com/wide.jpg"},
                {"type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/tall.jpg"}]}}"#;
        assert_eq!(
            parse_metadata_art(json),
            Art {
                wide: Some("https://cdn1.epicgames.com/wide.jpg?w=1920&h=1080&resize=1".into()),
                tall: Some("https://cdn1.epicgames.com/tall.jpg?w=600&h=900&resize=1".into()),
            }
        );
        // Only a thumbnail, and a plain-HTTP wide image: no wide art.
        let json = br#"{"metadata": {"keyImages": [
                {"type": "Thumbnail", "url": "https://cdn1.epicgames.com/thumb.jpg"},
                {"type": "DieselGameBox", "url": "http://cdn1.epicgames.com/wide.jpg"}]}}"#;
        let art = parse_metadata_art(json);
        assert_eq!(art.wide, None);
        assert_eq!(
            art.tall.as_deref(),
            Some("https://cdn1.epicgames.com/thumb.jpg?w=600&h=900&resize=1")
        );
        assert_eq!(parse_metadata_art(b"{}"), Art::default());
        assert_eq!(parse_metadata_art(b"no"), Art::default());

        let dir = std::env::temp_dir().join(format!("canape-legendary-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("metadata")).unwrap();
        std::fs::write(
            dir.join("metadata/Anemone.json"),
            br#"{"metadata": {"keyImages": [{"type": "DieselGameBox", "url": "https://e/w.jpg"}]}}"#,
        )
        .unwrap();
        assert!(metadata_art(&dir, "Anemone").wide.is_some());
        assert_eq!(metadata_art(&dir, "../Anemone"), Art::default());
        assert_eq!(metadata_art(&dir, "Missing"), Art::default());
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn auth_code_from_the_page_or_alone() {
        let page = r#"{"warning":"Do not share this code","redirectUrl":"https://localhost/launcher/authorized?code=0123456789abcdef0123456789ABCDEF","authorizationCode":"0123456789abcdef0123456789ABCDEF","exchangeCode":null,"sid":null}"#;
        assert_eq!(
            parse_auth_code(page).as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            parse_auth_code(" 0123456789abcdef0123456789abcdef\n").as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(parse_auth_code(r#"{"authorizationCode":null}"#), None);
        assert_eq!(parse_auth_code("https://store.epicgames.com/"), None);
    }
}
