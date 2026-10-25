//! Time to beat on the game page ("Main story ~25 h"), from IGDB with the
//! Twitch application whose id and secret the user put in shell.json
//! (`igdb_client_id`, `igdb_client_secret`). Without them nothing is asked.
//!
//! Times are kept in `igdb.json` per game id (see
//! [`canape_library::igdb::Cache`]); a game is looked up again when its
//! entry is old, on a thread, one at a time and at most
//! [`PER_MINUTE`] times a minute. A failed lookup is logged in one line
//! and pauses the lookups for [`PAUSE`]; the page simply shows no time.
//! The token is kept in `igdb-token.json`, next to the cache.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use canape_library::igdb::{Cache, Credentials, TimeToBeat, Token};

/// Lookups started in any minute.
const PER_MINUTE: usize = 6;
/// No lookup for this long after one failed.
const PAUSE: Duration = Duration::from_secs(10 * 60);

/// What a lookup came to.
pub enum Outcome {
    /// No credentials in shell.json: the feature is off.
    Off,
    /// IGDB answered: its times, or `None` when it has none.
    Found(Option<TimeToBeat>, Option<Token>),
    /// Network or API error, already logged.
    Failed(Option<Token>),
}

/// The cache and the pace of the lookups (UI thread).
pub struct Lookups {
    cache: Cache,
    cache_path: PathBuf,
    token_path: PathBuf,
    token: Option<Token>,
    /// A development fixture never goes online.
    offline: bool,
    busy: bool,
    started: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

/// Minutes of a main story, -1 when unknown.
fn minutes(times: Option<&TimeToBeat>) -> i32 {
    to_minutes(times.and_then(TimeToBeat::main_story))
}

/// Seconds to whole minutes, -1 when unknown.
fn to_minutes(secs: Option<u64>) -> i32 {
    secs.filter(|s| *s > 0)
        .map_or(-1, |secs| ((secs + 30) / 60).min(i32::MAX as u64) as i32)
}

/// The three times of a game, in minutes, -1 for each one unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Minutes {
    /// Rushing through the main story.
    pub rushed: i32,
    /// The main story, as [`TimeToBeat::main_story`].
    pub main: i32,
    /// Everything.
    pub complete: i32,
}

impl Lookups {
    /// `suffix` is `-fixture` for a development fixture, which reads its
    /// own cache and never goes online.
    pub fn new(dir: &Path, suffix: &str, offline: bool) -> Self {
        let cache_path = dir.join(format!("igdb{suffix}.json"));
        let token_path = dir.join(format!("igdb-token{suffix}.json"));
        Self {
            cache: Cache::load(&cache_path),
            token: if offline {
                None
            } else {
                Token::load(&token_path)
            },
            cache_path,
            token_path,
            offline,
            busy: false,
            started: VecDeque::new(),
            paused_until: None,
        }
    }

    /// All the times of `id`, from the cache (even an old entry).
    pub fn all(&self, id: &str) -> Minutes {
        let times = self.cache.games.get(id).and_then(|e| e.times.as_ref());
        Minutes {
            rushed: to_minutes(times.and_then(|t| t.hastily)),
            main: minutes(times),
            complete: to_minutes(times.and_then(|t| t.completely)),
        }
    }

    /// Whether `id` should be looked up now; if so, the lookup counts as
    /// started and [`Lookups::finish`] must follow.
    pub fn start(&mut self, id: &str, now: Instant) -> bool {
        if self.offline
            || self.busy
            || self.paused_until.is_some_and(|until| now < until)
            || self.cache.fresh(id, canape_library::now()).is_some()
        {
            return false;
        }
        while self
            .started
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            self.started.pop_front();
        }
        if self.started.len() >= PER_MINUTE {
            return false;
        }
        self.started.push_back(now);
        self.busy = true;
        true
    }

    /// The token to start a lookup with, and where it is saved.
    pub fn job(&self) -> (Option<Token>, PathBuf) {
        (self.token.clone(), self.token_path.clone())
    }

    /// Takes the outcome of the lookup of `id`; returns the times now
    /// known for it.
    pub fn finish(&mut self, id: &str, outcome: Outcome) -> Minutes {
        self.busy = false;
        match outcome {
            // Keys added to shell.json are seen after the pause.
            Outcome::Off => self.paused_until = Some(Instant::now() + PAUSE),
            Outcome::Found(times, token) => {
                self.token = token.or(self.token.take());
                self.cache.record(id, times, canape_library::now());
                if let Err(err) = self.cache.save(&self.cache_path) {
                    crate::log(&format!("library: saving igdb.json: {err}"));
                }
            }
            Outcome::Failed(token) => {
                self.token = token.or(self.token.take());
                self.paused_until = Some(Instant::now() + PAUSE);
            }
        }
        self.all(id)
    }
}

/// Looks `id` (titled `title`) up on IGDB. Runs on a thread.
pub fn lookup(
    id: &str,
    title: &str,
    extra: &[String],
    token: Option<Token>,
    token_path: &Path,
) -> Outcome {
    let settings = crate::i18n::ShellSettings::load();
    let Some(credentials) =
        Credentials::new(&settings.igdb_client_id, &settings.igdb_client_secret)
    else {
        return Outcome::Off;
    };
    let before = token.clone();
    let (result, token, found) = run(&credentials, id, title, extra, token);
    if token != before
        && let Some(token) = &token
        && let Err(err) = token.save(token_path)
    {
        crate::log(&format!("library: saving the IGDB token: {err}"));
    }
    match result {
        Ok(times) => {
            // Why a game page shows no time.
            match (found, &times) {
                (None, _) => crate::log(&format!("library: {id} not found on IGDB")),
                (Some(game), None) => {
                    crate::log(&format!("library: {id} is IGDB game {game}, without times"))
                }
                _ => {}
            }
            Outcome::Found(times, token)
        }
        Err(err) => {
            // The error is a status or a system message: no secret in it.
            crate::log(&format!("library: time to beat of {id}: {err}"));
            Outcome::Failed(token)
        }
    }
}

#[cfg(windows)]
fn run(
    credentials: &Credentials,
    id: &str,
    title: &str,
    extra: &[String],
    token: Option<Token>,
) -> (io::Result<Option<TimeToBeat>>, Option<Token>, Option<u64>) {
    let http = match canape_library::http::WinHttp::new() {
        Ok(http) => http,
        Err(err) => return (Err(err), token, None),
    };
    let mut client = canape_library::igdb::Client::new(&http, credentials, token);
    let result = client.lookup(id, title, extra, canape_library::now());
    (result, client.token, client.found)
}

#[cfg(not(windows))]
fn run(
    _: &Credentials,
    _: &str,
    _: &str,
    _: &[String],
    token: Option<Token>,
) -> (io::Result<Option<TimeToBeat>>, Option<Token>, Option<u64>) {
    (
        Err(io::Error::new(io::ErrorKind::Unsupported, "no HTTP here")),
        token,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("canape-timetobeat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn minutes_round_and_fall_back() {
        assert_eq!(minutes(None), -1);
        let times = TimeToBeat {
            hastily: Some(5400),
            normally: Some(43_170),
            ..Default::default()
        };
        assert_eq!(minutes(Some(&times)), 720);
        let hasty = TimeToBeat {
            hastily: Some(5400),
            ..Default::default()
        };
        assert_eq!(minutes(Some(&hasty)), 90);
    }

    #[test]
    fn lookups_are_paced_and_cached() {
        let dir = temp("pace");
        let mut lookups = Lookups::new(&dir, "", false);
        let now = Instant::now();
        assert!(lookups.start("steam:1", now));
        // One at a time.
        assert!(!lookups.start("steam:2", now));
        let times = TimeToBeat {
            normally: Some(3600),
            ..Default::default()
        };
        assert_eq!(
            lookups.finish("steam:1", Outcome::Found(Some(times), None)),
            Minutes {
                rushed: -1,
                main: 60,
                complete: -1
            }
        );
        // Cached now, and saved.
        assert!(!lookups.start("steam:1", now));
        assert_eq!(Lookups::new(&dir, "", false).all("steam:1").main, 60);
        for i in 2..=PER_MINUTE {
            assert!(lookups.start(&format!("gog:{i}"), now));
            lookups.finish(&format!("gog:{i}"), Outcome::Found(None, None));
        }
        assert!(!lookups.start("gog:99", now));
        assert!(lookups.start("gog:99", now + Duration::from_secs(61)));
        // A failure pauses every lookup.
        lookups.finish("gog:99", Outcome::Failed(None));
        assert!(!lookups.start("gog:100", now + Duration::from_secs(200)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fixture_stays_offline() {
        let dir = temp("fixture");
        let mut lookups = Lookups::new(&dir, "-fixture", true);
        assert!(!lookups.start("steam:1", Instant::now()));
        assert_eq!(lookups.all("steam:1").main, -1);
    }
}
