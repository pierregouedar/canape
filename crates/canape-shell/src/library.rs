//! The library page: games from `canape-library`, shown at once from the
//! last scan's cache while a new scan runs in the background; covers
//! decoded on a worker thread at the size they are shown, only for the
//! rows on screen and around them.
//!
//! Everything here runs on the UI thread except the scan, the decoding and
//! the launch itself, which post their results back to the event loop.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use canape_library::cache::{self, Recent};
use canape_library::online;
use canape_library::{Game, Played, Roots, Sort, Source};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode,
    VecModel, Weak,
};

use crate::grid::{self, Move};
use crate::{AppWindow, GameView, Library};

/// How long "Launching…" shows: the game's window usually covers the shell
/// before that.
const LAUNCH_SHOWN: Duration = Duration::from_secs(5);
/// Corner radius of the cover tiles in library.slint, logical pixels.
const COVER_RADIUS: f32 = 10.0;
/// Coming back to the page rescans if the last scan is older than this.
const RESCAN_AFTER: Duration = Duration::from_secs(60);

/// The gamepad sends arrow keys (instead of Tab / Shift+Tab) while the
/// library has the focus: the grid moves in two dimensions.
static LIBRARY_FOCUSED: AtomicBool = AtomicBool::new(false);
static LIBRARY_SHOWN: AtomicBool = AtomicBool::new(false);

/// True when the D-pad should send arrow keys.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn arrow_navigation() -> bool {
    LIBRARY_SHOWN.load(Ordering::Relaxed) && LIBRARY_FOCUSED.load(Ordering::Relaxed)
}

/// A cover to decode for the game `id`.
struct Job {
    id: String,
    path: PathBuf,
    width: u32,
    height: u32,
    /// Corner radius, physical pixels.
    radius: f32,
}

/// What the grid shows, as last reported by the page.
#[derive(Clone, Copy, PartialEq)]
struct View {
    first_row: usize,
    visible_rows: usize,
    cols: usize,
    width: u32,
    height: u32,
}

struct State {
    ui: Weak<AppWindow>,
    /// Every game, merged and deduplicated.
    all: Vec<Game>,
    /// The games shown, in model order.
    shown: Vec<Game>,
    model: Rc<VecModel<GameView>>,
    filter: Option<Source>,
    sort: Sort,
    recent: Recent,
    view: Option<View>,
    /// Ids whose cover is in the model.
    loaded: HashSet<String>,
    /// Ids sent to the decoder and not back yet.
    queued: HashSet<String>,
    /// Ids the decoder should still decode (others are skipped).
    wanted: Arc<Mutex<HashSet<String>>>,
    jobs: Sender<Job>,
    last_scan: Option<Instant>,
    scanning: bool,
    /// Development fixture: nothing is really launched.
    fixture: bool,
    roots: Roots,
    cache_path: PathBuf,
    /// Downloaded covers.
    covers_dir: PathBuf,
    recent_path: PathBuf,
    launch_timer: Timer,
    offset: i64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state(f: impl FnOnce(&mut State)) {
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            f(state);
        }
    });
}

fn source_filter(index: i32) -> Option<Source> {
    usize::try_from(index - 1)
        .ok()
        .and_then(|i| Source::ALL.get(i).copied())
}

fn played_index(game: &Game, now: u64, offset: i64) -> i32 {
    match game
        .last_played
        .map(|t| canape_library::played(now, t, offset))
    {
        None => 0,
        Some(Played::Today) => 1,
        Some(Played::Yesterday) => 2,
        Some(Played::Week) => 3,
        Some(Played::Month) => 4,
        Some(Played::Older) => 5,
    }
}

/// Where the games come from: the real stores, or in a development build
/// the fake machine of `CANAPE_LIBRARY_FIXTURE` (see
/// [`canape_library::Roots::fixture`]).
fn roots() -> (Roots, bool) {
    match std::env::var_os("CANAPE_LIBRARY_FIXTURE") {
        Some(dir) if cfg!(debug_assertions) => (Roots::fixture(&PathBuf::from(dir)), true),
        _ => (Roots::system(), false),
    }
}

impl State {
    fn rebuild(&mut self, keep: Option<String>) {
        let mut shown: Vec<Game> = self
            .all
            .iter()
            .filter(|g| self.filter.is_none_or(|s| g.source == s))
            .cloned()
            .collect();
        canape_library::sort(&mut shown, self.sort);
        let now = canape_library::now();
        let views: Vec<GameView> = shown
            .iter()
            .map(|g| GameView {
                id: g.id.as_str().into(),
                title: g.title.as_str().into(),
                source: g.source.label().into(),
                cover: Image::default(),
                art: g.cover.is_some(),
                played: played_index(g, now, self.offset),
            })
            .collect();
        let selected = keep
            .and_then(|id| shown.iter().position(|g| g.id == id))
            .unwrap_or(0);
        self.shown = shown;
        self.model.set_vec(views);
        self.loaded.clear();
        self.queued.clear();
        self.wanted.lock().unwrap().clear();
        if let Some(ui) = self.ui.upgrade() {
            let library = ui.global::<Library>();
            library.set_total(self.all.len() as i32);
            library.set_selected(selected as i32);
        }
        self.refresh_covers();
    }

    fn selected_id(&self) -> Option<String> {
        let ui = self.ui.upgrade()?;
        let index = usize::try_from(ui.global::<Library>().get_selected()).ok()?;
        self.shown.get(index).map(|g| g.id.clone())
    }

    /// Queues the covers of the rows around the view and drops the others.
    fn refresh_covers(&mut self) {
        let Some(view) = self.view else { return };
        let cols = view.cols.max(1);
        let rows = self.shown.len().div_ceil(cols);
        let wanted = grid::wanted_rows(view.first_row, view.visible_rows, rows);
        let kept = grid::kept_rows(&wanted, rows);
        let in_rows = |rows: &std::ops::Range<usize>, i: usize| rows.contains(&(i / cols));
        let scale = self
            .ui
            .upgrade()
            .map_or(1.0, |ui| ui.window().scale_factor());

        for (i, game) in self.shown.iter().enumerate() {
            if !in_rows(&kept, i) && self.loaded.remove(&game.id) {
                if let Some(mut row) = self.model.row_data(i) {
                    row.cover = Image::default();
                    self.model.set_row_data(i, row);
                }
            }
        }
        let mut wanted_ids = self.wanted.lock().unwrap();
        wanted_ids.clear();
        for (i, game) in self.shown.iter().enumerate() {
            if !in_rows(&wanted, i) {
                continue;
            }
            let Some(path) = &game.cover else { continue };
            wanted_ids.insert(game.id.clone());
            if !self.loaded.contains(&game.id) && self.queued.insert(game.id.clone()) {
                let _ = self.jobs.send(Job {
                    id: game.id.clone(),
                    path: path.clone(),
                    width: view.width,
                    height: view.height,
                    radius: COVER_RADIUS * scale,
                });
            }
        }
        // Queued but no longer wanted: the decoder skips them.
        self.queued.retain(|id| wanted_ids.contains(id));
    }

    fn cover_ready(&mut self, id: &str, image: Image) {
        self.queued.remove(id);
        // Scrolled away while it was decoded.
        if !self.wanted.lock().unwrap().contains(id) {
            return;
        }
        let Some(i) = self.shown.iter().position(|g| g.id == id) else {
            return;
        };
        if let Some(mut row) = self.model.row_data(i) {
            row.cover = image;
            self.model.set_row_data(i, row);
            self.loaded.insert(id.to_owned());
        }
    }

    fn scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_scanning(true);
        }
        let (roots, cache_path) = (self.roots.clone(), self.cache_path.clone());
        let covers = self.covers_dir.clone();
        let fixture = self.fixture;
        thread::spawn(move || {
            let started = Instant::now();
            let mut games = canape_library::scan(&roots);
            online::attach(&mut games, &covers);
            crate::log(&format!(
                "library: {} games found in {} ms",
                games.len(),
                started.elapsed().as_millis()
            ));
            publish(&cache_path, games.clone());
            // A fixture never goes online.
            if !fixture && download_covers(&games, &covers) {
                online::attach(&mut games, &covers);
                publish(&cache_path, games);
            }
        });
    }

    fn launch(&mut self, index: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        if !library.get_launching().is_empty() {
            return;
        }
        let Some(game) = self.shown.get(index).cloned() else {
            return;
        };
        library.set_launching(game.id.as_str().into());
        library.set_failed("".into());
        self.recent.record(&game.id, canape_library::now());
        if let Err(err) = self.recent.save(&self.recent_path) {
            crate::log(&format!("library: saving recent launches: {err}"));
        }
        crate::log(&format!(
            "library: launching {} ({:?})",
            game.id, game.launch
        ));

        let fixture = self.fixture;
        let (id, title) = (game.id.clone(), game.title.clone());
        thread::spawn(move || {
            let result = if fixture {
                Ok(())
            } else {
                canape_library::launch(&game.launch)
            };
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| state.launched(id, title, result));
            });
        });
    }

    /// The launch returned: show the outcome, then put the game first.
    fn launched(&mut self, id: String, title: String, result: std::io::Result<()>) {
        if let Err(err) = &result {
            crate::log(&format!("library: launching {id}: {err}"));
            if let Some(ui) = self.ui.upgrade() {
                let library = ui.global::<Library>();
                library.set_launching("".into());
                library.set_failed(title.into());
            }
        }
        let ui = self.ui.clone();
        self.launch_timer
            .start(TimerMode::SingleShot, LAUNCH_SHOWN, move || {
                if let Some(ui) = ui.upgrade() {
                    let library = ui.global::<Library>();
                    library.set_launching("".into());
                    library.set_failed("".into());
                }
                with_state(|state| {
                    let all = std::mem::take(&mut state.all);
                    state.all = canape_library::merge(all, &state.recent.launched);
                    state.rebuild(Some(id.clone()));
                });
            });
    }
}

/// Saves `games` as the last scan and shows them.
fn publish(cache_path: &Path, games: Vec<Game>) {
    if let Err(err) = cache::save(cache_path, &games) {
        crate::log(&format!("library: saving the cache: {err}"));
    }
    let _ = slint::invoke_from_event_loop(move || {
        with_state(|state| {
            state.scanning = false;
            state.last_scan = Some(Instant::now());
            state.all = canape_library::merge(games, &state.recent.launched);
            let keep = state.selected_id();
            state.rebuild(keep);
            if let Some(ui) = state.ui.upgrade() {
                let library = ui.global::<Library>();
                library.set_scanning(false);
                library.set_ready(true);
            }
        })
    });
}

/// Where covers may be downloaded from, per shell.json.
pub fn online_settings(settings: &crate::i18n::ShellSettings) -> online::Settings {
    let key = settings.steamgriddb_key.trim();
    online::Settings {
        steam_cdn: settings.online_covers,
        griddb_key: (settings.online_covers && !key.is_empty()).then(|| key.to_owned()),
    }
}

/// Downloads the covers of games that have none; true if any arrived.
fn download_covers(games: &[Game], dir: &Path) -> bool {
    let settings = online_settings(&crate::i18n::ShellSettings::load());
    if !settings.enabled() {
        return false;
    }
    #[cfg(windows)]
    let result = canape_library::http::WinHttp::new()
        .and_then(|http| online::download_missing(games, dir, &settings, &http));
    #[cfg(not(windows))]
    let result: std::io::Result<Vec<String>> = {
        let _ = (games, dir);
        Ok(Vec::new())
    };
    match result {
        Ok(got) => {
            if !got.is_empty() {
                crate::log(&format!("library: {} covers downloaded", got.len()));
            }
            !got.is_empty()
        }
        Err(err) => {
            crate::log(&format!("library: downloading covers: {err}"));
            false
        }
    }
}

/// Keeps decoding the covers asked for, in order, skipping those no
/// longer wanted.
fn spawn_decoder(wanted: Arc<Mutex<HashSet<String>>>) -> Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    thread::spawn(move || {
        for job in rx {
            if !wanted.lock().unwrap().contains(&job.id) {
                continue;
            }
            match canape_library::cover::load(&job.path, job.width, job.height) {
                Ok(rgb) => {
                    let id = job.id;
                    let rgba = canape_library::cover::rounded(&rgb, job.radius);
                    let _ = slint::invoke_from_event_loop(move || {
                        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                            &rgba, rgb.width, rgb.height,
                        );
                        with_state(|state| state.cover_ready(&id, Image::from_rgba8(buffer)));
                    });
                }
                Err(err) => {
                    crate::log(&format!("library: cover {}: {err}", job.path.display()));
                    let id = job.id;
                    let _ = slint::invoke_from_event_loop(move || {
                        with_state(|state| {
                            state.queued.remove(&id);
                        })
                    });
                }
            }
        }
    });
    tx
}

/// Tells whether the library page is the one shown.
pub fn set_shown(shown: bool) {
    LIBRARY_SHOWN.store(shown, Ordering::Relaxed);
}

/// Shows the cached library, wires the page, and starts a scan.
pub fn start(ui: &AppWindow) {
    let (roots, fixture) = roots();
    let dir = canape_core::log::dir("LOCALAPPDATA");
    // A fixture must not overwrite the real cache.
    let suffix = if fixture { "-fixture" } else { "" };
    let cache_path = dir.join(format!("library{suffix}.json"));
    let recent_path = dir.join(format!("recent{suffix}.json"));
    let covers_dir = dir.join(format!("covers{suffix}"));
    let recent = Recent::load(&recent_path);
    let cached = cache::load(&cache_path);

    let model = Rc::new(VecModel::<GameView>::default());
    let library = ui.global::<Library>();
    library.set_games(ModelRc::from(model.clone()));
    library.set_ready(!cached.is_empty());

    let wanted = Arc::new(Mutex::new(HashSet::new()));
    let mut state = State {
        ui: ui.as_weak(),
        all: canape_library::merge(cached, &recent.launched),
        shown: Vec::new(),
        model,
        filter: None,
        sort: Sort::Recent,
        recent,
        view: None,
        loaded: HashSet::new(),
        queued: HashSet::new(),
        jobs: spawn_decoder(wanted.clone()),
        wanted,
        last_scan: None,
        scanning: false,
        fixture,
        roots,
        cache_path,
        recent_path,
        covers_dir,
        launch_timer: Timer::default(),
        offset: canape_library::local_utc_offset(),
    };
    state.rebuild(None);
    state.scan();
    STATE.with(|s| *s.borrow_mut() = Some(state));

    library.on_grid_move(|index, count, cols, direction| {
        let (Ok(index), Ok(count), Ok(cols)) = (
            usize::try_from(index),
            usize::try_from(count),
            usize::try_from(cols),
        ) else {
            return -1;
        };
        Move::from_index(direction)
            .and_then(|m| grid::step(index, count, cols, m))
            .map_or(-1, |i| i as i32)
    });
    library.on_view(|first_row, visible_rows, cols, width, height| {
        let view = View {
            first_row: first_row.max(0) as usize,
            visible_rows: visible_rows.max(1) as usize,
            cols: cols.max(1) as usize,
            width: width.round().max(1.0) as u32,
            height: height.round().max(1.0) as u32,
        };
        with_state(|state| {
            if state.view == Some(view) {
                return;
            }
            let resized = state
                .view
                .is_some_and(|v| (v.width, v.height) != (view.width, view.height));
            state.view = Some(view);
            if resized {
                // Covers decoded for another size: start again.
                let keep = state.selected_id();
                state.rebuild(keep);
            } else {
                state.refresh_covers();
            }
        });
    });
    library.on_launch(|index| {
        if let Ok(index) = usize::try_from(index) {
            with_state(|state| state.launch(index));
        }
    });
    library.on_set_filter({
        let ui = ui.as_weak();
        move |index| {
            with_state(|state| {
                state.filter = source_filter(index);
                state.rebuild(None);
            });
            if let Some(ui) = ui.upgrade() {
                ui.global::<Library>().set_filter(index);
            }
        }
    });
    library.on_set_sort({
        let ui = ui.as_weak();
        move |index| {
            with_state(|state| {
                state.sort = if index == 1 {
                    Sort::Title
                } else {
                    Sort::Recent
                };
                state.rebuild(None);
            });
            if let Some(ui) = ui.upgrade() {
                ui.global::<Library>().set_sort(index);
            }
        }
    });
    library.on_focus_changed(|focused| LIBRARY_FOCUSED.store(focused, Ordering::Relaxed));
    library.on_shown(|shown| {
        set_shown(shown);
        if shown {
            with_state(|state| {
                if state.last_scan.is_some_and(|t| t.elapsed() > RESCAN_AFTER) {
                    state.scan();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_indices_follow_the_bar() {
        assert_eq!(source_filter(0), None);
        assert_eq!(source_filter(1), Some(Source::Steam));
        assert_eq!(source_filter(2), Some(Source::Epic));
        assert_eq!(source_filter(3), Some(Source::Gog));
        assert_eq!(source_filter(4), Some(Source::Xbox));
        assert_eq!(source_filter(5), None);
        assert_eq!(source_filter(-1), None);
    }
}
