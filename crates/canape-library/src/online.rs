//! Cover art downloaded for games that have none on disk: Steam's public
//! store CDN for Steam games, and SteamGridDB (with the user's own API key)
//! for every store.
//!
//! Covers land in one folder, named after the game id, and are reused by
//! every later scan. A game without art anywhere gets an empty `.miss`
//! marker, so it is asked for again only after [`RETRY_AFTER`].
//!
//! URL building and response parsing are pure and tested; the transport is
//! the [`Http`] trait, implemented with WinHTTP on Windows.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::{Game, Source};

const GRIDDB_API: &str = "https://www.steamgriddb.com/api/v2";
/// Portrait grids, the shape of the library's tiles.
const GRIDDB_FILTER: &str = "dimensions=600x900,342x482,660x930&mimes=image/jpeg,image/png&types=static&nsfw=false&humor=false";
/// A game with no art anywhere is looked up again after this.
pub const RETRY_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
/// Downloads per scan, to stay gentle with the services.
pub const MAX_PER_SCAN: usize = 60;
/// Largest image accepted.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Where covers may come from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Steam's store CDN, for Steam games (no account needed).
    pub steam_cdn: bool,
    /// SteamGridDB API key, from the user's SteamGridDB preferences.
    pub griddb_key: Option<String>,
}

impl Settings {
    pub fn enabled(&self) -> bool {
        self.steam_cdn || self.griddb_key.is_some()
    }
}

/// HTTPS requests.
pub trait Http {
    /// The body of a 200 answer, at most `max` bytes; an error for any
    /// other status or a longer body.
    fn get_limited(&self, url: &str, bearer: Option<&str>, max: usize) -> io::Result<Vec<u8>>;

    /// [`Http::get_limited`] for images and API answers.
    fn get(&self, url: &str, bearer: Option<&str>) -> io::Result<Vec<u8>> {
        self.get_limited(url, bearer, MAX_BYTES)
    }

    /// A POST of `body` with extra `headers` (name, value): the body of a
    /// 200 answer, at most [`MAX_BYTES`]. A 401 is a `PermissionDenied`
    /// error, any other status an error too.
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> io::Result<Vec<u8>>;
}

/// File name stem for a game id: `steam:1245620` -> `steam_1245620`.
fn stem(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn steam_appid(game: &Game) -> Option<u32> {
    (game.source == Source::Steam)
        .then(|| game.id.strip_prefix("steam:")?.parse().ok())
        .flatten()
}

/// Steam's store CDN, newest layout first.
pub fn steam_cdn_urls(appid: u32) -> [String; 2] {
    [
        format!(
            "https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/library_600x900.jpg"
        ),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"),
    ]
}

pub fn griddb_steam_url(appid: u32) -> String {
    format!("{GRIDDB_API}/grids/steam/{appid}?{GRIDDB_FILTER}")
}

pub fn griddb_game_url(game_id: u64) -> String {
    format!("{GRIDDB_API}/grids/game/{game_id}?{GRIDDB_FILTER}")
}

pub fn griddb_search_url(title: &str) -> String {
    format!("{GRIDDB_API}/search/autocomplete/{}", percent_encode(title))
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
pub(crate) fn percent_encode(text: &str) -> String {
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

#[derive(Deserialize)]
struct Answer<T> {
    #[serde(default)]
    success: bool,
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct Grid {
    url: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
}

#[derive(Deserialize)]
struct Found {
    id: u64,
    name: String,
}

/// The first portrait grid of a `grids` answer.
pub fn parse_grids(json: &[u8]) -> Option<String> {
    let answer: Answer<Grid> = serde_json::from_slice(json).ok()?;
    answer
        .success
        .then(|| {
            answer
                .data
                .into_iter()
                .find(|g| g.height > g.width && g.url.starts_with("https://"))
                .map(|g| g.url)
        })
        .flatten()
}

/// The SteamGridDB game whose name matches `title`, else the first result.
pub fn parse_search(json: &[u8], title: &str) -> Option<u64> {
    let answer: Answer<Found> = serde_json::from_slice(json).ok()?;
    if !answer.success {
        return None;
    }
    let key = crate::title_key(title);
    answer
        .data
        .iter()
        .find(|f| crate::title_key(&f.name) == key)
        .or(answer.data.first())
        .map(|f| f.id)
}

/// JPEG or PNG bytes, with the extension to save them under.
fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else {
        None
    }
}

/// A cover already downloaded for `id`.
pub fn cached(dir: &Path, id: &str) -> Option<PathBuf> {
    let stem = stem(id);
    ["jpg", "png"]
        .into_iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|path| path.is_file())
}

/// No cover, or only a landscape one from Steam's cache.
fn wants_art(game: &Game) -> bool {
    game.cover.as_deref().is_none_or(crate::steam::landscape)
}

/// Gives games without a portrait cover the one downloaded earlier, if any.
pub fn attach(games: &mut [Game], dir: &Path) {
    for game in games.iter_mut().filter(|g| wants_art(g)) {
        if let Some(path) = cached(dir, &game.id) {
            game.cover = Some(path);
        }
    }
}

fn missed_recently(dir: &Path, id: &str, now: SystemTime) -> bool {
    fs::metadata(dir.join(format!("{}.miss", stem(id))))
        .and_then(|m| m.modified())
        .is_ok_and(|t| now.duration_since(t).unwrap_or_default() < RETRY_AFTER)
}

/// The image bytes of the first source that has art for `game`: its own
/// art link if the store gave one (`art`), Steam's CDN, then SteamGridDB.
fn find(game: &Game, art: Option<&str>, settings: &Settings, http: &dyn Http) -> Option<Vec<u8>> {
    if let Some(bytes) = art.and_then(|url| http.get(url, None).ok()) {
        return Some(bytes);
    }
    let appid = steam_appid(game);
    if settings.steam_cdn
        && let Some(appid) = appid
    {
        for url in steam_cdn_urls(appid) {
            if let Ok(bytes) = http.get(&url, None) {
                return Some(bytes);
            }
        }
    }
    let key = settings.griddb_key.as_deref()?;
    let grids = match appid {
        Some(appid) => http.get(&griddb_steam_url(appid), Some(key)).ok(),
        None => {
            let found = http.get(&griddb_search_url(&game.title), Some(key)).ok()?;
            let id = parse_search(&found, &game.title)?;
            http.get(&griddb_game_url(id), Some(key)).ok()
        }
    }?;
    // CDN links need no key.
    http.get(&parse_grids(&grids)?, None).ok()
}

/// Wide art downloaded earlier for `id` (the game page's background).
pub fn cached_wide(dir: &Path, id: &str) -> Option<PathBuf> {
    let stem = stem(id);
    ["jpg", "png"]
        .into_iter()
        .map(|ext| dir.join(format!("{stem}.wide.{ext}")))
        .find(|path| path.is_file())
}

/// The wide art of `id`: downloaded earlier, else from `url` (a link its
/// store gave) into `dir`, once: a failed download is tried again only
/// after [`RETRY_AFTER`].
pub fn wide_art(dir: &Path, id: &str, url: Option<&str>, http: &dyn Http) -> Option<PathBuf> {
    if let Some(path) = cached_wide(dir, id) {
        return Some(path);
    }
    let url = url?;
    let stem = stem(id);
    // Its own miss marker, beside the cover's.
    let miss_id = format!("{id}:wide");
    if missed_recently(dir, &miss_id, SystemTime::now()) {
        return None;
    }
    fs::create_dir_all(dir).ok()?;
    let image = http
        .get(url, None)
        .ok()
        .filter(|b| b.len() <= MAX_BYTES)
        .and_then(|b| Some((image_extension(&b)?, b)));
    let Some((ext, bytes)) = image else {
        let _ = fs::write(dir.join(format!("{}.miss", self::stem(&miss_id))), b"");
        return None;
    };
    let part = dir.join(format!("{stem}.wide.part"));
    fs::write(&part, &bytes).ok()?;
    let path = dir.join(format!("{stem}.wide.{ext}"));
    fs::rename(&part, &path).ok()?;
    Some(path)
}

/// Downloads the covers of games that have none (or only a landscape one), at most
/// [`MAX_PER_SCAN`]. `art` maps game ids to art links their store gave
/// (Epic). Returns the ids that got one.
pub fn download_missing(
    games: &[Game],
    art: &HashMap<String, String>,
    dir: &Path,
    settings: &Settings,
    http: &dyn Http,
) -> io::Result<Vec<String>> {
    let mut got = Vec::new();
    if !settings.enabled() {
        return Ok(got);
    }
    fs::create_dir_all(dir)?;
    let now = SystemTime::now();
    let wanted = games
        .iter()
        .filter(|g| wants_art(g) && cached(dir, &g.id).is_none())
        // A store's own link is worth a try even after the other sources
        // had nothing.
        .filter(|g| art.contains_key(&g.id) || !missed_recently(dir, &g.id, now))
        .take(MAX_PER_SCAN);
    for game in wanted {
        let stem = stem(&game.id);
        let link = art.get(&game.id).map(String::as_str);
        let image = find(game, link, settings, http)
            .filter(|b| b.len() <= MAX_BYTES)
            .and_then(|b| Some((image_extension(&b)?, b)));
        match image {
            Some((ext, bytes)) => {
                // Written aside first: a cut-off download is never shown.
                let part = dir.join(format!("{stem}.part"));
                fs::write(&part, &bytes)?;
                fs::rename(&part, dir.join(format!("{stem}.{ext}")))?;
                let _ = fs::remove_file(dir.join(format!("{stem}.miss")));
                got.push(game.id.clone());
            }
            None => fs::write(dir.join(format!("{stem}.miss")), b"")?,
        }
    }
    Ok(got)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use std::cell::RefCell;

    struct Fake {
        answers: HashMap<String, Vec<u8>>,
        asked: RefCell<Vec<(String, bool)>>,
    }

    impl Http for Fake {
        fn get_limited(&self, url: &str, bearer: Option<&str>, _: usize) -> io::Result<Vec<u8>> {
            self.asked
                .borrow_mut()
                .push((url.to_owned(), bearer.is_some()));
            self.answers
                .get(url)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "404"))
        }

        fn post(&self, url: &str, _: &[(&str, &str)], _: &[u8]) -> io::Result<Vec<u8>> {
            panic!("no POST expected: {url}")
        }
    }

    fn game(id: &str, title: &str, source: Source) -> Game {
        Game {
            id: id.into(),
            title: title.into(),
            source,
            cover: None,
            launch: Launch::Uri { uri: String::new() },
            last_played: None,
            size: None,
            playtime: None,
        }
    }

    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("canape-online-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn urls_are_built_and_encoded() {
        assert!(steam_cdn_urls(1245620)[0].ends_with("/apps/1245620/library_600x900.jpg"));
        assert!(
            griddb_steam_url(400)
                .starts_with("https://www.steamgriddb.com/api/v2/grids/steam/400?")
        );
        assert_eq!(
            griddb_search_url("Hades II: Été/Ω"),
            "https://www.steamgriddb.com/api/v2/search/autocomplete/Hades%20II%3A%20%C3%89t%C3%A9%2F%CE%A9"
        );
        assert_eq!(stem("epic:ns:item"), "epic_ns_item");
    }

    #[test]
    fn grids_pick_the_first_portrait() {
        let json = br#"{"success":true,"data":[
            {"id":1,"url":"https://cdn2.steamgriddb.com/grid/wide.png","width":920,"height":430},
            {"id":2,"url":"https://cdn2.steamgriddb.com/grid/tall.png","width":600,"height":900}]}"#;
        assert_eq!(
            parse_grids(json).as_deref(),
            Some("https://cdn2.steamgriddb.com/grid/tall.png")
        );
        assert_eq!(parse_grids(br#"{"success":false,"errors":["x"]}"#), None);
        assert_eq!(parse_grids(b"not json"), None);
    }

    #[test]
    fn search_prefers_the_exact_title() {
        let json = r#"{"success":true,"data":[
            {"id":10,"name":"Hades II"},{"id":11,"name":"HADES™"}]}"#
            .as_bytes();
        assert_eq!(parse_search(json, "Hades"), Some(11));
        assert_eq!(parse_search(json, "Something else"), Some(10));
        assert_eq!(
            parse_search(br#"{"success":true,"data":[]}"#, "Hades"),
            None
        );
    }

    #[test]
    fn steam_games_use_the_cdn_others_griddb() {
        let dir = temp_dir("download");
        let grids = br#"{"success":true,"data":[{"url":"https://cdn2.steamgriddb.com/grid/q.png","width":600,"height":900}]}"#;
        let mut answers = HashMap::new();
        answers.insert(steam_cdn_urls(400)[1].clone(), JPEG.to_vec());
        answers.insert(
            griddb_search_url("Fabrikam Quest"),
            br#"{"success":true,"data":[{"id":7,"name":"Fabrikam Quest"}]}"#.to_vec(),
        );
        answers.insert(griddb_game_url(7), grids.to_vec());
        answers.insert(
            "https://cdn2.steamgriddb.com/grid/q.png".into(),
            PNG.to_vec(),
        );
        let http = Fake {
            answers,
            asked: RefCell::new(Vec::new()),
        };
        let settings = Settings {
            steam_cdn: true,
            griddb_key: Some("key".into()),
        };
        let mut games = vec![
            game("steam:400", "Portal", Source::Steam),
            game("gog:1", "Fabrikam Quest", Source::Gog),
            game("epic:x", "Nowhere", Source::Epic),
        ];
        // Only Steam's landscape header on the device: a portrait is wanted.
        games[0].cover = Some(Path::new("librarycache").join("400").join("header.jpg"));
        let got = download_missing(&games, &HashMap::new(), &dir, &settings, &http).unwrap();
        assert_eq!(got, ["steam:400", "gog:1"]);
        // The key only goes to the SteamGridDB API, never to a CDN.
        for (url, bearer) in http.asked.borrow().iter() {
            assert_eq!(*bearer, url.starts_with(GRIDDB_API), "{url}");
        }
        assert!(dir.join("epic_x.miss").is_file());

        attach(&mut games, &dir);
        assert_eq!(games[0].cover, Some(dir.join("steam_400.jpg")));
        assert_eq!(games[1].cover, Some(dir.join("gog_1.png")));
        assert_eq!(games[2].cover, None);

        // Nothing is asked again: covers are cached, the miss is recent.
        http.asked.borrow_mut().clear();
        let mut fresh = games.clone();
        fresh[0].cover = None;
        assert!(
            download_missing(&fresh, &HashMap::new(), &dir, &settings, &http)
                .unwrap()
                .is_empty()
        );
        assert!(http.asked.borrow().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wide_art_is_downloaded_once() {
        let dir = temp_dir("wide");
        let url = "https://cdn1.epicgames.com/wide.jpg?w=1920&h=1080&resize=1";
        let http = Fake {
            answers: HashMap::from([(url.to_owned(), JPEG.to_vec())]),
            asked: RefCell::new(Vec::new()),
        };
        assert_eq!(wide_art(&dir, "epic:A", None, &http), None);
        let path = wide_art(&dir, "epic:A", Some(url), &http).unwrap();
        assert_eq!(path, dir.join("epic_A.wide.jpg"));
        // Not taken for the cover.
        assert_eq!(cached(&dir, "epic:A"), None);
        assert_eq!(wide_art(&dir, "epic:A", Some(url), &http), Some(path));
        assert_eq!(http.asked.borrow().len(), 1);
        // A failure is not asked again soon.
        let missing = "https://cdn1.epicgames.com/none.jpg";
        assert_eq!(wide_art(&dir, "epic:B", Some(missing), &http), None);
        assert_eq!(wide_art(&dir, "epic:B", Some(missing), &http), None);
        assert_eq!(http.asked.borrow().len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_fetched_when_disabled() {
        let dir = temp_dir("disabled");
        let http = Fake {
            answers: HashMap::new(),
            asked: RefCell::new(Vec::new()),
        };
        let games = [game("steam:400", "Portal", Source::Steam)];
        let got =
            download_missing(&games, &HashMap::new(), &dir, &Settings::default(), &http).unwrap();
        assert!(got.is_empty() && http.asked.borrow().is_empty());
        assert!(!dir.exists());
    }
}
