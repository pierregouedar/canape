//! What the user changed about games in Canapé, kept in Canapé's own
//! `overrides.json` (never in a store's files): a title of their own,
//! categories, and whether the game is hidden from the library. Keyed by
//! game id (`steam:1245620`), so a change survives rescans and reinstalls.
//!
//! ```json
//! {"version": 1, "games": {"steam:1245620": {"title": "Elden", "categories": ["RPG"], "hidden": true}}}
//! ```
//!
//! Rewritten whole through a temporary file. An unreadable file is an
//! error, so that the caller does not overwrite it.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Game;

const VERSION: u32 = 1;
/// Longest title or category name kept, in characters.
pub const MAX_NAME: usize = 80;

/// The changes made to one game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Override {
    /// Shown instead of the store's title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The user's categories, each a shelf of the library.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// Left out of every shelf.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl Override {
    fn is_empty(&self) -> bool {
        self.title.is_none() && self.categories.is_empty() && !self.hidden
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overrides {
    pub version: u32,
    pub games: BTreeMap<String, Override>,
}

impl Default for Overrides {
    fn default() -> Self {
        Self {
            version: VERSION,
            games: BTreeMap::new(),
        }
    }
}

/// A title or category name as kept: spaces trimmed and collapsed, control
/// characters dropped, at most [`MAX_NAME`] characters; `None` when
/// nothing is left.
pub fn clean_name(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let joined: String = words
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    let joined = joined.trim().to_owned();
    (!joined.is_empty()).then_some(joined)
}

impl Overrides {
    /// The file at `path`; empty when it does not exist.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err),
        };
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let file: Self = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if file.version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("version {} not supported", file.version),
            ));
        }
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
        std::fs::rename(&tmp, path)
    }

    fn get(&self, id: &str) -> Option<&Override> {
        self.games.get(id)
    }

    /// Changes the entry of `id`, dropping it once it holds nothing.
    fn edit<T>(&mut self, id: &str, f: impl FnOnce(&mut Override) -> T) -> T {
        let entry = self.games.entry(id.to_owned()).or_default();
        let out = f(entry);
        if entry.is_empty() {
            self.games.remove(id);
        }
        out
    }

    /// The user's title for `id`, if renamed.
    pub fn title(&self, id: &str) -> Option<&str> {
        self.get(id)?.title.as_deref()
    }

    /// Renames `id` to `title`; a title that is empty or the store's own
    /// (`original`) takes the rename back. True if anything changed.
    pub fn set_title(&mut self, id: &str, title: &str, original: &str) -> bool {
        let title = clean_name(title).filter(|t| t != original.trim());
        self.edit(id, |o| {
            let changed = o.title != title;
            o.title = title;
            changed
        })
    }

    pub fn hidden(&self, id: &str) -> bool {
        self.get(id).is_some_and(|o| o.hidden)
    }

    pub fn set_hidden(&mut self, id: &str, hidden: bool) {
        self.edit(id, |o| o.hidden = hidden);
    }

    /// Ids of the hidden games.
    pub fn hidden_ids(&self) -> Vec<&str> {
        self.games
            .iter()
            .filter(|(_, o)| o.hidden)
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// The categories of `id`.
    pub fn categories(&self, id: &str) -> &[String] {
        self.get(id).map_or(&[], |o| o.categories.as_slice())
    }

    /// Every category of a game that is not hidden, by name (case
    /// ignored), each once.
    pub fn all_categories(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for o in self.games.values().filter(|o| !o.hidden) {
            for name in &o.categories {
                if !names
                    .iter()
                    .any(|n| n.to_lowercase() == name.to_lowercase())
                {
                    names.push(name.clone());
                }
            }
        }
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Puts `id` in the category `name`, or takes it out if it is in it.
    /// True when the game is in it afterwards.
    pub fn toggle_category(&mut self, id: &str, name: &str) -> bool {
        let key = name.to_lowercase();
        self.edit(id, |o| {
            if let Some(i) = o.categories.iter().position(|n| n.to_lowercase() == key) {
                o.categories.remove(i);
                false
            } else {
                o.categories.push(name.to_owned());
                o.categories.sort_by_key(|n| n.to_lowercase());
                true
            }
        })
    }

    /// Puts `id` in a category typed by the user: an existing one of the
    /// same name (case ignored) keeps its spelling. Returns the name used.
    pub fn add_category(&mut self, id: &str, typed: &str) -> Option<String> {
        let name = clean_name(typed)?;
        let name = self
            .games
            .values()
            .flat_map(|o| &o.categories)
            .find(|n| n.to_lowercase() == name.to_lowercase())
            .cloned()
            .unwrap_or(name);
        if !self
            .categories(id)
            .iter()
            .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            self.toggle_category(id, &name);
        }
        Some(name)
    }

    /// The games to show: hidden ones left out, renamed ones under their
    /// new title.
    pub fn apply(&self, games: &[Game]) -> Vec<Game> {
        games
            .iter()
            .filter(|g| !self.hidden(&g.id))
            .map(|g| {
                let mut g = g.clone();
                if let Some(title) = self.title(&g.id) {
                    g.title = title.to_owned();
                }
                g
            })
            .collect()
    }

    /// Game id → its categories, for the shelves.
    pub fn category_map(&self) -> HashMap<String, Vec<String>> {
        self.games
            .iter()
            .filter(|(_, o)| !o.categories.is_empty())
            .map(|(id, o)| (id.clone(), o.categories.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Launch, Source};

    fn game(id: &str, title: &str) -> Game {
        Game {
            id: id.into(),
            title: title.into(),
            source: Source::Steam,
            cover: None,
            launch: Launch::Uri { uri: String::new() },
            last_played: None,
            size: None,
            playtime: None,
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Elden \t Ring \n"), Some("Elden Ring".into()));
        assert_eq!(clean_name(" \u{7} "), None);
        assert_eq!(clean_name(&"x".repeat(200)).unwrap().len(), MAX_NAME);
    }

    #[test]
    fn rename_and_take_it_back() {
        let mut o = Overrides::default();
        assert!(o.set_title("steam:1", " Elden ", "ELDEN RING"));
        assert_eq!(o.title("steam:1"), Some("Elden"));
        assert!(!o.set_title("steam:1", "Elden", "ELDEN RING"));
        // The store's own title, or nothing, is no rename.
        assert!(o.set_title("steam:1", "ELDEN RING", "ELDEN RING"));
        assert!(o.games.is_empty());
        o.set_title("steam:1", "Elden", "ELDEN RING");
        assert!(o.set_title("steam:1", "   ", "ELDEN RING"));
        assert_eq!(o.title("steam:1"), None);
    }

    #[test]
    fn categories_are_shared_by_name() {
        let mut o = Overrides::default();
        assert_eq!(
            o.add_category("a", " Co-op  games"),
            Some("Co-op games".into())
        );
        // The same name typed otherwise joins the existing category.
        assert_eq!(
            o.add_category("b", "co-op GAMES"),
            Some("Co-op games".into())
        );
        o.add_category("b", "Arcade");
        assert_eq!(o.categories("b"), ["Arcade", "Co-op games"]);
        assert_eq!(o.all_categories(), ["Arcade", "Co-op games"]);
        assert!(!o.toggle_category("b", "arcade"));
        assert_eq!(o.all_categories(), ["Co-op games"]);
        assert!(o.add_category("c", " ").is_none());
        // A category whose only games are hidden is not offered.
        o.set_hidden("a", true);
        o.set_hidden("b", true);
        assert!(o.all_categories().is_empty());
        assert_eq!(o.category_map().len(), 2);
    }

    #[test]
    fn hidden_games_leave_the_list_and_renames_apply() {
        let mut o = Overrides::default();
        o.set_hidden("steam:2", true);
        o.set_title("steam:1", "Mine", "Theirs");
        let shown = o.apply(&[game("steam:1", "Theirs"), game("steam:2", "Gone")]);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].title, "Mine");
        assert_eq!(o.hidden_ids(), ["steam:2"]);
        o.set_hidden("steam:2", false);
        assert!(o.hidden_ids().is_empty());
        assert_eq!(o.games.len(), 1);
    }

    #[test]
    fn file_round_trip_and_bad_files() {
        let dir = std::env::temp_dir().join(format!("canape-overrides-{}", std::process::id()));
        let path = dir.join("overrides.json");
        assert_eq!(Overrides::load(&path).unwrap(), Overrides::default());
        let mut o = Overrides::default();
        o.set_title("gog:1", "A", "B");
        o.add_category("gog:1", "RPG");
        o.set_hidden("epic:x", true);
        o.save(&path).unwrap();
        assert_eq!(Overrides::load(&path).unwrap(), o);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"hidden\": true"));
        assert!(!text.contains("\"hidden\": false"));
        std::fs::write(&path, "garbage").unwrap();
        assert!(Overrides::load(&path).is_err());
        std::fs::write(&path, r#"{"version": 9, "games": {}}"#).unwrap();
        assert!(Overrides::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
