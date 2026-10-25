//! Epic Games Launcher: one JSON `*.item` manifest per installed app in
//! `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`. Launched through
//! the launcher's URI; Epic keeps no cover art on disk.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Game, Launch, Source};

/// The manifests folder, if the launcher is installed.
#[cfg(windows)]
pub fn manifest_dir() -> Option<PathBuf> {
    let data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    Some(data.join(r"Epic\EpicGamesLauncher\Data\Manifests"))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Item {
    display_name: String,
    app_name: String,
    catalog_namespace: String,
    catalog_item_id: String,
    /// Set on DLC and add-ons: the game they belong to.
    main_game_app_name: String,
    app_categories: Vec<String>,
    #[serde(rename = "bIsIncompleteInstall")]
    is_incomplete_install: bool,
    install_size: u64,
    install_location: String,
}

/// A game from one manifest, if it is a complete game install.
pub fn parse_item(text: &str) -> Option<Game> {
    let item: Item = serde_json::from_str(text).ok()?;
    let is_game = item.app_categories.is_empty()
        || item
            .app_categories
            .iter()
            .any(|c| c.eq_ignore_ascii_case("games"));
    let is_addon = !item.main_game_app_name.is_empty() && item.main_game_app_name != item.app_name;
    if !is_game
        || is_addon
        || item.is_incomplete_install
        || item.app_name.is_empty()
        || item.catalog_namespace.is_empty()
        || item.catalog_item_id.is_empty()
    {
        return None;
    }
    let title = if item.display_name.trim().is_empty() {
        item.app_name.clone()
    } else {
        item.display_name.trim().to_owned()
    };
    Some(Game {
        id: format!("epic:{}", item.app_name),
        title,
        source: Source::Epic,
        cover: None,
        launch: Launch::Uri {
            uri: format!(
                "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true",
                item.catalog_namespace, item.catalog_item_id, item.app_name
            ),
        },
        last_played: None,
        size: Some(item.install_size).filter(|&s| s > 0),
        playtime: None,
    })
}

/// A game installed by the launcher, from one manifest: its app name and
/// its folder, for handing it to Legendary.
pub fn parse_install(text: &str) -> Option<(String, PathBuf)> {
    parse_item(text)?;
    let item: Item = serde_json::from_str(text).ok()?;
    (!item.install_location.is_empty())
        .then(|| (item.app_name, PathBuf::from(item.install_location)))
}

/// The launcher's installs in `dir` (see [`parse_install`]).
pub fn installs(dir: &Path) -> Vec<(String, PathBuf)> {
    manifests(dir)
        .iter()
        .filter_map(|text| parse_install(text))
        .collect()
}

/// Games from every manifest in `dir`.
pub fn scan(dir: &Path) -> Vec<Game> {
    manifests(dir)
        .iter()
        .filter_map(|text| parse_item(text))
        .collect()
}

/// The text of every `.item` manifest in `dir`, in file name order.
fn manifests(dir: &Path) -> Vec<String> {
    let mut items: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("item"))
        })
        .collect();
    items.sort();
    items
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .trim_start_matches('\u{feff}')
                .to_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::fixture;

    #[test]
    fn games_only() {
        let games = scan(&fixture("epic"));
        assert_eq!(games.len(), 1, "{games:?}");
        let game = &games[0];
        assert_eq!(game.id, "epic:Fortnite");
        assert_eq!(game.title, "Fortnite");
        assert_eq!(
            game.launch,
            Launch::Uri {
                uri: "com.epicgames.launcher://apps/fn%3A4fe75bbc5a674f4f9b356b5c90567da5%3AFortnite?action=launch&silent=true".into()
            }
        );
    }

    #[test]
    fn installs_for_legendary() {
        // The engine install is not a game: only Fortnite is handed over.
        assert_eq!(
            installs(&fixture("epic")),
            [(
                "Fortnite".to_owned(),
                PathBuf::from(r"C:\Program Files\Epic Games\Fortnite")
            )]
        );
    }

    #[test]
    fn rejects_addons_engines_and_partial_installs() {
        let base =
            r#""AppName":"A","CatalogNamespace":"ns","CatalogItemId":"id","DisplayName":"A""#;
        assert!(parse_item(&format!("{{{base}}}")).is_some());
        assert!(parse_item(&format!(r#"{{{base},"AppCategories":["engines"]}}"#)).is_none());
        assert!(parse_item(&format!(r#"{{{base},"MainGameAppName":"B"}}"#)).is_none());
        assert!(parse_item(&format!(r#"{{{base},"MainGameAppName":"A"}}"#)).is_some());
        assert!(parse_item(&format!(r#"{{{base},"bIsIncompleteInstall":true}}"#)).is_none());
        assert!(parse_item(r#"{"AppName":"A"}"#).is_none());
        assert!(parse_item("not json").is_none());
    }
}
