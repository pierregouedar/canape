//! Games no store knows (a standalone executable, an emulator, a game from
//! itch.io…), listed by the user in `%LOCALAPPDATA%\Canape\games.json`:
//!
//! ```json
//! { "games": [
//!   { "title": "Celeste", "exe": "D:\\Games\\Celeste\\Celeste.exe" },
//!   { "title": "Metroid Fusion", "exe": "D:\\Emu\\mGBA\\mGBA.exe",
//!     "args": "\"D:\\Roms\\Metroid Fusion.gba\"", "dir": "D:\\Emu\\mGBA",
//!     "cover": "covers\\fusion.png" }
//! ] }
//! ```
//!
//! `exe` is anything Windows opens: an executable, a shortcut (`.lnk`), a
//! script, or a URI (`itch://…`). `dir` defaults to the executable's folder,
//! a relative `cover` is taken from the folder of `games.json`. A bare array
//! of games is read too. The shell writes the same file when a game is
//! added from its shortcut list (see [`crate::shortcut`]).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Game, Launch, Source};

/// One game of `games.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub title: String,
    pub exe: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub args: String,
    /// Working folder; the executable's folder when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub dir: String,
    /// Cover image (JPEG or PNG, portrait), absolute or relative to the
    /// folder of `games.json`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cover: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    games: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Doc {
    File(File),
    List(Vec<Entry>),
}

/// The entries of `path`; none if it does not exist. An error for a file
/// that cannot be read or parsed: it must not be overwritten.
pub fn load(path: &Path) -> io::Result<Vec<Entry>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    // Notepad may save a byte order mark.
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Doc>(text) {
        Ok(Doc::File(file)) => Ok(file.games),
        Ok(Doc::List(games)) => Ok(games),
        Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    }
}

/// Rewrites `path` whole, through a temporary file.
pub fn save(path: &Path, games: &[Entry]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File {
        games: games.to_vec(),
    };
    let text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// `other:<16 hex digits>`, from the program and its arguments: an emulator
/// listed once per game gets one id per game, and editing the title keeps
/// the launch history.
pub fn id(entry: &Entry) -> String {
    let key = format!("{}\n{}", entry.exe.trim().to_lowercase(), entry.args.trim());
    let digest = crate::sha256::digest(key.as_bytes());
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("other:{hex}")
}

/// `itch://…`, `shell:AppsFolder\…`: a scheme of two characters or more
/// (`C:` is a drive).
fn is_uri(exe: &str) -> bool {
    exe.find(':').is_some_and(|i| {
        i > 1
            && exe[..i]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    })
}

/// A Windows path's folder, whichever separator it uses.
fn parent(path: &str) -> &str {
    path.rfind(['\\', '/']).map_or("", |i| &path[..i])
}

fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with(['\\', '/']) || (bytes.len() > 1 && bytes[1] == b':')
}

/// The game for `entry`; `base` is the folder of `games.json`. None without
/// a title or a program.
pub fn game(entry: &Entry, base: &Path) -> Option<Game> {
    let (title, exe) = (entry.title.trim(), entry.exe.trim());
    if title.is_empty() || exe.is_empty() {
        return None;
    }
    let args = entry.args.trim();
    let launch = if is_uri(exe) {
        let uri = if args.is_empty() {
            exe.to_owned()
        } else {
            format!("{exe} {args}")
        };
        Launch::Uri { uri }
    } else {
        let dir = match entry.dir.trim() {
            "" => parent(exe),
            dir => dir,
        };
        Launch::Exe {
            path: exe.into(),
            args: args.to_owned(),
            cwd: dir.into(),
        }
    };
    let cover = match entry.cover.trim() {
        "" => None,
        cover if is_absolute(cover) => Some(PathBuf::from(cover)),
        cover => Some(base.join(cover)),
    };
    Some(Game {
        id: id(entry),
        title: title.to_owned(),
        source: Source::Other,
        // A missing image would show an empty card: the title shows
        // instead, or a downloaded cover.
        cover: cover.filter(|path| path.is_file()),
        launch,
        last_played: None,
        size: None,
        playtime: None,
    })
}

/// The games of `games.json`; none if it is missing or unreadable.
pub fn scan(path: &Path) -> Vec<Game> {
    let base = path.parent().unwrap_or(Path::new(""));
    load(path)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| game(entry, base))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("canape-external-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entries_become_games() {
        let dir = temp("games");
        std::fs::write(dir.join("fusion.png"), b"\x89PNG").unwrap();
        let path = dir.join("games.json");
        std::fs::write(
            &path,
            "\u{feff}{\"games\": [
              {\"title\": \"Celeste\", \"exe\": \"D:\\\\Games\\\\Celeste\\\\Celeste.exe\"},
              {\"title\": \" Metroid Fusion \", \"exe\": \"D:/Emu/mGBA.exe\",
               \"args\": \"fusion.gba\", \"dir\": \"D:\\\\Roms\", \"cover\": \"fusion.png\"},
              {\"title\": \"Itch game\", \"exe\": \"itch://caves/42/launch\"},
              {\"title\": \"\", \"exe\": \"nothing.exe\"},
              {\"title\": \"No program\"},
              {\"title\": \"Lost cover\", \"exe\": \"C:\\\\x.exe\", \"cover\": \"C:\\\\nowhere.png\", \"extra\": 1}
            ]}",
        )
        .unwrap();
        let games = scan(&path);
        assert_eq!(games.len(), 4);
        assert!(games.iter().all(|g| g.source == Source::Other));
        assert_eq!(
            games[0].launch,
            Launch::Exe {
                path: r"D:\Games\Celeste\Celeste.exe".into(),
                args: String::new(),
                cwd: r"D:\Games\Celeste".into(),
            }
        );
        assert_eq!(games[0].cover, None);
        assert_eq!(games[1].title, "Metroid Fusion");
        assert_eq!(
            games[1].launch,
            Launch::Exe {
                path: "D:/Emu/mGBA.exe".into(),
                args: "fusion.gba".into(),
                cwd: r"D:\Roms".into(),
            }
        );
        assert_eq!(games[1].cover, Some(dir.join("fusion.png")));
        assert_eq!(
            games[2].launch,
            Launch::Uri {
                uri: "itch://caves/42/launch".into()
            }
        );
        assert_eq!(games[3].cover, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_follow_the_program_and_its_arguments() {
        let a = Entry {
            title: "A".into(),
            exe: r"D:\Emu\mGBA.exe".into(),
            args: "a.gba".into(),
            ..Entry::default()
        };
        let renamed = Entry {
            title: "Renamed".into(),
            exe: r"d:\emu\MGBA.EXE".into(),
            ..a.clone()
        };
        let other_rom = Entry {
            args: "b.gba".into(),
            ..a.clone()
        };
        assert_eq!(id(&a), id(&renamed));
        assert_ne!(id(&a), id(&other_rom));
        assert!(id(&a).starts_with("other:") && id(&a).len() == 6 + 16);
    }

    #[test]
    fn files_round_trip_and_bad_ones_are_errors() {
        let dir = temp("save");
        let path = dir.join("games.json");
        assert!(load(&path).unwrap().is_empty());
        let games = vec![Entry {
            title: "Celeste".into(),
            exe: r"C:\Celeste.exe".into(),
            ..Entry::default()
        }];
        save(&path, &games).unwrap();
        assert_eq!(load(&path).unwrap(), games);
        // Empty fields are left out of the file.
        assert!(!std::fs::read_to_string(&path).unwrap().contains("args"));
        std::fs::write(&path, r#"[{"title": "B", "exe": "b.exe"}]"#).unwrap();
        assert_eq!(load(&path).unwrap()[0].title, "B");
        std::fs::write(&path, "{\"games\": [ oops").unwrap();
        assert!(load(&path).is_err());
        assert!(scan(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uris_and_drives() {
        assert!(is_uri("itch://caves/1"));
        assert!(is_uri(r"shell:AppsFolder\X!App"));
        assert!(!is_uri(r"C:\Games\x.exe"));
        assert!(!is_uri("game.exe"));
        assert!(is_absolute(r"C:\x.png") && is_absolute(r"\\nas\x.png"));
        assert!(!is_absolute(r"covers\x.png"));
    }
}
