//! Achievements of a Steam game, through the Steam Web API with the key and
//! the account already used for the owned games: the game's list (names,
//! descriptions, icons), which ones the account has unlocked and when, and
//! the share of players who have each.
//!
//! Three requests per game: `GetSchemaForGame` first (a game without
//! achievements stops there), `GetPlayerAchievements` (refused when the
//! profile's game details are not public), then
//! `GetGlobalAchievementPercentagesForApp` (optional: without it no
//! rarity). The key only goes to `api.steampowered.com`; nothing here logs
//! or formats a URL. Answers are read by pure functions; the transport is
//! the [`Http`] trait.
//!
//! The lists are kept per Canapé game id in a cache file
//! ([`Cache`]), with the date and the language of the names.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::online::Http;

/// The Steam Web API's achievement interface.
pub const API: &str = "https://api.steampowered.com/ISteamUserStats";
/// A list is asked for again when its page opens after this long, seconds.
pub const FRESH_FOR: u64 = 3600;
/// Icons are small: a larger answer is not one.
pub const ICON_MAX_BYTES: usize = 512 * 1024;
const CACHE_VERSION: u32 = 1;

/// One achievement, as shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Achievement {
    /// The game's own name for it (`apiname`).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The game keeps its description secret until it is unlocked.
    #[serde(default)]
    pub hidden: bool,
    /// Icon URLs: unlocked, and the greyed one shown while locked.
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub icon_locked: String,
    /// Unlocked: when, unix seconds (0 when the date is unknown).
    #[serde(default)]
    pub unlocked: Option<u64>,
    /// Share of the game's players who have it, percent.
    #[serde(default)]
    pub percent: Option<f32>,
}

impl Achievement {
    /// The icon to show: the greyed one while locked.
    pub fn shown_icon(&self) -> &str {
        if self.unlocked.is_some() || self.icon_locked.is_empty() {
            &self.icon
        } else {
            &self.icon_locked
        }
    }
}

/// Steam's name for an interface language code (see the shell's
/// `i18n::LANGUAGES`), for the names and descriptions.
pub fn steam_language(code: &str) -> &'static str {
    match code {
        "fr" => "french",
        "es" => "spanish",
        "de" => "german",
        "it" => "italian",
        "pt" => "portuguese",
        "nl" => "dutch",
        "pl" => "polish",
        "ru" => "russian",
        "tr" => "turkish",
        "ja" => "japanese",
        "ko" => "koreana",
        "zh_CN" => "schinese",
        _ => "english",
    }
}

pub fn schema_url(key: &str, appid: u32, language: &str) -> String {
    format!("{API}/GetSchemaForGame/v2/?key={key}&appid={appid}&l={language}&format=json")
}

pub fn player_url(key: &str, steam_id: u64, appid: u32, language: &str) -> String {
    format!(
        "{API}/GetPlayerAchievements/v1/?key={key}&steamid={steam_id}&appid={appid}&l={language}&format=json"
    )
}

/// Needs no key.
pub fn global_url(appid: u32) -> String {
    format!("{API}/GetGlobalAchievementPercentagesForApp/v2/?gameid={appid}&format=json")
}

#[derive(Deserialize)]
struct SchemaAnswer {
    game: SchemaGame,
}

#[derive(Deserialize)]
struct SchemaGame {
    #[serde(default, rename = "availableGameStats")]
    stats: Option<SchemaStats>,
}

#[derive(Deserialize)]
struct SchemaStats {
    #[serde(default)]
    achievements: Vec<SchemaItem>,
}

#[derive(Deserialize)]
struct SchemaItem {
    name: String,
    #[serde(default, rename = "displayName")]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    hidden: u8,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    icongray: String,
}

/// The achievements a game has, locked, in the game's order; empty when it
/// has none, `None` when the answer cannot be read.
pub fn parse_schema(json: &[u8]) -> Option<Vec<Achievement>> {
    let answer: SchemaAnswer = serde_json::from_slice(json).ok()?;
    let items = answer
        .game
        .stats
        .map(|s| s.achievements)
        .unwrap_or_default();
    Some(
        items
            .into_iter()
            .map(|item| Achievement {
                name: if item.display_name.is_empty() {
                    item.name.clone()
                } else {
                    item.display_name
                },
                id: item.name,
                description: item.description,
                hidden: item.hidden != 0,
                icon: item.icon,
                icon_locked: item.icongray,
                unlocked: None,
                percent: None,
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct PlayerAnswer {
    playerstats: PlayerStats,
}

#[derive(Deserialize)]
struct PlayerStats {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    achievements: Vec<PlayerItem>,
}

#[derive(Deserialize)]
struct PlayerItem {
    apiname: String,
    #[serde(default)]
    achieved: u8,
    #[serde(default)]
    unlocktime: u64,
    #[serde(default)]
    description: String,
}

/// What the account has: per achievement id, the unlock date (0 unknown)
/// and the description in the asked language, which the schema leaves
/// out for hidden ones.
#[derive(Debug, Default, PartialEq)]
pub struct Unlocks {
    pub unlocked: HashMap<String, u64>,
    pub descriptions: HashMap<String, String>,
}

/// The account's achievements; `Err` with Steam's reason when it refused
/// (a private profile), or a generic one when the answer cannot be read.
pub fn parse_player(json: &[u8]) -> Result<Unlocks, String> {
    let answer: PlayerAnswer =
        serde_json::from_slice(json).map_err(|_| "unreadable answer".to_owned())?;
    let stats = answer.playerstats;
    if !stats.success {
        return Err(if stats.error.is_empty() {
            "refused".into()
        } else {
            stats.error
        });
    }
    let mut unlocks = Unlocks::default();
    for item in stats.achievements {
        if item.achieved != 0 {
            unlocks
                .unlocked
                .insert(item.apiname.clone(), item.unlocktime);
        }
        if !item.description.is_empty() {
            unlocks.descriptions.insert(item.apiname, item.description);
        }
    }
    Ok(unlocks)
}

#[derive(Deserialize)]
struct GlobalAnswer {
    achievementpercentages: GlobalList,
}

#[derive(Deserialize)]
struct GlobalList {
    #[serde(default)]
    achievements: Vec<GlobalItem>,
}

#[derive(Deserialize)]
struct GlobalItem {
    name: String,
    percent: serde_json::Value,
}

/// Share of players per achievement id, percent. The share comes as a
/// number or as a string holding one.
pub fn parse_global(json: &[u8]) -> Option<HashMap<String, f32>> {
    let answer: GlobalAnswer = serde_json::from_slice(json).ok()?;
    Some(
        answer
            .achievementpercentages
            .achievements
            .into_iter()
            .filter_map(|item| {
                let percent = match &item.percent {
                    serde_json::Value::Number(n) => n.as_f64(),
                    serde_json::Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                }?;
                Some((item.name, (percent as f32).clamp(0.0, 100.0)))
            })
            .collect(),
    )
}

/// The schema with what the account has and how rare each one is:
/// unlocked first, the latest first, then locked, the most common first.
pub fn merge(
    mut list: Vec<Achievement>,
    unlocks: &Unlocks,
    percents: &HashMap<String, f32>,
) -> Vec<Achievement> {
    for a in &mut list {
        a.unlocked = unlocks.unlocked.get(&a.id).copied();
        a.percent = percents.get(&a.id).copied();
        // A hidden one keeps its secret until unlocked.
        if a.description.is_empty()
            && (!a.hidden || a.unlocked.is_some())
            && let Some(text) = unlocks.descriptions.get(&a.id)
        {
            a.description = text.clone();
        }
    }
    list.sort_by(|a, b| match (a.unlocked, b.unlocked) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b
            .percent
            .unwrap_or(-1.0)
            .total_cmp(&a.percent.unwrap_or(-1.0)),
    });
    list
}

/// Where an icon can be downloaded: its own address, then the same file
/// on Steam's other image hosts (the oldest one no longer serves every
/// game).
pub fn icon_urls(url: &str) -> Vec<String> {
    const HOSTS: [&str; 3] = [
        "steamcdn-a.akamaihd.net",
        "cdn.cloudflare.steamstatic.com",
        "cdn.akamai.steamstatic.com",
    ];
    let mut urls = vec![url.to_owned()];
    if let Some(host) = HOSTS.iter().find(|h| url.contains(*h)) {
        urls.extend(
            HOSTS
                .iter()
                .filter(|h| *h != host)
                .map(|h| url.replacen(host, h, 1)),
        );
    }
    urls
}

/// The HTTP status of an error from [`Http`] ("HTTP 403"), if it is one.
pub fn http_status(err: &io::Error) -> Option<u32> {
    err.to_string().strip_prefix("HTTP ")?.trim().parse().ok()
}

/// What Steam answered for a game.
#[derive(Debug, PartialEq)]
pub enum Answer {
    List(Vec<Achievement>),
    /// Nothing to show, and why (for the log).
    Nothing(String),
}

/// Asks Steam for the achievements of `appid` and the account `steam_id`.
/// `Err` for a network or server error, worth trying again later.
pub fn fetch(
    http: &dyn Http,
    key: &str,
    steam_id: u64,
    appid: u32,
    language: &str,
) -> io::Result<Answer> {
    let schema = http.get(&schema_url(key, appid, language), None)?;
    let list = parse_schema(&schema).ok_or_else(|| io::Error::other("unreadable schema"))?;
    if list.is_empty() {
        return Ok(Answer::Nothing("no achievements".into()));
    }
    let unlocks = match http.get(&player_url(key, steam_id, appid, language), None) {
        Ok(json) => match parse_player(&json) {
            Ok(unlocks) => unlocks,
            Err(reason) => return Ok(Answer::Nothing(reason)),
        },
        // Private game details, or no stats for this game.
        Err(err) if matches!(http_status(&err), Some(400 | 403)) => {
            return Ok(Answer::Nothing(format!(
                "{err} (private game details, or no stats)"
            )));
        }
        Err(err) => return Err(err),
    };
    let percents = http
        .get(&global_url(appid), None)
        .ok()
        .and_then(|json| parse_global(&json))
        .unwrap_or_default();
    Ok(Answer::List(merge(list, &unlocks, &percents)))
}

/// The cache file name of an icon: a hash of its URL, with its extension.
pub fn icon_file(url: &str) -> String {
    let digest = crate::sha256::digest(url.as_bytes());
    let name: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let ext = match path.rsplit_once('.') {
        Some((_, ext)) if ext.eq_ignore_ascii_case("png") => "png",
        _ => "jpg",
    };
    format!("{name}.{ext}")
}

/// One game's list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub at: u64,
    /// Steam's language of the names.
    #[serde(default)]
    pub language: String,
    /// `None`: nothing to show (no achievements, private profile).
    #[serde(default)]
    pub list: Option<Vec<Achievement>>,
}

/// Lists by Canapé game id: `%LOCALAPPDATA%\Canape\achievements.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    pub games: HashMap<String, Entry>,
}

impl Cache {
    /// The saved cache; empty when missing, unreadable or of another version.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .filter(|c| c.version == CACHE_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let file = Self {
            version: CACHE_VERSION,
            games: self.games.clone(),
        };
        crate::igdb::write(
            path,
            &serde_json::to_string(&file).map_err(io::Error::other)?,
        )
    }

    /// Whether the list of `id` in `language` is recent enough.
    pub fn fresh(&self, id: &str, language: &str, now: u64) -> bool {
        self.games
            .get(id)
            .is_some_and(|e| e.language == language && now < e.at + FRESH_FOR)
    }

    pub fn record(&mut self, id: &str, language: &str, list: Option<Vec<Achievement>>, now: u64) {
        self.games.insert(
            id.to_owned(),
            Entry {
                at: now,
                language: language.to_owned(),
                list,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;
    use std::cell::RefCell;

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(&format!("steam/achievements/{name}"))).unwrap()
    }

    /// Answers by URL; an `Err` answer is an HTTP status.
    struct Fake {
        answers: Vec<(String, Result<Vec<u8>, u32>)>,
        asked: RefCell<Vec<String>>,
    }

    impl Http for Fake {
        fn get_limited(&self, url: &str, _: Option<&str>, _: usize) -> io::Result<Vec<u8>> {
            self.asked.borrow_mut().push(url.to_owned());
            match self.answers.iter().find(|(u, _)| u == url) {
                Some((_, Ok(body))) => Ok(body.clone()),
                Some((_, Err(status))) => Err(io::Error::other(format!("HTTP {status}"))),
                None => Err(io::Error::other("no route")),
            }
        }

        fn post(&self, _: &str, _: &[(&str, &str)], _: &[u8]) -> io::Result<Vec<u8>> {
            unreachable!()
        }
    }

    #[test]
    fn urls_carry_the_language() {
        assert_eq!(
            player_url("K", 7, 900001, "french"),
            "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v1/?key=K&steamid=7&appid=900001&l=french&format=json"
        );
        assert!(schema_url("K", 9, "koreana").contains("appid=9&l=koreana"));
        assert!(!global_url(9).contains("key="));
        assert_eq!(steam_language("zh_CN"), "schinese");
        assert_eq!(steam_language("en"), "english");
        assert_eq!(steam_language("xx"), "english");
    }

    #[test]
    fn schema_lists_names_icons_and_hidden_ones() {
        let list = parse_schema(&read("schema.json")).unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].id, "ACH_FIRST_LIGHT");
        assert_eq!(list[0].name, "First Light");
        assert_eq!(list[0].description, "Reach the lighthouse.");
        assert!(list[0].icon.ends_with("1a2b3c.jpg"));
        assert!(list[0].icon_locked.ends_with("4d5e6f.jpg"));
        assert!(list[2].hidden && list[2].description.is_empty());
        assert_eq!(parse_schema(&read("schema-none.json")), Some(Vec::new()));
        assert_eq!(parse_schema(b"<html>"), None);
    }

    #[test]
    fn player_unlocks_and_refusals() {
        let unlocks = parse_player(&read("player.json")).unwrap();
        assert_eq!(unlocks.unlocked.len(), 2);
        assert_eq!(unlocks.unlocked["ACH_DEEP_WATER"], 1_710_000_000);
        assert_eq!(
            parse_player(&read("player-private.json")),
            Err("Profile is not public".into())
        );
        assert!(parse_player(b"{}").is_err());
    }

    #[test]
    fn global_shares_are_numbers_or_strings() {
        let percents = parse_global(&read("global.json")).unwrap();
        assert_eq!(percents["ACH_FIRST_LIGHT"], 88.4);
        assert_eq!(percents["ACH_DEEP_WATER"], 41.25);
        assert_eq!(percents.len(), 4);
    }

    #[test]
    fn unlocked_come_first_latest_first_then_most_common() {
        let list = merge(
            parse_schema(&read("schema.json")).unwrap(),
            &parse_player(&read("player.json")).unwrap(),
            &parse_global(&read("global.json")).unwrap(),
        );
        let ids: Vec<_> = list.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "ACH_DEEP_WATER",
                "ACH_FIRST_LIGHT",
                "ACH_ENDING",
                "ACH_COLLECTOR"
            ]
        );
        assert_eq!(list[0].unlocked, Some(1_710_000_000));
        assert_eq!(list[3].percent, Some(3.1));
        // Hidden and locked: no description, even though Steam sent one.
        assert!(list[2].hidden && list[2].description.is_empty());
        // Locked: the greyed icon.
        assert!(list[0].shown_icon().ends_with("7a8b9c.jpg"));
        assert!(list[3].shown_icon().ends_with("2d3e4f.jpg"));
    }

    fn fake(player: Result<Vec<u8>, u32>, global: Result<Vec<u8>, u32>) -> Fake {
        Fake {
            answers: vec![
                (schema_url("K", 900001, "english"), Ok(read("schema.json"))),
                (player_url("K", 7, 900001, "english"), player),
                (global_url(900001), global),
                (
                    schema_url("K", 900002, "english"),
                    Ok(read("schema-none.json")),
                ),
            ],
            asked: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn fetch_asks_three_times_and_tolerates_no_rarity() {
        let http = fake(Ok(read("player.json")), Ok(read("global.json")));
        let Answer::List(list) = fetch(&http, "K", 7, 900001, "english").unwrap() else {
            panic!("no list");
        };
        assert_eq!(list.len(), 4);
        assert_eq!(http.asked.borrow().len(), 3);
        // The key only goes to the Steam Web API.
        for url in http.asked.borrow().iter() {
            assert!(url.starts_with(API));
        }
        let http = fake(Ok(read("player.json")), Err(500));
        let Answer::List(list) = fetch(&http, "K", 7, 900001, "english").unwrap() else {
            panic!("no list");
        };
        assert!(list.iter().all(|a| a.percent.is_none()));
    }

    #[test]
    fn fetch_shows_nothing_for_private_profiles_and_games_without() {
        // A game without achievements: one request.
        let http = fake(Err(403), Err(500));
        assert_eq!(
            fetch(&http, "K", 7, 900002, "english").unwrap(),
            Answer::Nothing("no achievements".into())
        );
        assert_eq!(http.asked.borrow().len(), 1);
        assert!(matches!(
            fetch(&http, "K", 7, 900001, "english").unwrap(),
            Answer::Nothing(_)
        ));
        let http = fake(Ok(read("player-private.json")), Err(500));
        assert_eq!(
            fetch(&http, "K", 7, 900001, "english").unwrap(),
            Answer::Nothing("Profile is not public".into())
        );
        // A server error is worth trying again.
        let http = fake(Err(503), Err(500));
        assert!(fetch(&http, "K", 7, 900001, "english").is_err());
    }

    #[test]
    fn statuses_are_read_from_errors() {
        assert_eq!(http_status(&io::Error::other("HTTP 403")), Some(403));
        assert_eq!(http_status(&io::Error::other("timed out")), None);
    }

    #[test]
    fn icon_files_are_named_by_url() {
        let a = icon_file("https://cdn.example/apps/1/abc.jpg");
        assert_eq!(a.len(), 32 + 4);
        assert!(a.ends_with(".jpg"));
        assert_ne!(a, icon_file("https://cdn.example/apps/1/abd.jpg"));
        assert!(icon_file("https://cdn.example/x.PNG?v=2").ends_with(".png"));
        // The fake library names its icons the same way.
        assert_eq!(
            icon_file("https://example.invalid/ach/0.jpg"),
            "2430d2aef49f657f679f1dca43414af3.jpg"
        );
    }

    #[test]
    fn cache_round_trips_and_ages() {
        let dir = std::env::temp_dir().join(format!("canape-ach-{}", std::process::id()));
        let path = dir.join("achievements.json");
        let mut cache = Cache::default();
        let list = parse_schema(&read("schema.json")).unwrap();
        cache.record("steam:900001", "english", Some(list), 1000);
        cache.record("steam:900002", "english", None, 1000);
        cache.save(&path).unwrap();
        let back = Cache::load(&path);
        assert_eq!(back.games, cache.games);
        assert!(back.fresh("steam:900001", "english", 1000 + FRESH_FOR - 1));
        assert!(!back.fresh("steam:900001", "english", 1000 + FRESH_FOR));
        assert!(!back.fresh("steam:900001", "french", 1001));
        assert!(!back.fresh("steam:1", "english", 1001));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icons_have_other_hosts() {
        let url = "https://steamcdn-a.akamaihd.net/steamcommunity/public/images/apps/1/a.jpg";
        let urls = icon_urls(url);
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], url);
        assert!(urls[1].starts_with("https://cdn.cloudflare.steamstatic.com/"));
        assert_eq!(icon_urls("https://example.com/a.jpg").len(), 1);
    }
}
