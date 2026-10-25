//! The game library: games installed with Steam, the Epic Games Launcher,
//! GOG Galaxy and the Xbox app (and by Canapé itself, through Legendary and
//! gogdl), plus the games the user listed in `games.json` ([`external`]),
//! merged into one list for the shell. Games
//! without art on disk can get a cover downloaded ([`online`]), and any
//! game its time to beat from IGDB ([`igdb`]).
//!
//! Every store is read from its own files (or the registry for GOG) with
//! pure parsers that are tested on Linux; only finding the files and
//! launching are Windows-specific. Nothing here needs elevation: the shell
//! scans and launches from the user session, at medium integrity. The one
//! exception, a game's anti-cheat setup ([`eac`]), runs elevated only once
//! the user allows it at Windows' prompt.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod achievements;
pub mod appinfo;
pub mod cache;
pub mod cover;
pub mod disk;
pub mod eac;
pub mod epic;
pub mod external;
pub mod gog;
pub mod gogdl;
#[cfg(windows)]
pub mod http;
pub mod igdb;
pub mod install;
mod launch;
pub mod legendary;
pub mod online;
mod sha256;
pub mod shortcut;
pub mod steam;
pub mod steam_queue;
pub mod tools;
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
    /// A game no store knows, listed in `games.json` ([`external`]).
    Other,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::Steam,
        Source::Epic,
        Source::Gog,
        Source::Xbox,
        Source::Other,
    ];

    /// Store name as shown in the UI (a brand: not translated; the shell
    /// translates `Other` itself).
    pub fn label(self) -> &'static str {
        match self {
            Source::Steam => "Steam",
            Source::Epic => "Epic",
            Source::Gog => "GOG",
            Source::Xbox => "Xbox",
            Source::Other => "Other",
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
            Source::Other => 4,
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
    /// Owned but not installed: A installs it (see [`install`]).
    Install { install: install::Install },
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
    /// Bytes on disk, from the store's install manifest.
    #[serde(default)]
    pub size: Option<u64>,
    /// Minutes played, from the store's account data (Steam).
    #[serde(default)]
    pub playtime: Option<u32>,
}

/// An owned game to install, and its art link if its store gave one.
#[derive(Debug, Clone, PartialEq)]
pub struct Owned {
    pub game: Game,
    pub art_url: Option<String>,
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
    /// Games installed with Legendary: its config folder and executable.
    pub legendary: Option<(PathBuf, PathBuf)>,
    /// The user's own list of games (`games.json`, see [`external`]).
    pub external: Option<PathBuf>,
    /// Games installed with gogdl: Canapé's record of them
    /// ([`gogdl::Installed`]).
    pub gogdl: Option<PathBuf>,
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
                legendary: None,
                external: None,
                gogdl: None,
            }
        }
        #[cfg(not(windows))]
        {
            Roots::default()
        }
    }

    /// A fake machine for development and screenshots: `<dir>/steam` (a
    /// Steam folder), `<dir>/epic` (manifests), `<dir>/gog.json` and
    /// `<dir>/xbox` (an `XboxGames` folder), `<dir>/games.json` (the user's
    /// own games). Missing parts are skipped.
    pub fn fixture(dir: &Path) -> Self {
        Roots {
            steam: Some(dir.join("steam")),
            epic: Some(dir.join("epic")),
            gog: GogSource::File(dir.join("gog.json")),
            xbox: vec![dir.join("xbox")],
            legendary: None,
            external: Some(dir.join("games.json")),
            gogdl: None,
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
    // A launcher install that Legendary took over launches through
    // Legendary, without the launcher: its own entry is left out.
    let legendary = roots
        .legendary
        .as_ref()
        .map(|(config, exe)| legendary::scan(config, exe))
        .unwrap_or_default();
    if let Some(dir) = &roots.epic {
        games.extend(
            epic::scan(dir)
                .into_iter()
                .filter(|g| legendary.iter().all(|l| l.id != g.id)),
        );
    }
    match &roots.gog {
        GogSource::None => {}
        GogSource::Registry => games.extend(gog::scan_registry()),
        GogSource::File(path) => games.extend(gog::scan_file(path)),
    }
    if let Some(record) = &roots.gogdl {
        games.extend(gogdl::scan(record));
    }
    for dir in &roots.xbox {
        games.extend(xbox::scan(dir));
    }
    games.extend(legendary);
    if let Some(path) = &roots.external {
        games.extend(external::scan(path));
    }
    games
}

/// Programs of the stores themselves (clients, launchers, their helpers):
/// what comes to the front after a launch, before the game, and is never
/// the game.
pub fn is_store_program(exe: &str) -> bool {
    const STORE: &[&str] = &[
        "steam.exe",
        "steamwebhelper.exe",
        "steamservice.exe",
        "epicgameslauncher.exe",
        "epicwebhelper.exe",
        "legendary.exe",
        "galaxyclient.exe",
        "galaxyclient helper.exe",
        "gamingservices.exe",
        "xboxpcapp.exe",
        "eadesktop.exe",
        "upc.exe",
        "battle.net.exe",
        "unitycrashhandler64.exe",
        "crashreportclient.exe",
        // Consoles and anti-cheat starters that come first, before the game.
        "windowsterminal.exe",
        "openconsole.exe",
        "conhost.exe",
        "cmd.exe",
        "start_protected_game.exe",
        "easyanticheat.exe",
        "easyanticheat_eos.exe",
        "easyanticheat_eos_setup.exe",
        "easyanticheat_setup.exe",
    ];
    STORE.iter().any(|s| s.eq_ignore_ascii_case(exe))
}

/// Folds a title for comparison: `DOOM Eternal™` and `Doom Eternal` match.
pub fn title_key(title: &str) -> String {
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
/// copy played last, else Steam, GOG, Epic, Xbox, Other in this order), sorted by
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

/// The owned games that are not installed: `owned` minus every game of
/// `installed` with the same id or the same title, and one entry per
/// store and title (see [`one_per_store`]).
pub fn not_installed(owned: &[Game], installed: &[Game]) -> Vec<Game> {
    let ids: std::collections::HashSet<&str> = installed.iter().map(|g| g.id.as_str()).collect();
    let titles: std::collections::HashSet<String> =
        installed.iter().map(|g| title_key(&g.title)).collect();
    one_per_store(
        owned
            .iter()
            .filter(|g| !ids.contains(g.id.as_str()) && !titles.contains(&title_key(&g.title)))
            .cloned()
            .collect(),
    )
}

/// `games` with one entry per id and per store and title: a store that
/// lists one game under two ids (two app names of one Epic product) shows
/// it once, the copy with a cover, else the first. Order kept.
pub fn one_per_store(games: Vec<Game>) -> Vec<Game> {
    let mut out: Vec<Game> = Vec::with_capacity(games.len());
    let mut seen: HashMap<(Source, String), usize> = HashMap::new();
    for game in games {
        let key = (game.source, title_key(&game.title));
        let same = out
            .iter()
            .position(|g| g.id == game.id)
            .or_else(|| seen.get(&key).copied().filter(|_| !key.1.is_empty()));
        match same {
            Some(i) => {
                if out[i].cover.is_none() && game.cover.is_some() {
                    out[i] = game;
                }
            }
            None => {
                seen.insert(key, out.len());
                out.push(game);
            }
        }
    }
    out
}

/// `games` with one entry per title across stores: the copy of the store
/// that comes first (Steam, GOG, Epic, Xbox, Other), else the one with a
/// cover. Order kept.
pub fn one_per_title(games: &[Game]) -> Vec<Game> {
    let mut out: Vec<Game> = Vec::with_capacity(games.len());
    let mut seen: HashMap<String, usize> = HashMap::new();
    for game in games {
        let key = title_key(&game.title);
        match seen.get(&key).copied().filter(|_| !key.is_empty()) {
            Some(i) => {
                let kept = &out[i];
                let better = (std::cmp::Reverse(game.source.rank()), game.cover.is_some())
                    > (std::cmp::Reverse(kept.source.rank()), kept.cover.is_some());
                if better {
                    out[i] = game.clone();
                }
            }
            None => {
                seen.insert(key, out.len());
                out.push(game.clone());
            }
        }
    }
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
            size: None,
            playtime: None,
        }
    }

    #[test]
    fn store_programs_are_not_games() {
        assert!(is_store_program("steam.exe"));
        assert!(is_store_program("EpicGamesLauncher.exe"));
        assert!(!is_store_program("Among Us.exe"));
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
    fn installed_games_leave_the_owned_list() {
        let installed = vec![
            game("steam:400", "Portal", Source::Steam, None),
            game("gog:1", "HADES™", Source::Gog, None),
        ];
        let owned = vec![
            game("steam:400", "Portal", Source::Steam, None),
            game("epic:x", "Hades", Source::Epic, None),
            game("steam:620", "Portal 2", Source::Steam, None),
        ];
        let left = not_installed(&owned, &installed);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "steam:620");
    }

    #[test]
    fn owned_duplicates_collapse_per_store() {
        let mut covered = game("epic:b", "DEATH STRANDING", Source::Epic, None);
        covered.cover = Some("ds.jpg".into());
        let owned = vec![
            // One Epic product under two app names: the copy with a cover.
            game("epic:a", "Death Stranding", Source::Epic, None),
            covered,
            game("epic:a", "Death Stranding", Source::Epic, None),
            // The same title on Steam stays, on its own store.
            game("steam:1190460", "DEATH STRANDING", Source::Steam, None),
            // Another game, though its title starts the same.
            game(
                "epic:c",
                "DEATH STRANDING DIRECTOR'S CUT",
                Source::Epic,
                None,
            ),
        ];
        let left = not_installed(&owned, &[]);
        assert_eq!(ids(&left), ["epic:b", "steam:1190460", "epic:c"]);
        // Across stores, once, preferring Steam.
        assert_eq!(ids(&one_per_title(&left)), ["steam:1190460", "epic:c"]);
        let mut gog = game("gog:1", "Hades", Source::Gog, None);
        gog.cover = Some("h.jpg".into());
        let mixed = [game("epic:h", "HADES", Source::Epic, None), gog];
        assert_eq!(ids(&one_per_title(&mixed)), ["gog:1"]);
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
