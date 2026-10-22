//! The game library: games installed with Steam, the Epic Games Launcher,
//! GOG Galaxy and the Xbox app, merged into one list for the shell.
//!
//! Every store is read from its own files (or the registry for GOG) with
//! pure parsers that are tested on Linux; only finding the files and
//! launching are Windows-specific. Nothing here needs elevation: the shell
//! scans and launches from the user session, at medium integrity.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod cache;
pub mod cover;
pub mod epic;
pub mod gog;
mod launch;
mod sha256;
pub mod steam;
pub mod vdf;
#[cfg(windows)]
mod win;
pub mod xbox;
mod xml;

pub use launch::launch;

/// Where a game comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Steam,
    Epic,
    Gog,
    Xbox,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::Steam, Source::Epic, Source::Gog, Source::Xbox];

    /// Store name as shown in the UI (a brand: not translated).
    pub fn label(self) -> &'static str {
        match self {
            Source::Steam => "Steam",
            Source::Epic => "Epic",
            Source::Gog => "GOG",
            Source::Xbox => "Xbox",
        }
    }

    /// Which copy wins when the same game is installed from several stores
    /// and none was played: lower first.
    fn rank(self) -> u8 {
        match self {
            Source::Steam => 0,
            Source::Gog => 1,
            Source::Epic => 2,
            Source::Xbox => 3,
        }
    }
}

/// How to start a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Launch {
    /// A URI handled by the store's launcher (`steam://…`,
    /// `com.epicgames.launcher://…`, `shell:AppsFolder\…`).
    Uri { uri: String },
    /// An executable, started in `cwd` with `args`.
    Exe {
        path: PathBuf,
        args: String,
        cwd: PathBuf,
    },
}

/// One installed game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    /// `<source>:<the store's id>`, e.g. `steam:1245620`: stable across scans.
    pub id: String,
    pub title: String,
    pub source: Source,
    /// Local cover art, portrait if possible.
    #[serde(default)]
    pub cover: Option<PathBuf>,
    pub launch: Launch,
    /// Unix seconds, from the store or from Canapé's own launches.
    #[serde(default)]
    pub last_played: Option<u64>,
}

/// Where to look for games.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    /// Steam's install folder (holds `steamapps`, `appcache`, `userdata`).
    pub steam: Option<PathBuf>,
    /// Folder of the Epic launcher's `*.item` manifests.
    pub epic: Option<PathBuf>,
    pub gog: GogSource,
    /// `XboxGames` folders.
    pub xbox: Vec<PathBuf>,
}

/// Where GOG games are listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GogSource {
    #[default]
    None,
    /// `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games` (Windows).
    Registry,
    /// A JSON array of registry-like value maps, for development fixtures.
    File(PathBuf),
}

impl Roots {
    /// The real stores of this machine. Empty off Windows.
    pub fn system() -> Self {
        #[cfg(windows)]
        {
            Roots {
                steam: Some(steam::install_dir()),
                epic: epic::manifest_dir(),
                gog: GogSource::Registry,
                xbox: xbox::roots(),
            }
        }
        #[cfg(not(windows))]
        {
            Roots::default()
        }
    }

    /// A fake machine for development and screenshots: `<dir>/steam` (a
    /// Steam folder), `<dir>/epic` (manifests), `<dir>/gog.json` and
    /// `<dir>/xbox` (an `XboxGames` folder). Missing parts are skipped.
    pub fn fixture(dir: &Path) -> Self {
        Roots {
            steam: Some(dir.join("steam")),
            epic: Some(dir.join("epic")),
            gog: GogSource::File(dir.join("gog.json")),
            xbox: vec![dir.join("xbox")],
        }
    }
}

/// Every installed game found under `roots`, unsorted and not deduplicated
/// (see [`merge`]). Unreadable stores are skipped.
pub fn scan(roots: &Roots) -> Vec<Game> {
    let mut games = Vec::new();
    if let Some(dir) = &roots.steam {
        games.extend(steam::scan(dir));
    }
    if let Some(dir) = &roots.epic {
        games.extend(epic::scan(dir));
    }
    match &roots.gog {
        GogSource::None => {}
        GogSource::Registry => games.extend(gog::scan_registry()),
        GogSource::File(path) => games.extend(gog::scan_file(path)),
    }
    for dir in &roots.xbox {
        games.extend(xbox::scan(dir));
    }
    games
}

/// Folds a title for comparison: `DOOM Eternal™` and `Doom Eternal` match.
fn title_key(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Order in which the library is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    /// Last played first, then by title.
    #[default]
    Recent,
    /// By title only.
    Title,
}

/// Sorts `games` in place.
pub fn sort(games: &mut [Game], order: Sort) {
    let by_title = |a: &Game, b: &Game| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    };
    match order {
        Sort::Recent => games.sort_by(|a, b| {
            // `Some` before `None`, newest first.
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| by_title(a, b))
        }),
        Sort::Title => games.sort_by(by_title),
    }
}

/// The list to show: Canapé's own launch times applied (`launched`: id →
/// unix seconds), one entry per id, one entry per title across stores (the
/// copy played last, else Steam, GOG, Epic, Xbox in this order), sorted by
/// [`Sort::Recent`].
pub fn merge(games: Vec<Game>, launched: &HashMap<String, u64>) -> Vec<Game> {
    let mut out: Vec<Game> = Vec::with_capacity(games.len());
    let mut by_title: HashMap<String, usize> = HashMap::new();
    for mut game in games {
        if let Some(&at) = launched.get(&game.id) {
            game.last_played = game.last_played.max(Some(at));
        }
        let key = title_key(&game.title);
        let same = out
            .iter()
            .position(|g| g.id == game.id)
            .or_else(|| by_title.get(&key).copied().filter(|_| !key.is_empty()));
        match same {
            Some(i) => {
                let kept = &mut out[i];
                let better = (game.last_played, std::cmp::Reverse(game.source.rank()))
                    > (kept.last_played, std::cmp::Reverse(kept.source.rank()));
                // The same game seen twice (two library folders): keep
                // whatever art either copy has.
                let same_id = kept.id == game.id;
                if better {
                    if same_id && game.cover.is_none() {
                        game.cover = kept.cover.take();
                    }
                    by_title.remove(&title_key(&kept.title));
                    *kept = game;
                    by_title.insert(key, i);
                } else if same_id && kept.cover.is_none() {
                    kept.cover = game.cover;
                }
            }
            None => {
                by_title.insert(key, out.len());
                out.push(game);
            }
        }
    }
    sort(&mut out, Sort::Recent);
    out
}

/// How long ago a game was played, in calendar terms, for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Played {
    Today,
    Yesterday,
    /// Earlier, within 7 days.
    Week,
    /// Earlier, within 30 days.
    Month,
    Older,
}

/// When `then` falls relative to `now` (unix seconds), counting calendar
/// days in a time zone `offset` seconds east of UTC.
pub fn played(now: u64, then: u64, offset: i64) -> Played {
    let day = |t: u64| (t as i64 + offset).div_euclid(86_400);
    match day(now) - day(then) {
        i64::MIN..=0 => Played::Today,
        1 => Played::Yesterday,
        2..=6 => Played::Week,
        7..=29 => Played::Month,
        _ => Played::Older,
    }
}

/// Offset of local time from UTC, in seconds east (daylight saving
/// included). 0 off Windows: development only.
pub fn local_utc_offset() -> i64 {
    #[cfg(windows)]
    {
        win::utc_offset()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Current time, unix seconds.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::path::PathBuf;

    /// `tests/fixtures/<name>`.
    pub fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, title: &str, source: Source, last_played: Option<u64>) -> Game {
        Game {
            id: id.into(),
            title: title.into(),
            source,
            cover: None,
            launch: Launch::Uri { uri: id.into() },
            last_played,
        }
    }

    fn ids(games: &[Game]) -> Vec<&str> {
        games.iter().map(|g| g.id.as_str()).collect()
    }

    #[test]
    fn merge_sorts_by_last_played_then_title() {
        let games = vec![
            game("steam:1", "beta", Source::Steam, None),
            game("steam:2", "Alpha", Source::Steam, None),
            game("steam:3", "Zeta", Source::Steam, Some(100)),
            game("epic:x", "Gamma", Source::Epic, None),
            game("steam:4", "Omega", Source::Steam, Some(300)),
        ];
        let merged = merge(games, &HashMap::new());
        assert_eq!(
            ids(&merged),
            ["steam:4", "steam:3", "steam:2", "steam:1", "epic:x"]
        );
    }

    #[test]
    fn canape_launches_count_as_played() {
        let games = vec![
            game("steam:1", "A", Source::Steam, Some(100)),
            game("gog:2", "B", Source::Gog, None),
        ];
        let launched = HashMap::from([("gog:2".to_string(), 200), ("steam:1".to_string(), 50)]);
        let merged = merge(games, &launched);
        assert_eq!(ids(&merged), ["gog:2", "steam:1"]);
        // An older Canapé launch does not hide a newer one from the store.
        assert_eq!(merged[1].last_played, Some(100));
    }

    #[test]
    fn duplicates_collapse() {
        let mut twice = game("steam:1", "Hades", Source::Steam, None);
        twice.cover = Some("hades.jpg".into());
        let games = vec![
            game("steam:1", "Hades", Source::Steam, None),
            twice,
            // Same title from other stores: Steam wins when none was played…
            game("epic:h", "HADES™", Source::Epic, None),
            game("gog:h", "Hades", Source::Gog, None),
            // …and the one played last wins otherwise.
            game("xbox:c", "Celeste", Source::Xbox, Some(10)),
            game("steam:c", "Celeste", Source::Steam, None),
        ];
        let merged = merge(games, &HashMap::new());
        assert_eq!(ids(&merged), ["xbox:c", "steam:1"]);
        assert_eq!(merged[1].cover, Some(PathBuf::from("hades.jpg")));
    }

    #[test]
    fn title_sort_ignores_case() {
        let mut games = vec![
            game("a", "zelda", Source::Steam, Some(5)),
            game("b", "Ark", Source::Steam, None),
            game("c", "balatro", Source::Steam, Some(9)),
        ];
        sort(&mut games, Sort::Title);
        assert_eq!(ids(&games), ["b", "c", "a"]);
    }

    #[test]
    fn played_counts_calendar_days() {
        let day = 86_400;
        // 2026-10-22 08:00 UTC.
        let now = 1_792_627_200 + 8 * 3600;
        assert_eq!(played(now, now - 60, 0), Played::Today);
        // 23:00 the day before is yesterday, though less than 24 h ago.
        assert_eq!(played(now, now - 9 * 3600, 0), Played::Yesterday);
        // … but still today at UTC+2 (01:00 local).
        assert_eq!(played(now, now - 9 * 3600, 2 * 3600), Played::Today);
        assert_eq!(played(now, now - 3 * day, 0), Played::Week);
        assert_eq!(played(now, now - 20 * day, 0), Played::Month);
        assert_eq!(played(now, now - 90 * day, 0), Played::Older);
        // A clock set back: still today.
        assert_eq!(played(now, now + 3 * day, 0), Played::Today);
    }
}
