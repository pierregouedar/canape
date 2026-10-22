//! GOG: GOG Galaxy and the offline installers register each game under
//! `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>`. Launched through the
//! game's own executable (no need for Galaxy to run), else Galaxy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{Game, Launch, Source};

/// A game from the values of one registry key (value name → string data;
/// names compared ignoring case). `key` is the key's name, the game id.
pub fn parse_values(key: &str, values: &HashMap<String, String>) -> Option<Game> {
    let get = |name: &str| {
        values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };
    // DLC are registered too, pointing at their game.
    if get("dependsOn").is_some() {
        return None;
    }
    let id = get("gameID").unwrap_or(key);
    let title = get("gameName")?.to_owned();
    let dir = get("path").map(PathBuf::from);
    let launch = match get("exe") {
        Some(exe) => {
            let cwd = get("workingDir")
                .map(PathBuf::from)
                .or_else(|| dir.clone())
                .or_else(|| Path::new(exe).parent().map(Path::to_path_buf))
                .unwrap_or_default();
            Launch::Exe {
                path: PathBuf::from(exe),
                args: get("launchParam").unwrap_or_default().to_owned(),
                cwd,
            }
        }
        None => Launch::Uri {
            uri: format!("goggalaxy://openGameView/{id}"),
        },
    };
    Some(Game {
        id: format!("gog:{id}"),
        title,
        source: Source::Gog,
        cover: None,
        launch,
        last_played: None,
    })
}

/// Games registered in the registry.
#[cfg(windows)]
pub fn scan_registry() -> Vec<Game> {
    use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    let base = r"SOFTWARE\WOW6432Node\GOG.com\Games";
    let mut games = Vec::new();
    for key in crate::win::reg_subkeys(HKEY_LOCAL_MACHINE, base) {
        let values = crate::win::reg_values(HKEY_LOCAL_MACHINE, &format!(r"{base}\{key}"));
        games.extend(parse_values(&key, &values));
    }
    games
}

#[cfg(not(windows))]
pub fn scan_registry() -> Vec<Game> {
    Vec::new()
}

/// Development fixture: a JSON array of `{"key": …, "values": {…}}`.
pub fn scan_file(path: &Path) -> Vec<Game> {
    #[derive(serde::Deserialize)]
    struct Entry {
        key: String,
        values: HashMap<String, String>,
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Entry>>(&text)
        .unwrap_or_default()
        .iter()
        .filter_map(|e| parse_values(&e.key, &e.values))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn launches_the_executable_in_its_folder() {
        let game = parse_values(
            "1207658924",
            &values(&[
                ("gameID", "1207658924"),
                ("GAMENAME", "Unreal Tournament 2004"),
                ("path", r"C:\GOG Games\UT2004"),
                ("exe", r"C:\GOG Games\UT2004\System\UT2004.exe"),
                ("launchParam", "-windowed"),
                ("dependsOn", ""),
            ]),
        )
        .unwrap();
        assert_eq!(game.id, "gog:1207658924");
        assert_eq!(game.title, "Unreal Tournament 2004");
        assert_eq!(
            game.launch,
            Launch::Exe {
                path: r"C:\GOG Games\UT2004\System\UT2004.exe".into(),
                args: "-windowed".into(),
                cwd: r"C:\GOG Games\UT2004".into(),
            }
        );
    }

    #[test]
    fn working_dir_wins_and_galaxy_is_the_fallback() {
        let game = parse_values(
            "1",
            &values(&[
                ("gameName", "A"),
                ("path", r"C:\A"),
                ("exe", r"C:\A\bin\a.exe"),
                ("workingDir", r"C:\A\bin"),
            ]),
        )
        .unwrap();
        assert!(matches!(game.launch, Launch::Exe { cwd, .. } if cwd == Path::new(r"C:\A\bin")));
        let game = parse_values("2", &values(&[("gameName", "B")])).unwrap();
        assert_eq!(
            game.launch,
            Launch::Uri {
                uri: "goggalaxy://openGameView/2".into()
            }
        );
    }

    #[test]
    fn skips_dlc_and_nameless_keys() {
        assert!(parse_values("3", &values(&[("gameName", "DLC"), ("dependsOn", "1")])).is_none());
        assert!(parse_values("4", &values(&[("exe", "x.exe")])).is_none());
    }
}
