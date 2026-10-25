//! Files the shell keeps in `%LOCALAPPDATA%\Canape`: `library.json`, the
//! last scan, shown at once on the next start while a new scan runs; and
//! `recent.json`, when each game was last launched from Canapé.
//!
//! Both are rewritten whole, through a temporary file, so a crash never
//! leaves half a file; an unreadable file reads as empty.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Game;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    games: Vec<Game>,
}

fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The games of the last scan.
pub fn load(path: &Path) -> Vec<Game> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<LibraryFile>(&text).ok())
        .filter(|file| file.version == VERSION)
        .map(|file| file.games)
        .unwrap_or_default()
}

pub fn save(path: &Path, games: &[Game]) -> io::Result<()> {
    let file = LibraryFile {
        version: VERSION,
        games: games.to_vec(),
    };
    write(
        path,
        &serde_json::to_string(&file).map_err(io::Error::other)?,
    )
}

/// Launches made from Canapé: game id → unix seconds, and the program each
/// game turned out to be.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recent {
    pub launched: HashMap<String, u64>,
    /// Game id → executable name (`Among Us.exe`), learnt when the game
    /// came to the front after a launch: profiles are keyed by it.
    pub exes: HashMap<String, String>,
}

impl Recent {
    /// Entries kept: older launches are forgotten.
    const KEEP: usize = 200;

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write(
            path,
            &serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )
    }

    /// `id` was launched at `at`.
    pub fn record(&mut self, id: &str, at: u64) {
        self.launched.insert(id.to_owned(), at);
        if self.launched.len() > Self::KEEP {
            let mut times: Vec<u64> = self.launched.values().copied().collect();
            times.sort_unstable_by(|a, b| b.cmp(a));
            let oldest_kept = times[Self::KEEP - 1];
            self.launched.retain(|_, t| *t >= oldest_kept);
            let launched = &self.launched;
            self.exes.retain(|id, _| launched.contains_key(id));
        }
    }

    /// The game `id` runs as `exe`.
    pub fn learn(&mut self, id: &str, exe: &str) {
        self.exes.insert(id.to_owned(), exe.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Launch, Source};

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("canape-library-test-{}", std::process::id()));
        dir.join(name)
    }

    #[test]
    fn library_round_trip_and_bad_files() {
        let path = temp("library.json");
        let games = vec![Game {
            id: "gog:1".into(),
            title: "A".into(),
            source: Source::Gog,
            cover: Some("a.png".into()),
            launch: Launch::Exe {
                path: r"C:\A\a.exe".into(),
                args: "-x".into(),
                cwd: r"C:\A".into(),
            },
            last_played: Some(7),
            size: None,
            playtime: None,
        }];
        save(&path, &games).unwrap();
        assert_eq!(load(&path), games);
        std::fs::write(&path, "{\"version\":99,\"games\":[]}").unwrap();
        assert!(load(&path).is_empty());
        std::fs::write(&path, "garbage").unwrap();
        assert!(load(&path).is_empty());
        assert!(load(&temp("missing.json")).is_empty());
    }

    #[test]
    fn recent_keeps_the_newest() {
        let mut recent = Recent::default();
        for i in 0..250 {
            recent.record(&format!("steam:{i}"), 1000 + i);
        }
        assert_eq!(recent.launched.len(), Recent::KEEP);
        assert!(recent.launched.contains_key("steam:249"));
        assert!(!recent.launched.contains_key("steam:0"));
        recent.record("steam:249", 5000);
        assert_eq!(recent.launched["steam:249"], 5000);

        let path = temp("recent.json");
        recent.save(&path).unwrap();
        assert_eq!(Recent::load(&path), recent);
    }
}
