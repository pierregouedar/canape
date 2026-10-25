//! The "Add a game" sub-page (from Settings): the games of `games.json` and
//! the Start menu and desktop shortcuts that could be games, in one list
//! picked with the controller. A adds a shortcut to `games.json`, or takes
//! a game out of it; the library then scans again.
//!
//! Shortcuts to a game a store already lists (same title or same program)
//! are left out. Finding the shortcuts runs on a thread; writing the file
//! is small and done at once.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;

use canape_library::external::{self, Entry};
use canape_library::shortcut::{self, Shortcut};
use canape_library::{Game, Launch, Source};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

use crate::{AddGameView, AppWindow, Library};

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    entry: Entry,
    added: bool,
}

struct State {
    ui: Weak<AppWindow>,
    lines: Vec<Line>,
    model: Rc<VecModel<AddGameView>>,
    /// Bumped at each opening: a late answer for an older one is dropped.
    generation: u64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// The program's file name, shown beside the title.
fn file_name(exe: &str) -> &str {
    exe.rsplit(['\\', '/']).next().unwrap_or(exe)
}

fn view(line: &Line) -> AddGameView {
    AddGameView {
        name: line.entry.title.as_str().into(),
        file: file_name(&line.entry.exe).into(),
        added: line.added,
    }
}

/// The list: what `games.json` holds, then the `shortcuts` that are not in
/// it and launch no game of the stores (`known`), all by name.
fn lines(listed: &[Entry], shortcuts: &[Shortcut], known: &[Game]) -> Vec<Line> {
    let ids: HashSet<String> = listed.iter().map(external::id).collect();
    let titles: HashSet<String> = known
        .iter()
        .filter(|g| g.source != Source::Other)
        .map(|g| canape_library::title_key(&g.title))
        .collect();
    let programs: HashSet<String> = known
        .iter()
        .filter_map(|g| match &g.launch {
            Launch::Exe { path, .. } => Some(path.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    let mut out: Vec<Line> = listed
        .iter()
        .map(|entry| Line {
            entry: entry.clone(),
            added: true,
        })
        .collect();
    for s in shortcuts {
        let entry = s.entry();
        if ids.contains(&external::id(&entry))
            || titles.contains(&canape_library::title_key(&entry.title))
            || programs.contains(&entry.exe.to_lowercase())
        {
            continue;
        }
        out.push(Line {
            entry,
            added: false,
        });
    }
    out.sort_by_key(|l| l.entry.title.to_lowercase());
    out
}

/// `listed` with `line` added, or taken out if it was there.
fn toggled(listed: &[Entry], line: &Line) -> Vec<Entry> {
    let id = external::id(&line.entry);
    let mut out: Vec<Entry> = listed
        .iter()
        .filter(|e| external::id(e) != id)
        .cloned()
        .collect();
    if !line.added {
        out.push(line.entry.clone());
    }
    out
}

fn with_state(f: impl FnOnce(&mut State)) {
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            f(state);
        }
    });
}

/// Where shortcuts are looked for: the Start menu and the desktops, or a
/// fixture's `shortcuts` folder.
fn shortcut_folders(games_file: &Path, fixture: bool) -> Vec<PathBuf> {
    match (fixture, games_file.parent()) {
        (true, Some(dir)) => vec![dir.join("shortcuts")],
        _ => shortcut::folders(),
    }
}

/// Reads `games.json` and the shortcuts again, on a thread.
fn open() {
    let Some((path, known, fixture)) = crate::library::snapshot() else {
        return;
    };
    let mut generation = 0;
    with_state(|state| {
        state.generation += 1;
        generation = state.generation;
        if let Some(ui) = state.ui.upgrade() {
            let library = ui.global::<Library>();
            library.set_add_games_loading(true);
            library.set_add_games_error("".into());
        }
    });
    thread::spawn(move || {
        let listed = external::load(&path);
        let folders = shortcut_folders(&path, fixture);
        // A fixture's targets are made up.
        let mut shortcuts = shortcut::find(&folders, &shortcut::windows_dir(), |target| {
            fixture || Path::new(target).is_file()
        });
        // Tools that drive the hardware are not games.
        shortcuts.retain(|s| !canape_core::conflicts::is_tool_name(&s.name));
        let error = listed.as_ref().err().map(|err| err.to_string());
        let found = lines(listed.as_deref().unwrap_or_default(), &shortcuts, &known);
        crate::log(&format!(
            "add games: {} listed, {} shortcuts offered",
            found.iter().filter(|l| l.added).count(),
            found.iter().filter(|l| !l.added).count()
        ));
        let _ = slint::invoke_from_event_loop(move || {
            with_state(|state| {
                if state.generation != generation {
                    return;
                }
                state
                    .model
                    .set_vec(found.iter().map(view).collect::<Vec<_>>());
                state.lines = found;
                let Some(ui) = state.ui.upgrade() else { return };
                let library = ui.global::<Library>();
                library.set_add_games_loading(false);
                if let Some(err) = error {
                    crate::log(&format!("add games: {}: {err}", path.display()));
                    library.set_add_games_error(format!("games.json: {err}").into());
                }
            })
        });
    });
}

/// A on the line `index`.
fn toggle(index: usize) {
    let Some((path, _, _)) = crate::library::snapshot() else {
        return;
    };
    let mut changed = false;
    with_state(|state| {
        let Some(line) = state.lines.get(index).cloned() else {
            return;
        };
        let Some(ui) = state.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        // An unreadable file is never overwritten: the user's games are in it.
        let result = external::load(&path).and_then(|listed| {
            let new = toggled(&listed, &line);
            external::save(&path, &new).map(|_| new.len())
        });
        match result {
            Ok(count) => {
                let added = !line.added;
                crate::log(&format!(
                    "add games: {} {}",
                    if added { "added" } else { "removed" },
                    line.entry.title
                ));
                state.lines[index].added = added;
                state.model.set_row_data(index, view(&state.lines[index]));
                library.set_external(count as i32);
                changed = true;
            }
            Err(err) => {
                crate::log(&format!("add games: {}: {err}", path.display()));
                library.set_add_games_error(format!("games.json: {err}").into());
            }
        }
    });
    if changed {
        crate::library::rescan();
    }
}

/// Wires the sub-page. Call after `library::start`.
pub fn start(ui: &AppWindow) {
    let model = Rc::new(VecModel::<AddGameView>::default());
    let library = ui.global::<Library>();
    library.set_add_games(ModelRc::from(model.clone()));
    if let Some((path, _, _)) = crate::library::snapshot() {
        let count = external::load(&path).map_or(0, |l| l.len());
        library.set_external(count as i32);
    }
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            ui: ui.as_weak(),
            lines: Vec::new(),
            model,
            generation: 0,
        })
    });
    library.on_open_add_games(open);
    library.on_toggle_add_game(|index| {
        if let Ok(index) = usize::try_from(index) {
            toggle(index);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_library::shortcut::Link;

    fn shortcut(name: &str, target: &str) -> Shortcut {
        Shortcut {
            name: name.into(),
            link: Link {
                target: target.into(),
                ..Link::default()
            },
        }
    }

    fn entry(title: &str, exe: &str) -> Entry {
        Entry {
            title: title.into(),
            exe: exe.into(),
            ..Entry::default()
        }
    }

    fn store_game(title: &str, exe: Option<&str>) -> Game {
        Game {
            id: format!("gog:{title}"),
            title: title.into(),
            source: Source::Gog,
            cover: None,
            launch: match exe {
                Some(exe) => Launch::Exe {
                    path: exe.into(),
                    args: String::new(),
                    cwd: PathBuf::new(),
                },
                None => Launch::Uri { uri: String::new() },
            },
            last_played: None,
            size: None,
            playtime: None,
        }
    }

    #[test]
    fn listed_games_and_new_shortcuts_by_name() {
        let listed = [entry("Celeste", r"D:\Celeste\Celeste.exe")];
        let shortcuts = [
            shortcut("Celeste", r"D:\Celeste\Celeste.exe"),
            shortcut("anodyne", r"D:\Anodyne\Anodyne.exe"),
            // A store already has these two.
            shortcut("HADES", r"D:\Hades\Hades.exe"),
            shortcut("Copper Crown", r"C:\GOG Games\Copper\crown.exe"),
            shortcut("Baba Is You", r"D:\Baba\Baba.exe"),
        ];
        let known = [
            store_game("Hades", None),
            store_game("Crown (GOG)", Some(r"C:\GOG Games\Copper\Crown.exe")),
        ];
        let found = lines(&listed, &shortcuts, &known);
        let names: Vec<(&str, bool)> = found
            .iter()
            .map(|l| (l.entry.title.as_str(), l.added))
            .collect();
        assert_eq!(
            names,
            [
                ("anodyne", false),
                ("Baba Is You", false),
                ("Celeste", true)
            ]
        );
        assert_eq!(view(&found[0]).file, "Anodyne.exe");
    }

    #[test]
    fn a_adds_or_takes_out() {
        let listed = vec![entry("Celeste", r"D:\Celeste\Celeste.exe")];
        let new = Line {
            entry: entry("Baba", r"D:\Baba\Baba.exe"),
            added: false,
        };
        let more = toggled(&listed, &new);
        assert_eq!(more.len(), 2);
        // Taking out matches by program, whatever the title became.
        let old = Line {
            entry: entry("Renamed", r"d:\celeste\celeste.exe"),
            added: true,
        };
        assert_eq!(toggled(&more, &old), [new.entry]);
    }
}
