//! Time to beat from IGDB: how long players took to finish a game, with
//! the user's own Twitch application (client id and secret).
//!
//! A game is found once: by its store's id when IGDB links it
//! (`external_games`: Steam's app id, GOG's product id, Epic's ids), else
//! by its title (`games`): the exact title first (letters and digits
//! compared), else a close one, the same title with or without a subtitle
//! or an edition ("Fall Guys" and "Fall Guys: Ultimate Knockout", "Death
//! Stranding Director's Cut" and "Death Stranding"); main games first.
//! Its times (`game_time_to_beats`) are kept in a cache file per game id,
//! and so is a game IGDB does not know, for a shorter time.
//!
//! The credentials and the token only ever go to `id.twitch.tv` and
//! `api.igdb.com`; nothing here logs or formats them. Queries and answers
//! are built and read by pure functions, tested on recorded answers; the
//! transport is the [`Http`] trait.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::online::{Http, percent_encode};

pub const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
pub const API: &str = "https://api.igdb.com/v4";
/// Times found are asked for again after this many seconds (30 days).
pub const FOUND_FOR: u64 = 30 * 24 * 3600;
/// A game IGDB does not know, or has no times for, is looked up again
/// after this many seconds (7 days).
pub const MISSED_FOR: u64 = 7 * 24 * 3600;
/// A token this close to its expiry (seconds) is renewed first.
const TOKEN_MARGIN: u64 = 3600;
/// `external_game_sources` ids of Steam (uid: the app id), GOG (the
/// product id) and the Epic Games Store. What IGDB keeps as an Epic uid
/// is not documented: the lookup offers every id Canapé knows for the
/// game (Legendary's app name, the catalog item id, the namespace).
pub const STEAM_SOURCE: u64 = 1;
pub const GOG_SOURCE: u64 = 5;
pub const EPIC_SOURCE: u64 = 26;
/// `game_types` ids: a main game, then the kinds that stand for one
/// (standalone expansion, remake, remaster, expanded game, port).
const MAIN_GAME: u64 = 0;
const LIKE_MAIN: [u64; 5] = [4, 8, 9, 10, 11];
/// Version 2 finds more games than version 1: version 1's misses are
/// dropped on load, its times kept.
const CACHE_VERSION: u32 = 2;

/// The user's Twitch application. Not `Debug`: the secret must not end up
/// in a log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

impl Credentials {
    /// Both values, trimmed; `None` if either is empty.
    pub fn new(client_id: &str, client_secret: &str) -> Option<Self> {
        let (id, secret) = (client_id.trim(), client_secret.trim());
        (!id.is_empty() && !secret.is_empty()).then(|| Self {
            client_id: id.to_owned(),
            client_secret: secret.to_owned(),
        })
    }
}

/// An app access token, for the client id it was issued to. Not `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    /// Unix seconds.
    pub expires_at: u64,
    pub client_id: String,
}

impl Token {
    /// Usable now by `client_id`.
    pub fn valid(&self, client_id: &str, now: u64) -> bool {
        self.client_id == client_id && now + TOKEN_MARGIN < self.expires_at
    }

    /// The token saved earlier, if any.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write(
            path,
            &serde_json::to_string(self).map_err(io::Error::other)?,
        )
    }
}

/// Writes `text` to `path` through a temporary file, creating its folder.
pub(crate) fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Form body of the client-credentials request.
pub fn token_body(credentials: &Credentials) -> String {
    format!(
        "client_id={}&client_secret={}&grant_type=client_credentials",
        percent_encode(&credentials.client_id),
        percent_encode(&credentials.client_secret)
    )
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    expires_in: u64,
}

/// The token of a client-credentials answer, received at `now`.
pub fn parse_token(json: &[u8], client_id: &str, now: u64) -> Option<Token> {
    let answer: TokenAnswer = serde_json::from_slice(json).ok()?;
    (!answer.access_token.is_empty()).then(|| Token {
        access_token: answer.access_token,
        expires_at: now + answer.expires_in,
        client_id: client_id.to_owned(),
    })
}

/// An Apicalypse string literal.
fn quoted(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            // Trademark signs only blur the search.
            '™' | '®' | '©' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `external_games` query: the IGDB game of a store's id, one of `uids`
/// (in order of preference), in the store `source` (`STEAM_SOURCE`…).
pub fn external_games_query(source: u64, uids: &[String]) -> String {
    let uid = match uids {
        [one] => format!("uid = {}", quoted(one)),
        _ => format!(
            "uid = ({})",
            uids.iter().map(|u| quoted(u)).collect::<Vec<_>>().join(",")
        ),
    };
    format!(
        "fields game,uid,external_game_source; where {uid} & external_game_source = {source}; limit 5;"
    )
}

#[derive(Deserialize)]
struct External {
    #[serde(default)]
    game: Option<u64>,
    #[serde(default)]
    uid: String,
    #[serde(default)]
    external_game_source: Option<u64>,
}

/// The IGDB game id of an `external_games` answer for the store
/// `source`: the entry of the first of `uids` it has.
pub fn parse_external_games(json: &[u8], source: u64, uids: &[String]) -> Option<u64> {
    let found: Vec<External> = serde_json::from_slice(json).ok()?;
    let found: Vec<External> = found
        .into_iter()
        .filter(|e| e.external_game_source.is_none_or(|s| s == source))
        .filter(|e| e.game.is_some())
        .collect();
    uids.iter()
        .find_map(|uid| found.iter().find(|e| &e.uid == uid))
        .or(found.first())
        .and_then(|e| e.game)
}

/// The store and ids under which IGDB may list the game `id`, plus
/// `extra` ids its store gave (Epic's catalog ids): `None` for a game
/// of no linked store.
pub fn store_ids(id: &str, extra: &[String]) -> Option<(u64, Vec<String>)> {
    let (store, own) = id.split_once(':')?;
    let source = match store {
        "steam" => STEAM_SOURCE,
        "gog" => GOG_SOURCE,
        "epic" => EPIC_SOURCE,
        _ => return None,
    };
    let mut uids = vec![own.to_owned()];
    if source == EPIC_SOURCE {
        uids.extend(extra.iter().cloned());
    }
    let mut seen = std::collections::HashSet::new();
    uids.retain(|u| !u.is_empty() && seen.insert(u.clone()));
    (!uids.is_empty()).then_some((source, uids))
}

/// `games` query: games named like `title`.
pub fn search_query(title: &str) -> String {
    format!(
        "search {}; fields name,game_type,version_parent; limit 20;",
        quoted(title)
    )
}

#[derive(Deserialize)]
struct Found {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    game_type: Option<u64>,
    #[serde(default)]
    version_parent: Option<u64>,
}

impl Found {
    /// Lower is better; `None` for DLC, bundles, mods, episodes…
    fn rank(&self) -> Option<u8> {
        let kind = match self.game_type {
            None | Some(MAIN_GAME) => 0,
            Some(t) if LIKE_MAIN.contains(&t) => 1,
            Some(_) => return None,
        };
        // An edition of another game (`version_parent`) comes after it.
        Some(kind * 2 + u8::from(self.version_parent.is_some()))
    }
}

/// Editions a store adds to a game's title, compared folded.
const EDITIONS: [&str; 18] = [
    "Director's Cut",
    "Final Cut",
    "Game of the Year Edition",
    "Game of the Year",
    "GOTY Edition",
    "GOTY",
    "Definitive Edition",
    "Complete Edition",
    "Digital Deluxe Edition",
    "Deluxe Edition",
    "Ultimate Edition",
    "Standard Edition",
    "Enhanced Edition",
    "Special Edition",
    "Gold Edition",
    "Anniversary Edition",
    "Premium Edition",
    "Legendary Edition",
];

/// `title` without its subtitle (after `:` or a dash) and without an
/// edition at its end: "Fall Guys: Ultimate Knockout" and "Death
/// Stranding Director's Cut" give "Fall Guys" and "Death Stranding".
pub fn base_title(title: &str) -> String {
    let mut text = title.trim();
    for sep in [":", " - ", " – ", " — "] {
        if let Some(at) = text.find(sep).filter(|&at| at > 0) {
            text = text[..at].trim_end();
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    for edition in EDITIONS {
        let key = crate::title_key(edition);
        for n in 1..words.len() {
            if crate::title_key(&words[words.len() - n..].join(" ")) == key {
                let rest = words[..words.len() - n].join(" ");
                return rest
                    .trim_end_matches(|c: char| !c.is_alphanumeric())
                    .to_owned();
            }
        }
    }
    text.to_owned()
}

/// How well an IGDB name matches a store title: 0 the same, 1 the same
/// but for a subtitle or an edition on one side; `None` otherwise.
fn closeness(name: &str, title: &str) -> Option<u8> {
    let (name_key, title_key) = (crate::title_key(name), crate::title_key(title));
    if name_key.is_empty() {
        return None;
    }
    if name_key == title_key {
        return Some(0);
    }
    (crate::title_key(&base_title(name)) == title_key
        || name_key == crate::title_key(&base_title(title)))
    .then_some(1)
}

/// The game of a `games` answer named like `title` (letters and digits
/// compared, see [`crate::title_key`]): the same name first, else a close
/// one (see [`base_title`]); main games first, then IGDB's order. `None`
/// when no name is close.
pub fn parse_search(json: &[u8], title: &str) -> Option<u64> {
    let found: Vec<Found> = serde_json::from_slice(json).ok()?;
    found
        .iter()
        .enumerate()
        .filter_map(|(order, f)| Some(((closeness(&f.name, title)?, f.rank()?, order), f.id)))
        .min_by_key(|(key, _)| *key)
        .map(|(_, id)| id)
}

/// `game_time_to_beats` query for an IGDB game id.
pub fn time_to_beat_query(game: u64) -> String {
    format!("fields game_id,hastily,normally,completely,count; where game_id = {game}; limit 1;")
}

/// How long players took, in seconds, and how many answered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeToBeat {
    #[serde(default)]
    pub hastily: Option<u64>,
    #[serde(default)]
    pub normally: Option<u64>,
    #[serde(default)]
    pub completely: Option<u64>,
    #[serde(default)]
    pub count: u64,
}

impl TimeToBeat {
    /// The main story, in seconds: the usual time, else the fastest one.
    pub fn main_story(&self) -> Option<u64> {
        self.normally
            .filter(|s| *s > 0)
            .or(self.hastily.filter(|s| *s > 0))
    }
}

/// The times of a `game_time_to_beats` answer, if it has any.
pub fn parse_time_to_beat(json: &[u8]) -> Option<TimeToBeat> {
    let found: Vec<TimeToBeat> = serde_json::from_slice(json).ok()?;
    found.into_iter().find(|t| t.main_story().is_some())
}

/// IGDB queries with a token, asked for when missing or expired, and once
/// again when IGDB refuses it.
pub struct Client<'a> {
    http: &'a dyn Http,
    credentials: &'a Credentials,
    /// The token in use; read it back after a lookup to keep it.
    pub token: Option<Token>,
    /// The IGDB game the last lookup settled on, if any (for the log).
    pub found: Option<u64>,
}

impl<'a> Client<'a> {
    pub fn new(http: &'a dyn Http, credentials: &'a Credentials, token: Option<Token>) -> Self {
        Self {
            http,
            credentials,
            token,
            found: None,
        }
    }

    fn renew(&mut self, now: u64) -> io::Result<String> {
        let json = self.http.post(
            TOKEN_URL,
            &[("Content-Type", "application/x-www-form-urlencoded")],
            token_body(self.credentials).as_bytes(),
        )?;
        let token = parse_token(&json, &self.credentials.client_id, now)
            .ok_or_else(|| io::Error::other("unreadable token answer"))?;
        let access = token.access_token.clone();
        self.token = Some(token);
        Ok(access)
    }

    fn post(&self, endpoint: &str, access: &str, query: &str) -> io::Result<Vec<u8>> {
        let bearer = format!("Bearer {access}");
        self.http.post(
            &format!("{API}/{endpoint}"),
            &[
                ("Client-ID", self.credentials.client_id.as_str()),
                ("Authorization", bearer.as_str()),
                ("Content-Type", "text/plain"),
            ],
            query.as_bytes(),
        )
    }

    /// The answer of IGDB's `endpoint` to `query`.
    pub fn query(&mut self, endpoint: &str, query: &str, now: u64) -> io::Result<Vec<u8>> {
        let cached = self
            .token
            .as_ref()
            .filter(|t| t.valid(&self.credentials.client_id, now))
            .map(|t| t.access_token.clone());
        let access = match cached {
            Some(access) => access,
            None => self.renew(now)?,
        };
        match self.post(endpoint, &access, query) {
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                let access = self.renew(now)?;
                self.post(endpoint, &access, query)
            }
            other => other,
        }
    }

    /// The times of the game `id` (`steam:400`, `gog:1`…) titled `title`;
    /// `None` when IGDB does not know it or has no times for it. `extra`:
    /// other ids its store gave (Epic's catalog item id and namespace).
    pub fn lookup(
        &mut self,
        id: &str,
        title: &str,
        extra: &[String],
        now: u64,
    ) -> io::Result<Option<TimeToBeat>> {
        self.found = None;
        let mut game = None;
        if let Some((source, uids)) = store_ids(id, extra) {
            game = match self.query("external_games", &external_games_query(source, &uids), now) {
                Ok(json) => parse_external_games(&json, source, &uids),
                Err(err) if err.kind() == io::ErrorKind::PermissionDenied => return Err(err),
                // A query IGDB rejects still leaves the title.
                Err(_) => None,
            };
        }
        if game.is_none() {
            let json = self.query("games", &search_query(title), now)?;
            game = parse_search(&json, title);
        }
        // IGDB's search may not list the game under a store's longer
        // title: once more with the title alone.
        let base = base_title(title);
        if game.is_none() && crate::title_key(&base) != crate::title_key(title) && !base.is_empty()
        {
            let json = self.query("games", &search_query(&base), now)?;
            game = parse_search(&json, title);
        }
        self.found = game;
        let Some(game) = game else { return Ok(None) };
        let json = self.query("game_time_to_beats", &time_to_beat_query(game), now)?;
        Ok(parse_time_to_beat(&json))
    }
}

/// One game looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub at: u64,
    /// `None`: IGDB has no times for it.
    #[serde(default)]
    pub times: Option<TimeToBeat>,
}

impl Entry {
    pub fn fresh(&self, now: u64) -> bool {
        let keep = if self.times.is_some() {
            FOUND_FOR
        } else {
            MISSED_FOR
        };
        now < self.at + keep
    }
}

/// Times looked up, by Canapé game id: `%LOCALAPPDATA%\Canape\igdb.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    pub games: HashMap<String, Entry>,
}

impl Cache {
    /// The saved cache; empty when missing, unreadable or of another version.
    pub fn load(path: &Path) -> Self {
        let mut cache = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .filter(|c| c.version == CACHE_VERSION || c.version == 1)
            .unwrap_or_default();
        if cache.version == 1 {
            // Found by the lookup of version 1: kept. Missed: asked again.
            cache.games.retain(|_, e| e.times.is_some());
            cache.version = CACHE_VERSION;
        }
        cache
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let file = Self {
            version: CACHE_VERSION,
            games: self.games.clone(),
        };
        write(
            path,
            &serde_json::to_string(&file).map_err(io::Error::other)?,
        )
    }

    /// The entry of `id` if it is still fresh.
    pub fn fresh(&self, id: &str, now: u64) -> Option<&Entry> {
        self.games.get(id).filter(|e| e.fresh(now))
    }

    pub fn record(&mut self, id: &str, times: Option<TimeToBeat>, now: u64) {
        self.games.insert(id.to_owned(), Entry { at: now, times });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;
    use std::cell::RefCell;

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(&format!("igdb/{name}"))).unwrap()
    }

    fn credentials() -> Credentials {
        Credentials::new(" abc123 ", "s3cr&t").unwrap()
    }

    #[test]
    fn credentials_need_both_values() {
        assert!(Credentials::new("id", " ").is_none());
        assert!(Credentials::new("", "secret").is_none());
        assert_eq!(credentials().client_id, "abc123");
    }

    #[test]
    fn token_request_and_answer() {
        assert_eq!(
            token_body(&credentials()),
            "client_id=abc123&client_secret=s3cr%26t&grant_type=client_credentials"
        );
        let token = parse_token(&read("token.json"), "abc123", 1000).unwrap();
        assert_eq!(token.access_token, "fixturetoken0123456789");
        assert_eq!(token.expires_at, 1000 + 5_184_000);
        assert!(token.valid("abc123", 1000));
        assert!(!token.valid("other", 1000));
        assert!(!token.valid("abc123", token.expires_at - 60));
        assert!(parse_token(br#"{"status":400,"message":"invalid client"}"#, "a", 0).is_none());
    }

    #[test]
    fn queries_are_apicalypse() {
        assert_eq!(
            external_games_query(STEAM_SOURCE, &["1245620".into()]),
            "fields game,uid,external_game_source; where uid = \"1245620\" & external_game_source = 1; limit 5;"
        );
        assert_eq!(
            external_games_query(EPIC_SOURCE, &["Kiwi".into(), "a\"b".into()]),
            "fields game,uid,external_game_source; where uid = (\"Kiwi\",\"a\\\"b\") & external_game_source = 26; limit 5;"
        );
        assert_eq!(
            search_query("Say \"Hi\"\\ DOOM™"),
            "search \"Say \\\"Hi\\\"\\\\ DOOM\"; fields name,game_type,version_parent; limit 20;"
        );
        assert_eq!(
            time_to_beat_query(119_133),
            "fields game_id,hastily,normally,completely,count; where game_id = 119133; limit 1;"
        );
    }

    #[test]
    fn external_games_give_the_steam_entry() {
        let steam = ["1245620".to_owned()];
        assert_eq!(
            parse_external_games(&read("external-games.json"), STEAM_SOURCE, &steam),
            Some(119_133)
        );
        assert_eq!(
            parse_external_games(
                br#"[{"id":1,"game":5,"external_game_source":5}]"#,
                STEAM_SOURCE,
                &steam
            ),
            None
        );
        assert_eq!(parse_external_games(b"[]", STEAM_SOURCE, &steam), None);
        assert_eq!(parse_external_games(b"{}", STEAM_SOURCE, &steam), None);
        // Epic: the entry of the preferred id wins.
        let epic = ["app".to_owned(), "item".to_owned(), "ns".to_owned()];
        let json = br#"[{"id":1,"game":7,"uid":"ns","external_game_source":26},
            {"id":2,"game":8,"uid":"item","external_game_source":26}]"#;
        assert_eq!(parse_external_games(json, EPIC_SOURCE, &epic), Some(8));
    }

    #[test]
    fn store_ids_name_the_source() {
        assert_eq!(
            store_ids("steam:400", &["x".into()]),
            Some((STEAM_SOURCE, vec!["400".to_owned()]))
        );
        assert_eq!(
            store_ids("gog:1207658924", &[]),
            Some((GOG_SOURCE, vec!["1207658924".to_owned()]))
        );
        assert_eq!(
            store_ids("epic:Kiwi", &["item".into(), "Kiwi".into(), "ns".into()]),
            Some((
                EPIC_SOURCE,
                vec!["Kiwi".to_owned(), "item".to_owned(), "ns".to_owned()]
            ))
        );
        assert_eq!(store_ids("xbox:Foo", &[]), None);
        assert_eq!(store_ids("other:1", &[]), None);
    }

    #[test]
    fn base_titles_drop_subtitles_and_editions() {
        assert_eq!(base_title("Fall Guys: Ultimate Knockout"), "Fall Guys");
        assert_eq!(
            base_title("DEATH STRANDING DIRECTOR'S CUT"),
            "DEATH STRANDING"
        );
        assert_eq!(
            base_title("The Witcher 3: Wild Hunt - GOTY"),
            "The Witcher 3"
        );
        assert_eq!(base_title("Control Ultimate Edition"), "Control");
        assert_eq!(
            base_title("Borderlands 3 - Deluxe Edition"),
            "Borderlands 3"
        );
        assert_eq!(base_title("Hades II"), "Hades II");
        // An edition alone stays a title.
        assert_eq!(base_title("GOTY"), "GOTY");
        assert_eq!(base_title(": Odd"), ": Odd");
    }

    #[test]
    fn search_accepts_a_close_main_game() {
        let json = br#"[
            {"id": 1, "name": "Fall Guys: Ultimate Knockout - Season 2", "game_type": 7},
            {"id": 2, "name": "Fall Guys: Ultimate Knockout", "game_type": 0},
            {"id": 3, "name": "Fall Guys 2", "game_type": 0}]"#;
        assert_eq!(parse_search(json, "Fall Guys"), Some(2));
        assert_eq!(parse_search(json, "Fall Guys™"), Some(2));
        let json = br#"[
            {"id": 10, "name": "Death Stranding", "game_type": 0},
            {"id": 11, "name": "Death Stranding: Director's Cut", "game_type": 0, "version_parent": 10},
            {"id": 12, "name": "Death Stranding 2: On the Beach", "game_type": 0}]"#;
        // The same name beats a close one.
        assert_eq!(
            parse_search(json, "DEATH STRANDING DIRECTOR'S CUT"),
            Some(11)
        );
        assert_eq!(
            parse_search(json, "Death Stranding Director's Cut"),
            Some(11)
        );
        assert_eq!(parse_search(json, "Death Stranding"), Some(10));
        assert_eq!(
            parse_search(json, "Death Stranding Deluxe Edition"),
            Some(10)
        );
        // A sequel is not close.
        assert_eq!(
            parse_search(br#"[{"id": 3, "name": "Hades II"}]"#, "Hades"),
            None
        );
    }

    #[test]
    fn search_takes_the_main_game_of_that_title() {
        let json = read("games-search.json");
        // DLC and bundles of the same name lose to the game; an edition
        // (version_parent) to its original.
        assert_eq!(parse_search(&json, "Elden Ring"), Some(119_133));
        assert_eq!(parse_search(&json, "ELDEN RING™"), Some(119_133));
        assert_eq!(
            parse_search(&json, "Elden Ring Shadow of the Erdtree"),
            None
        );
        assert_eq!(parse_search(&json, "Something else"), None);
        let remaster =
            br#"[{"id":2,"name":"Halo","game_type":9},{"id":3,"name":"Halo","game_type":3}]"#;
        assert_eq!(parse_search(remaster, "Halo"), Some(2));
    }

    #[test]
    fn times_use_the_usual_then_the_fastest() {
        let times = parse_time_to_beat(&read("time-to-beat.json")).unwrap();
        assert_eq!(times.normally, Some(216_000));
        assert_eq!(times.main_story(), Some(216_000));
        assert_eq!(times.count, 412);
        let hasty = parse_time_to_beat(br#"[{"id":1,"game_id":2,"hastily":7200,"count":3}]"#);
        assert_eq!(hasty.unwrap().main_story(), Some(7200));
        assert_eq!(
            parse_time_to_beat(br#"[{"id":1,"game_id":2,"count":0}]"#),
            None
        );
        assert_eq!(parse_time_to_beat(b"[]"), None);
    }

    /// A request: URL, headers, body.
    type Asked = (String, Vec<(String, String)>, String);

    /// Answers by URL and request body; records what was asked, with which
    /// headers.
    struct Fake {
        answers: HashMap<(String, String), Vec<u8>>,
        /// The first IGDB request is refused with a 401 this many times.
        refuse: RefCell<u32>,
        asked: RefCell<Vec<Asked>>,
    }

    impl Fake {
        fn new(answers: &[(&str, &str, Vec<u8>)]) -> Self {
            Self {
                answers: answers
                    .iter()
                    .map(|(url, body, json)| ((url.to_string(), body.to_string()), json.clone()))
                    .collect(),
                refuse: RefCell::new(0),
                asked: RefCell::new(Vec::new()),
            }
        }
    }

    impl Http for Fake {
        fn get_limited(&self, url: &str, _: Option<&str>, _: usize) -> io::Result<Vec<u8>> {
            panic!("no GET expected: {url}")
        }

        fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> io::Result<Vec<u8>> {
            let body = String::from_utf8(body.to_vec()).unwrap();
            self.asked.borrow_mut().push((
                url.to_owned(),
                headers
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.to_string()))
                    .collect(),
                body.clone(),
            ));
            if url.starts_with(API) && *self.refuse.borrow() > 0 {
                *self.refuse.borrow_mut() -= 1;
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "HTTP 401"));
            }
            self.answers
                .get(&(url.to_owned(), body))
                .cloned()
                .ok_or_else(|| io::Error::other("HTTP 400"))
        }
    }

    fn igdb(endpoint: &str) -> String {
        format!("{API}/{endpoint}")
    }

    #[test]
    fn steam_games_are_found_by_app_id() {
        let creds = credentials();
        let token_body = token_body(&creds);
        let http = Fake::new(&[
            (TOKEN_URL, &token_body, read("token.json")),
            (
                &igdb("external_games"),
                &external_games_query(STEAM_SOURCE, &["1245620".into()]),
                read("external-games.json"),
            ),
            (
                &igdb("game_time_to_beats"),
                &time_to_beat_query(119_133),
                read("time-to-beat.json"),
            ),
        ]);
        let mut client = Client::new(&http, &creds, None);
        let times = client
            .lookup("steam:1245620", "ELDEN RING", &[], 1000)
            .unwrap();
        assert_eq!(times.unwrap().main_story(), Some(216_000));
        let asked = http.asked.borrow();
        assert_eq!(asked.len(), 3);
        // The secret goes to Twitch only, in the body; IGDB gets the id
        // and the token.
        assert_eq!(asked[0].0, TOKEN_URL);
        for (url, headers, body) in &asked[1..] {
            assert!(url.starts_with(API));
            assert!(!body.contains("s3cr") && headers.iter().all(|(_, v)| !v.contains("s3cr")));
            assert!(headers.contains(&("Client-ID".into(), "abc123".into())));
            assert!(headers.contains(&(
                "Authorization".into(),
                "Bearer fixturetoken0123456789".into()
            )));
        }
        // The token is kept for the next lookup.
        assert!(client.token.as_ref().unwrap().valid("abc123", 1000));
    }

    #[test]
    fn other_games_are_found_by_title_and_misses_are_none() {
        let creds = credentials();
        let token = parse_token(&read("token.json"), "abc123", 0);
        let http = Fake::new(&[
            (
                &igdb("games"),
                &search_query("Elden Ring"),
                read("games-search.json"),
            ),
            (
                &igdb("game_time_to_beats"),
                &time_to_beat_query(119_133),
                br#"[]"#.to_vec(),
            ),
            (&igdb("games"), &search_query("Nowhere"), br#"[]"#.to_vec()),
        ]);
        let mut client = Client::new(&http, &creds, token);
        // Known, but no times yet.
        assert_eq!(
            client.lookup("gog:1", "Elden Ring", &[], 1000).unwrap(),
            None
        );
        // Unknown: no time-to-beat query at all.
        assert_eq!(client.lookup("epic:x", "Nowhere", &[], 1000).unwrap(), None);
        let asked = http.asked.borrow();
        // Each store id first (refused here), then the title.
        let endpoints: Vec<&str> = asked
            .iter()
            .map(|(url, _, _)| url.trim_start_matches(API))
            .collect();
        assert_eq!(
            endpoints,
            [
                "/external_games",
                "/games",
                "/game_time_to_beats",
                "/external_games",
                "/games"
            ]
        );
    }

    #[test]
    fn epic_games_are_found_by_their_ids_or_a_shorter_title() {
        let creds = credentials();
        let token = parse_token(&read("token.json"), "abc123", 0);
        let times = br#"[{"id":1,"game_id":47823,"normally":36000,"count":5}]"#.to_vec();
        let ids = ["item".to_owned(), "ns".to_owned()];
        let uids = ["Kiwi".to_owned(), "item".to_owned(), "ns".to_owned()];
        let http = Fake::new(&[
            (
                &igdb("external_games"),
                &external_games_query(EPIC_SOURCE, &uids),
                br#"[{"id":9,"game":47823,"uid":"item","external_game_source":26}]"#.to_vec(),
            ),
            (
                &igdb("game_time_to_beats"),
                &time_to_beat_query(47_823),
                times.clone(),
            ),
            // Not linked: found by title, once with the edition dropped.
            (
                &igdb("external_games"),
                &external_games_query(EPIC_SOURCE, &["Other".to_owned()]),
                b"[]".to_vec(),
            ),
            (
                &igdb("games"),
                &search_query("Fall Guys Deluxe Edition"),
                b"[]".to_vec(),
            ),
            (
                &igdb("games"),
                &search_query("Fall Guys"),
                br#"[{"id": 47823, "name": "Fall Guys", "game_type": 0}]"#.to_vec(),
            ),
        ]);
        let mut client = Client::new(&http, &creds, token);
        let found = client.lookup("epic:Kiwi", "Fall Guys", &ids, 1000).unwrap();
        assert_eq!(found.unwrap().main_story(), Some(36_000));
        assert_eq!(http.asked.borrow().len(), 2);
        let found = client
            .lookup("epic:Other", "Fall Guys Deluxe Edition", &[], 1000)
            .unwrap();
        assert_eq!(found.unwrap().main_story(), Some(36_000));
    }

    #[test]
    fn a_refused_token_is_renewed_once() {
        let creds = credentials();
        let old = Token {
            access_token: "old".into(),
            expires_at: u64::MAX,
            client_id: "abc123".into(),
        };
        let token_body = token_body(&creds);
        let http = Fake::new(&[
            (TOKEN_URL, &token_body, read("token.json")),
            (&igdb("games"), &search_query("Nowhere"), br#"[]"#.to_vec()),
        ]);
        *http.refuse.borrow_mut() = 1;
        let mut client = Client::new(&http, &creds, Some(old));
        assert_eq!(client.lookup("gog:2", "Nowhere", &[], 1000).unwrap(), None);
        assert_eq!(
            client.token.as_ref().unwrap().access_token,
            "fixturetoken0123456789"
        );
        // Refused again after renewing: an error, no loop.
        *http.refuse.borrow_mut() = 2;
        let err = client.lookup("gog:2", "Nowhere", &[], 1000).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn cache_keeps_hits_longer_than_misses() {
        let dir = std::env::temp_dir().join(format!("canape-igdb-{}", std::process::id()));
        let path = dir.join("igdb.json");
        let mut cache = Cache::default();
        let times = TimeToBeat {
            normally: Some(3600),
            ..Default::default()
        };
        cache.record("steam:1", Some(times), 0);
        cache.record("gog:2", None, 0);
        cache.save(&path).unwrap();
        let cache = Cache::load(&path);
        assert_eq!(
            cache.fresh("steam:1", MISSED_FOR + 1).unwrap().times,
            Some(times)
        );
        assert!(cache.fresh("gog:2", MISSED_FOR - 1).is_some());
        assert!(cache.fresh("gog:2", MISSED_FOR + 1).is_none());
        assert!(cache.fresh("steam:1", FOUND_FOR + 1).is_none());
        std::fs::write(&path, r#"{"version":99,"games":{}}"#).unwrap();
        assert!(Cache::load(&path).games.is_empty());
        // A version 1 cache keeps its times and forgets its misses.
        std::fs::write(
            &path,
            r#"{"version":1,"games":{"steam:1":{"at":0,"times":{"normally":3600,"count":1}},"epic:x":{"at":0,"times":null}}}"#,
        )
        .unwrap();
        let old = Cache::load(&path);
        assert!(old.fresh("steam:1", 1).is_some());
        assert!(!old.games.contains_key("epic:x"));

        let token_path = dir.join("igdb-token.json");
        let token = parse_token(&read("token.json"), "abc123", 0).unwrap();
        token.save(&token_path).unwrap();
        assert!(Token::load(&token_path) == Some(token));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
