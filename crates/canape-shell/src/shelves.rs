//! The library's shelves, top to bottom: the games played lately, every
//! installed game, the installed games of each store (when there are
//! several stores), and the owned games to install. Pure: library.rs
//! turns them into models.

use canape_library::{Game, Sort, Source};

/// Games on the recent shelf.
pub const RECENT_SHOWN: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Recent,
    Installed,
    Store(Source),
    ToInstall,
}

impl Kind {
    /// The number library.slint's `ShelfView.kind` uses.
    pub fn index(self) -> i32 {
        match self {
            Kind::Recent => 0,
            Kind::Installed => 1,
            Kind::Store(_) => 2,
            Kind::ToInstall => 3,
        }
    }

    /// Store name for a store's shelf, "" otherwise.
    pub fn store(self) -> &'static str {
        match self {
            Kind::Store(source) => source.label(),
            _ => "",
        }
    }
}

pub struct Shelf {
    pub kind: Kind,
    pub games: Vec<Game>,
}

/// The shelves for these `installed` games and owned games `to_install`.
/// The installed and to-install shelves are always there, even empty: the
/// page explains what is missing.
pub fn build(installed: &[Game], mut to_install: Vec<Game>) -> Vec<Shelf> {
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
    let stores: Vec<Source> = Source::ALL
        .into_iter()
        .filter(|s| all.iter().any(|g| g.source == *s))
        .collect();
    // With one store, its shelf would repeat the installed one.
    let by_store: Vec<Shelf> = if stores.len() > 1 {
        stores
            .iter()
            .map(|&source| Shelf {
                kind: Kind::Store(source),
                games: all.iter().filter(|g| g.source == source).cloned().collect(),
            })
            .collect()
    } else {
        Vec::new()
    };
    shelves.push(Shelf {
        kind: Kind::Installed,
        games: all,
    });
    shelves.extend(by_store);

    canape_library::sort(&mut to_install, Sort::Title);
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
        }
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
        let shelves = build(&installed, vec![game("z", Source::Epic, None)]);
        let kinds: Vec<Kind> = shelves.iter().map(|s| s.kind).collect();
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
        assert_eq!(ids(&shelves[4]), ["z"]);
    }

    #[test]
    fn one_store_and_nothing_played() {
        let installed = [game("a", Source::Gog, None), game("b", Source::Gog, None)];
        let kinds: Vec<Kind> = build(&installed, Vec::new())
            .iter()
            .map(|s| s.kind)
            .collect();
        assert_eq!(kinds, [Kind::Installed, Kind::ToInstall]);
    }

    #[test]
    fn recent_is_capped() {
        let installed: Vec<Game> = (0..15)
            .map(|i| game(&format!("g{i}"), Source::Steam, Some(i)))
            .collect();
        let shelves = build(&installed, Vec::new());
        assert_eq!(shelves[0].games.len(), RECENT_SHOWN);
        assert_eq!(shelves[0].games[0].id, "g14");
    }

    #[test]
    fn empty_library_keeps_both_shelves() {
        let kinds: Vec<Kind> = build(&[], Vec::new()).iter().map(|s| s.kind).collect();
        assert_eq!(kinds, [Kind::Installed, Kind::ToInstall]);
    }
}
