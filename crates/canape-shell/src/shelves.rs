//! The library's shelves, top to bottom: the games played lately, every
//! installed game, the user's categories (by name), each store's games
//! (installed or to install, then the games of games.json), and the owned
//! games to install. Pure: library.rs turns them into models.

use std::collections::HashMap;

use canape_library::{Game, Sort, Source};

/// Games on the recent shelf.
pub const RECENT_SHOWN: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Recent,
    Installed,
    /// One of the user's categories, by name.
    Category(String),
    Store(Source),
    ToInstall,
}

impl Kind {
    /// The number library.slint's `ShelfView.kind` uses.
    pub fn index(&self) -> i32 {
        match self {
            Kind::Recent => 0,
            Kind::Installed => 1,
            Kind::Store(_) => 2,
            Kind::ToInstall => 3,
            Kind::Category(_) => 4,
        }
    }

    /// Store name for a store's shelf, the name of a category's, "" otherwise.
    pub fn store(&self) -> &str {
        match self {
            Kind::Store(source) => source.label(),
            Kind::Category(name) => name,
            _ => "",
        }
    }
}

pub struct Shelf {
    pub kind: Kind,
    pub games: Vec<Game>,
}

/// The shelves for these `installed` games and owned games `to_install`,
/// with the user's `categories` (game id → category names). The installed
/// and to-install shelves are always there, even empty: the page explains
/// what is missing; a category shelf only when a game is in it.
pub fn build(
    installed: &[Game],
    mut to_install: Vec<Game>,
    categories: &HashMap<String, Vec<String>>,
) -> Vec<Shelf> {
    let mut shelves = Vec::new();
    let mut recent: Vec<Game> = installed
        .iter()
        .filter(|g| g.last_played.is_some())
        .cloned()
        .collect();
    canape_library::sort(&mut recent, Sort::Recent);
    recent.truncate(RECENT_SHOWN);
    if !recent.is_empty() {
        shelves.push(Shelf {
            kind: Kind::Recent,
            games: recent,
        });
    }

    let mut all = installed.to_vec();
    canape_library::sort(&mut all, Sort::Title);
    canape_library::sort(&mut to_install, Sort::Title);
    // A store's shelf mixes its installed games and those to install.
    let mut by_store: Vec<Shelf> = Source::ALL
        .into_iter()
        .map(|source| {
            let mut games: Vec<Game> = all
                .iter()
                .chain(&to_install)
                .filter(|g| g.source == source)
                .cloned()
                .collect();
            canape_library::sort(&mut games, Sort::Title);
            Shelf {
                kind: Kind::Store(source),
                games,
            }
        })
        .filter(|shelf| !shelf.games.is_empty())
        .collect();
    // A lone store with nothing to install would repeat the installed shelf.
    if by_store.len() == 1 && to_install.is_empty() {
        by_store.clear();
    }
    // Each category: its games, installed or to install, by title; names
    // compared ignoring case, the first spelling met kept.
    let mut names: Vec<String> = Vec::new();
    for game in all.iter().chain(&to_install) {
        for name in categories.get(&game.id).into_iter().flatten() {
            if !names
                .iter()
                .any(|n| n.to_lowercase() == name.to_lowercase())
            {
                names.push(name.clone());
            }
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    let by_category: Vec<Shelf> = names
        .into_iter()
        .map(|name| {
            let key = name.to_lowercase();
            let mut games: Vec<Game> = all
                .iter()
                .chain(&to_install)
                .filter(|g| {
                    categories
                        .get(&g.id)
                        .is_some_and(|names| names.iter().any(|n| n.to_lowercase() == key))
                })
                .cloned()
                .collect();
            canape_library::sort(&mut games, Sort::Title);
            Shelf {
                kind: Kind::Category(name),
                games,
            }
        })
        .collect();
    shelves.push(Shelf {
        kind: Kind::Installed,
        games: all,
    });
    shelves.extend(by_category);
    shelves.extend(by_store);
    shelves.push(Shelf {
        kind: Kind::ToInstall,
        games: to_install,
    });
    shelves
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_library::Launch;

    fn game(id: &str, source: Source, last_played: Option<u64>) -> Game {
        Game {
            id: id.to_owned(),
            title: id.to_owned(),
            source,
            cover: None,
            launch: Launch::Uri { uri: String::new() },
            last_played,
            size: None,
            playtime: None,
        }
    }

    fn none() -> HashMap<String, Vec<String>> {
        HashMap::new()
    }

    fn ids(shelf: &Shelf) -> Vec<&str> {
        shelf.games.iter().map(|g| g.id.as_str()).collect()
    }

    #[test]
    fn shelves_in_order() {
        let installed = [
            game("b", Source::Steam, Some(5)),
            game("a", Source::Epic, None),
            game("c", Source::Steam, Some(9)),
        ];
        let shelves = build(&installed, vec![game("z", Source::Epic, None)], &none());
        let kinds: Vec<Kind> = shelves.iter().map(|s| s.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                Kind::Recent,
                Kind::Installed,
                Kind::Store(Source::Steam),
                Kind::Store(Source::Epic),
                Kind::ToInstall
            ]
        );
        assert_eq!(ids(&shelves[0]), ["c", "b"]);
        assert_eq!(ids(&shelves[1]), ["a", "b", "c"]);
        assert_eq!(ids(&shelves[2]), ["b", "c"]);
        // Epic: installed and to install, mixed by title.
        assert_eq!(ids(&shelves[3]), ["a", "z"]);
        assert_eq!(ids(&shelves[4]), ["z"]);
    }

    #[test]
    fn one_store_and_nothing_played() {
        let installed = [game("a", Source::Gog, None), game("b", Source::Gog, None)];
        let kinds: Vec<Kind> = build(&installed, Vec::new(), &none())
            .iter()
            .map(|s| s.kind.clone())
            .collect();
        assert_eq!(kinds, [Kind::Installed, Kind::ToInstall]);
    }

    #[test]
    fn one_store_with_games_to_install() {
        let installed = [game("a", Source::Steam, None)];
        let shelves = build(&installed, vec![game("b", Source::Steam, None)], &none());
        let kinds: Vec<Kind> = shelves.iter().map(|s| s.kind.clone()).collect();
        assert_eq!(
            kinds,
            [Kind::Installed, Kind::Store(Source::Steam), Kind::ToInstall]
        );
        assert_eq!(ids(&shelves[1]), ["a", "b"]);
    }

    #[test]
    fn other_games_come_after_the_stores() {
        let installed = [
            game("other:1", Source::Other, Some(3)),
            game("xbox:1", Source::Xbox, None),
            game("steam:1", Source::Steam, None),
        ];
        let shelves = build(&installed, Vec::new(), &none());
        let kinds: Vec<Kind> = shelves.iter().map(|s| s.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                Kind::Recent,
                Kind::Installed,
                Kind::Store(Source::Steam),
                Kind::Store(Source::Xbox),
                Kind::Store(Source::Other),
                Kind::ToInstall
            ]
        );
        assert_eq!(shelves[4].kind.store(), "Other");
        // Only games of games.json: the installed shelf says it all.
        let alone = [game("other:1", Source::Other, None)];
        let kinds: Vec<Kind> = build(&alone, Vec::new(), &none())
            .iter()
            .map(|s| s.kind.clone())
            .collect();
        assert_eq!(kinds, [Kind::Installed, Kind::ToInstall]);
    }

    #[test]
    fn categories_come_after_the_installed_games() {
        let installed = [
            game("steam:1", Source::Steam, None),
            game("steam:2", Source::Steam, None),
            game("gog:3", Source::Gog, None),
        ];
        let categories = HashMap::from([
            ("steam:1".to_owned(), vec!["rpg".to_owned()]),
            (
                "gog:3".to_owned(),
                vec!["Co-op".to_owned(), "RPG".to_owned()],
            ),
            ("epic:9".to_owned(), vec!["Co-op".to_owned()]),
            // A game no longer here: its category has no shelf.
            ("steam:404".to_owned(), vec!["Gone".to_owned()]),
        ]);
        let shelves = build(
            &installed,
            vec![game("epic:9", Source::Epic, None)],
            &categories,
        );
        let kinds: Vec<Kind> = shelves.iter().map(|s| s.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                Kind::Installed,
                Kind::Category("Co-op".into()),
                Kind::Category("RPG".into()),
                Kind::Store(Source::Steam),
                Kind::Store(Source::Epic),
                Kind::Store(Source::Gog),
                Kind::ToInstall
            ]
        );
        // Installed and to install, by title; names compared ignoring case.
        assert_eq!(ids(&shelves[1]), ["epic:9", "gog:3"]);
        assert_eq!(ids(&shelves[2]), ["gog:3", "steam:1"]);
        assert_eq!(shelves[2].kind.store(), "RPG");
        assert_eq!(shelves[2].kind.index(), 4);
    }

    #[test]
    fn recent_is_capped() {
        let installed: Vec<Game> = (0..15)
            .map(|i| game(&format!("g{i}"), Source::Steam, Some(i)))
            .collect();
        let shelves = build(&installed, Vec::new(), &none());
        assert_eq!(shelves[0].games.len(), RECENT_SHOWN);
        assert_eq!(shelves[0].games[0].id, "g14");
    }

    #[test]
    fn empty_library_keeps_both_shelves() {
        let kinds: Vec<Kind> = build(&[], Vec::new(), &none())
            .iter()
            .map(|s| s.kind.clone())
            .collect();
        assert_eq!(kinds, [Kind::Installed, Kind::ToInstall]);
    }
}
