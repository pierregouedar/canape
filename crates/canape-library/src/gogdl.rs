//! GOG games without GOG Galaxy, through heroic-gogdl (an open-source GOG
//! downloader, GPL-3, run as a separate program): sign-in, the games the
//! account owns (GOG's web API, with gogdl's token), installs and their
//! progress, and the games installed this way.
//!
//! - **Sign-in**: GOG's sign-in page opens in the browser ([`login_url`]);
//!   once signed in, GOG lands on a page whose address carries a code
//!   ([`parse_code`]), which `gogdl auth --code` trades for tokens, kept
//!   by gogdl in Canapé's GOG folder.
//! - **Owned games**: `gogdl auth` prints the current token (refreshed if
//!   needed); [`owned_url`] lists the account's games page by page.
//! - **Install**: `gogdl info` gives the folder name and the size,
//!   `gogdl download` downloads into the games folder.
//! - **Launch**: the game's own executable, from the `goggame-<id>.info`
//!   file GOG ships with every game ([`parse_game_info`]). Galaxy is never
//!   needed.
//!
//! gogdl does not record its installs: Canapé keeps them in
//! `installed.json` in its GOG folder ([`Installed`]).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::install::Install;
use crate::online::Http;
use crate::{Game, Launch, Owned, Source};

/// GOG Galaxy's public OAuth client, the one GOG's sign-in page accepts
/// for a desktop client.
pub const CLIENT_ID: &str = "46899977096215655";
/// Where GOG's sign-in page lands, with `&code=…` added.
pub const REDIRECT: &str = "https://embed.gog.com/on_login_success?origin=client";
/// Language asked for when the game does not have the interface's.
pub const LANGUAGE: &str = "en-US";
/// Owned list pages read at most (100 games each).
pub const MAX_PAGES: u32 = 50;

/// GOG's sign-in page, to open in the browser.
pub fn login_url() -> String {
    format!(
        "https://auth.gog.com/auth?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&layout=client2",
        encode(REDIRECT)
    )
}

/// Percent-encodes everything but unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The sign-in code in `text`: the address of the page GOG lands on after
/// signing in (`…on_login_success?origin=client&code=…`), copied whole or
/// in part. A bare code is not accepted: any word on the clipboard would
/// pass for one.
pub fn parse_code(text: &str) -> Option<String> {
    let text = text.trim();
    if !text.contains("on_login_success") {
        return None;
    }
    let query = &text[text.find('?')? + 1..];
    let code = query
        .split(['&', '#'])
        .find_map(|pair| pair.strip_prefix("code="))?;
    let valid = !code.is_empty()
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b));
    valid.then(|| code.to_owned())
}

/// Tokens of the signed-in account, as `gogdl auth` prints them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    #[serde(default)]
    pub user_id: String,
}

/// The output of `gogdl auth` (`null` when nobody is signed in, or the
/// session expired; `{"error": true}` when a code was refused).
pub fn parse_credentials(json: &[u8]) -> Option<Credentials> {
    let creds: Credentials = serde_json::from_slice(json).ok()?;
    (!creds.access_token.is_empty()).then_some(creds)
}

/// The account's games, one page of 100 (from 1).
pub fn owned_url(page: u32) -> String {
    format!(
        "https://embed.gog.com/account/getFilteredProducts?mediaType=1&sortBy=title&page={page}"
    )
}

/// The signed-in account (`username`).
pub const USER_URL: &str = "https://embed.gog.com/userData.json";

#[derive(Deserialize)]
struct UserData {
    #[serde(default)]
    username: String,
}

/// The account name from [`USER_URL`]'s answer.
pub fn parse_user(json: &[u8]) -> Option<String> {
    let user: UserData = serde_json::from_slice(json).ok()?;
    let name = user.username.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[derive(Deserialize)]
struct ProductPage {
    #[serde(default, rename = "totalPages")]
    total_pages: u32,
    products: Vec<Product>,
}

#[derive(Deserialize)]
struct Product {
    id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    image: String,
    #[serde(default, rename = "worksOn")]
    works_on: HashMap<String, bool>,
    #[serde(default = "yes", rename = "isGame")]
    is_game: bool,
}

fn yes() -> bool {
    true
}

/// The Windows games of one [`owned_url`] page, and the page count.
pub fn parse_owned_page(json: &[u8]) -> Option<(Vec<Owned>, u32)> {
    let page: ProductPage = serde_json::from_slice(json).ok()?;
    let games = page
        .products
        .into_iter()
        .filter(|p| p.is_game && !p.title.trim().is_empty())
        .filter(|p| p.works_on.get("Windows").copied().unwrap_or(false))
        .map(|p| {
            // `//images-N.gog-statics.com/<hash>`: the store's art, in the
            // size the suffix asks for.
            let art_url = p
                .image
                .strip_prefix("//")
                .filter(|rest| rest.contains(".gog-statics.com/"))
                .map(|rest| format!("https://{rest}_392.jpg"));
            Owned {
                game: Game {
                    id: format!("gog:{}", p.id),
                    title: p.title.trim().to_owned(),
                    source: Source::Gog,
                    cover: None,
                    launch: Launch::Install {
                        install: Install::Gog {
                            id: p.id.to_string(),
                        },
                    },
                    last_played: None,
                    size: None,
                    playtime: None,
                },
                art_url,
            }
        })
        .collect();
    Some((games, page.total_pages))
}

/// Every Windows game the account owns, page after page, with its token.
pub fn fetch_owned(http: &dyn Http, token: &str) -> io::Result<Vec<Owned>> {
    let mut all = Vec::new();
    let mut page = 1;
    loop {
        let json = http.get(&owned_url(page), Some(token))?;
        let (games, pages) =
            parse_owned_page(&json).ok_or_else(|| io::Error::other("unreadable list of games"))?;
        all.extend(games);
        if page >= pages.min(MAX_PAGES) {
            return Ok(all);
        }
        page += 1;
    }
}

/// What `gogdl info` says about a game to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Folder created in the games folder.
    pub folder: String,
    /// Bytes on disk once installed.
    pub disk_size: u64,
    /// Bytes downloaded.
    pub download_size: u64,
    /// GOG's code of the language to download, such as `fr-FR`.
    pub language: String,
}

#[derive(Deserialize)]
struct InfoAnswer {
    size: HashMap<String, Sizes>,
    #[serde(default)]
    folder_name: String,
}

#[derive(Deserialize)]
struct Sizes {
    #[serde(default)]
    download_size: u64,
    #[serde(default)]
    disk_size: u64,
}

/// The output of `gogdl info <id> --platform windows`: the files every
/// language gets (`*`) plus the English ones.
pub fn parse_info(json: &[u8], preferred: &str) -> Option<Info> {
    let answer: InfoAnswer = serde_json::from_slice(json).ok()?;
    let languages: Vec<&str> = answer
        .size
        .keys()
        .map(String::as_str)
        .filter(|k| *k != "*")
        .collect();
    let language = pick_language(&languages, preferred).unwrap_or(LANGUAGE);
    let mut info = Info {
        folder: answer.folder_name,
        disk_size: 0,
        download_size: 0,
        language: language.to_owned(),
    };
    for sizes in answer
        .size
        .get("*")
        .into_iter()
        .chain(answer.size.get(language))
    {
        info.disk_size += sizes.disk_size;
        info.download_size += sizes.download_size;
    }
    (!info.folder.is_empty()).then_some(info)
}

/// The game's language closest to `preferred`, an interface language such
/// as `fr` or `zh_CN`: the same language, else English, else its only one.
pub fn pick_language<'a>(languages: &[&'a str], preferred: &str) -> Option<&'a str> {
    let base = |code: &str| {
        code.split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let wanted = base(preferred);
    let mut same: Vec<&str> = languages
        .iter()
        .copied()
        .filter(|l| base(l) == wanted)
        .collect();
    // Several variants (pt-BR and pt-PT): the exact one, else a stable pick.
    same.sort_unstable();
    let exact = same
        .iter()
        .copied()
        .find(|l| l.eq_ignore_ascii_case(&preferred.replace('_', "-")));
    exact
        .or(same.first().copied())
        .or_else(|| languages.iter().copied().find(|l| *l == LANGUAGE))
        .or(match languages {
            [only] => Some(*only),
            _ => None,
        })
}

/// Share done, 0 to 1, from one line of `gogdl download` output:
/// `[PROGRESS] INFO: = Progress: 12.34 123/1000, Running for: …`.
pub fn parse_progress(line: &str) -> Option<f32> {
    let rest = &line[line.find("= Progress: ")? + "= Progress: ".len()..];
    let percent: f32 = rest.split_whitespace().next()?.parse().ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

#[derive(Deserialize)]
struct GameInfo {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "playTasks")]
    play_tasks: Vec<PlayTask>,
}

#[derive(Deserialize)]
struct PlayTask {
    #[serde(default, rename = "isPrimary")]
    is_primary: bool,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    arguments: String,
    #[serde(default, rename = "workingDir")]
    working_dir: String,
}

/// The title and launch of a game installed in `dir`, from its
/// `goggame-<id>.info`: the primary task that starts a file.
pub fn parse_game_info(text: &str, dir: &Path) -> Option<(String, Launch)> {
    let info: GameInfo = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let task = info
        .play_tasks
        .iter()
        .filter(|t| t.kind == "FileTask" && !t.path.is_empty())
        .find(|t| t.is_primary)?;
    let cwd = if task.working_dir.is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(&task.working_dir)
    };
    Some((
        info.name.trim().to_owned(),
        Launch::Exe {
            path: dir.join(&task.path),
            args: task.arguments.clone(),
            cwd,
        },
    ))
}

/// A game Canapé installed with gogdl.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    /// GOG's product id.
    pub id: String,
    /// The game's folder.
    pub path: PathBuf,
    /// Bytes on disk, as `gogdl info` said.
    #[serde(default)]
    pub size: u64,
}

/// The game of one install, from the text of its `goggame-<id>.info`.
pub fn installed_game(entry: &Installed, info: &str) -> Option<Game> {
    let (title, launch) = parse_game_info(info, &entry.path)?;
    Some(Game {
        id: format!("gog:{}", entry.id),
        title: if title.is_empty() {
            entry.id.clone()
        } else {
            title
        },
        source: Source::Gog,
        cover: None,
        launch,
        last_played: None,
        size: Some(entry.size).filter(|&s| s > 0),
        playtime: None,
    })
}

/// The installs recorded in `record` (`installed.json`).
pub fn load_installed(record: &Path) -> Vec<Installed> {
    std::fs::read_to_string(record)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Records an install, replacing an older one of the same game.
pub fn record_install(record: &Path, entry: Installed) -> io::Result<()> {
    let mut all = load_installed(record);
    all.retain(|e| e.id != entry.id);
    all.push(entry);
    if let Some(parent) = record.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = record.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&all).map_err(io::Error::other)?,
    )?;
    std::fs::rename(&tmp, record)
}

/// Games installed with gogdl whose folder is still there.
pub fn scan(record: &Path) -> Vec<Game> {
    load_installed(record)
        .iter()
        .filter_map(|entry| {
            let info =
                std::fs::read_to_string(entry.path.join(format!("goggame-{}.info", entry.id)))
                    .ok()?;
            installed_game(entry, &info)
        })
        .collect()
}

/// Folders holding the games of `record`.
pub fn install_parents(record: &Path) -> Vec<PathBuf> {
    load_installed(record)
        .into_iter()
        .filter_map(|e| e.path.parent().map(Path::to_path_buf))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(&format!("gog/{name}"))).unwrap()
    }

    #[test]
    fn the_login_page_lands_on_the_redirect() {
        assert_eq!(
            login_url(),
            "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=client2"
        );
    }

    #[test]
    fn codes_come_from_the_landing_address() {
        let url = "https://embed.gog.com/on_login_success?origin=client&code=Ab-12_cd.EF~";
        assert_eq!(parse_code(url).as_deref(), Some("Ab-12_cd.EF~"));
        assert_eq!(
            parse_code(&format!("  {url}\r\n")).as_deref(),
            Some("Ab-12_cd.EF~")
        );
        assert_eq!(
            parse_code("embed.gog.com/on_login_success?code=xyz&origin=client").as_deref(),
            Some("xyz")
        );
        assert_eq!(parse_code("Ab12cd"), None);
        assert_eq!(
            parse_code("https://embed.gog.com/on_login_success?origin=client"),
            None
        );
        assert_eq!(
            parse_code("https://embed.gog.com/on_login_success?code=a b"),
            None
        );
    }

    #[test]
    fn credentials_or_nobody() {
        let creds = parse_credentials(&read("auth.json")).unwrap();
        assert_eq!(creds.access_token, "eyJ0eXAiOiJKV1QifQ.fake.token");
        assert_eq!(creds.user_id, "48628349957132247");
        assert_eq!(parse_credentials(b"null\n"), None);
        assert_eq!(parse_credentials(br#"{"error": true}"#), None);
    }

    #[test]
    fn owned_windows_games_only() {
        let (games, pages) = parse_owned_page(&read("owned-page.json")).unwrap();
        assert_eq!(pages, 2);
        let ids: Vec<&str> = games.iter().map(|g| g.game.id.as_str()).collect();
        assert_eq!(ids, ["gog:1207658924", "gog:1453375253"]);
        assert_eq!(
            games[0].game.title,
            "Unreal Tournament 2004 Editor's Choice Edition"
        );
        assert_eq!(
            games[0].game.launch,
            Launch::Install {
                install: Install::Gog {
                    id: "1207658924".into()
                }
            }
        );
        assert_eq!(
            games[0].art_url.as_deref(),
            Some("https://images-2.gog-statics.com/c6ae6eb4e4e0d1b9f8c6a7e2a1f3c4d5_392.jpg")
        );
        assert_eq!(games[1].art_url, None);
        assert!(parse_owned_page(b"[]").is_none());
        assert_eq!(parse_user(&read("user.json")).as_deref(), Some("pierre_g"));
        assert_eq!(parse_user(br#"{"isLoggedIn": false}"#), None);
    }

    #[test]
    fn every_page_is_read_with_the_token() {
        struct Pages(std::cell::RefCell<Vec<(String, Option<String>)>>);
        impl Http for Pages {
            fn get_limited(
                &self,
                url: &str,
                bearer: Option<&str>,
                _: usize,
            ) -> io::Result<Vec<u8>> {
                self.0
                    .borrow_mut()
                    .push((url.to_owned(), bearer.map(str::to_owned)));
                Ok(std::fs::read(fixture("gog/owned-page.json")).unwrap())
            }

            fn post(&self, url: &str, _: &[(&str, &str)], _: &[u8]) -> io::Result<Vec<u8>> {
                panic!("no POST expected: {url}")
            }
        }
        let http = Pages(Default::default());
        let games = fetch_owned(&http, "tok").unwrap();
        assert_eq!(games.len(), 4);
        let asked = http.0.borrow();
        assert_eq!(asked.len(), 2);
        assert!(asked[1].0.ends_with("&page=2"));
        assert_eq!(asked[0].1.as_deref(), Some("tok"));
    }

    #[test]
    fn info_counts_common_and_chosen_language_files() {
        let info = parse_info(&read("info.json"), "en").unwrap();
        assert_eq!(info.folder, "Unreal Tournament 2004 ECE");
        assert_eq!(info.language, "en-US");
        assert_eq!(info.disk_size, 5_515_646_012 + 1_103_729_121);
        assert_eq!(info.download_size, 2_510_394_220 + 483_520_811);
        let french = parse_info(&read("info.json"), "fr").unwrap();
        assert_eq!(french.language, "fr-FR");
        assert_eq!(french.disk_size, 5_515_646_012 + 1_090_221_551);
        // Not in the game's languages: English.
        assert_eq!(
            parse_info(&read("info.json"), "ja").unwrap().language,
            "en-US"
        );
        // A single other language counts when there is no English.
        let json = br#"{"size": {"*": {"download_size": 10, "disk_size": 20},
            "de-DE": {"download_size": 1, "disk_size": 2}}, "folder_name": "Spiel"}"#;
        assert_eq!(parse_info(json, "fr").unwrap().disk_size, 22);
        assert!(parse_info(br#"{"size": {}, "folder_name": ""}"#, "en").is_none());
    }

    #[test]
    fn language_variants() {
        let langs = ["en-US", "pt-BR", "pt-PT", "zh-Hans"];
        assert_eq!(pick_language(&langs, "pt"), Some("pt-BR"));
        assert_eq!(pick_language(&langs, "zh_CN"), Some("zh-Hans"));
        assert_eq!(pick_language(&["de-DE", "fr-FR"], "ja"), None);
    }

    #[test]
    fn progress_lines() {
        let log = String::from_utf8(read("download.log")).unwrap();
        let shares: Vec<f32> = log.lines().filter_map(parse_progress).collect();
        assert_eq!(shares.len(), 3);
        assert_eq!(shares[0], 0.0);
        assert!((shares[1] - 0.4215).abs() < 1e-6);
        assert_eq!(shares[2], 1.0);
    }

    #[test]
    fn installed_games_launch_their_primary_task() {
        let text = String::from_utf8(read("goggame-1207658924.info")).unwrap();
        let entry = Installed {
            id: "1207658924".into(),
            path: PathBuf::from(r"D:\Games\Unreal Tournament 2004 ECE"),
            size: 6_619_375_133,
        };
        let game = installed_game(&entry, &text).unwrap();
        assert_eq!(game.id, "gog:1207658924");
        assert_eq!(game.title, "Unreal Tournament 2004 Editor's Choice Edition");
        assert_eq!(game.size, Some(6_619_375_133));
        let dir = PathBuf::from(r"D:\Games\Unreal Tournament 2004 ECE");
        assert_eq!(
            game.launch,
            Launch::Exe {
                path: dir.join("System\\UT2004.exe"),
                args: "-windowed".into(),
                cwd: dir.join("System"),
            }
        );
        // No primary file task: nothing to launch.
        let json = r#"{"name": "X", "playTasks": [{"type": "URLTask", "isPrimary": true, "link": "https://x"}]}"#;
        assert!(parse_game_info(json, &dir).is_none());
    }

    #[test]
    fn installs_are_recorded() {
        let dir = std::env::temp_dir().join(format!("canape-gog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let record = dir.join("installed.json");
        let game = dir.join("Games").join("UT");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(
            game.join("goggame-1207658924.info"),
            read("goggame-1207658924.info"),
        )
        .unwrap();
        let entry = |size| Installed {
            id: "1207658924".into(),
            path: game.clone(),
            size,
        };
        record_install(&record, entry(1)).unwrap();
        record_install(&record, entry(2)).unwrap();
        assert_eq!(load_installed(&record), [entry(2)]);
        let games = scan(&record);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].size, Some(2));
        assert_eq!(install_parents(&record), [dir.join("Games")]);
        // A deleted folder leaves the list.
        std::fs::remove_dir_all(&game).unwrap();
        assert!(scan(&record).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
