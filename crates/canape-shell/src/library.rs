//! The library page: games from `canape-library`, shown at once from the
//! last scan's cache while a new scan runs in the background; covers
//! decoded on a worker thread at the size they are shown, only for the
//! games on screen and around them.
//!
//! The games sit on shelves (shelves.rs): recent, installed, one per
//! store, and last the games the user owns but has not installed (Steam
//! through the Web API key in shell.json, Epic through Legendary), which A
//! installs without opening the store's launcher. Each shelf has its own
//! model and remembers its selection; a cover is decoded once and shared
//! by every shelf that shows the game.
//!
//! Everything here runs on the UI thread except the scan, the decoding,
//! the launch, the owned lists and the installs, which post their results
//! back to the event loop.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use canape_library::cache::{self, Recent};
use canape_library::install::{self, Event, Install, Tools};
use canape_library::{Game, Launch, Played, Roots, online, tools};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer, Timer,
    TimerMode, VecModel, Weak,
};

use crate::backdrop;
use crate::grid;
use crate::shelves::{self, Kind, Shelf};
use crate::{AppWindow, DetailView, GameView, Library, ShelfView};

/// How long "Launching…" shows: the game's window usually covers the shell
/// before that.
const LAUNCH_SHOWN: Duration = Duration::from_secs(5);
/// Theme.background in theme.slint: the backdrop fades into it.
const BACKGROUND: [u8; 3] = [0x0b, 0x0c, 0x0f];
/// Corner radius of the cover tiles in library.slint, logical pixels.
const COVER_RADIUS: f32 = 10.0;
/// Coming back to the page rescans if the last scan is older than this.
const RESCAN_AFTER: Duration = Duration::from_secs(60);
/// A backdrop change this soon after the last one is part of browsing: it
/// switches at once instead of crossfading.
const BACKDROP_FADE_AFTER: Duration = Duration::from_millis(800);
/// A program that comes to the front this soon after a launch is the game.
const LEARN_WITHIN: Duration = Duration::from_secs(120);
/// How long the selection rests before the backdrop takes its colour.
const BACKDROP_DELAY: Duration = Duration::from_millis(150);
/// How long an install or store error stays in the header.
const ERROR_SHOWN: Duration = Duration::from_secs(10);
/// The owned lists are asked for again after this.
const OWNED_REFRESH: Duration = Duration::from_secs(10 * 60);

/// The gamepad sends arrow keys (instead of Tab / Shift+Tab) while the
/// library has the focus: the grid moves in two dimensions.
static LIBRARY_FOCUSED: AtomicBool = AtomicBool::new(false);
static LIBRARY_SHOWN: AtomicBool = AtomicBool::new(false);
/// Another page that moves with the arrow keys has the focus.
static PAGE_ARROWS: AtomicBool = AtomicBool::new(false);

/// True when the D-pad should send arrow keys.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn arrow_navigation() -> bool {
    (LIBRARY_SHOWN.load(Ordering::Relaxed) && LIBRARY_FOCUSED.load(Ordering::Relaxed))
        || PAGE_ARROWS.load(Ordering::Relaxed)
}

/// A page that moves with the arrow keys gained or lost the focus.
pub fn set_page_arrows(on: bool) {
    PAGE_ARROWS.store(on, Ordering::Relaxed);
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

/// What a shelf shows, as last reported by the page.
#[derive(Clone, Copy, PartialEq)]
struct View {
    /// Covers across the page.
    visible: usize,
    /// Cover size, physical pixels.
    width: u32,
    height: u32,
}

struct State {
    ui: Weak<AppWindow>,
    /// Every game, merged and deduplicated.
    all: Vec<Game>,
    /// The shelves, top to bottom, with one model each.
    shelves: Vec<Shelf>,
    models: Vec<Rc<VecModel<GameView>>>,
    /// The shelves' model, in library.slint.
    shelf_model: Rc<VecModel<ShelfView>>,
    /// Selected game on each shelf.
    picks: Vec<usize>,
    /// First game in view on each shelf (library.slint scrolls a shelf
    /// only when its selection reaches an edge).
    firsts: Vec<usize>,
    /// Shows the selected game's colour once the selection rests.
    backdrop_timer: Timer,
    /// The shelf in front.
    focus: usize,
    recent: Recent,
    view: Option<View>,
    /// Decoded covers, by game id.
    covers: HashMap<String, Image>,
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
    /// Owned games (installed or not), as entries to install.
    owned: Vec<Game>,
    /// Art links the stores gave for owned games (Epic).
    art: HashMap<String, String>,
    owned_at: Option<Instant>,
    owned_loading: bool,
    owned_path: PathBuf,
    /// Installs running: id → share done.
    installing: HashMap<String, f32>,
    /// Canapé's helper programs (Legendary).
    tools_dir: PathBuf,
    /// Clears the install or store error after a while.
    error_timer: Timer,
    /// A game just installed: once the scan lists it, the installed shelf
    /// comes to the front with it selected.
    just_installed: Option<String>,
    /// Colour of each decoded cover, for the backdrop.
    tints: HashMap<String, [u8; 3]>,
    /// Game whose colour the backdrop shows, and that colour.
    backdrop: Option<(String, [u8; 3])>,
    /// The backdrop's second layer is in front.
    backdrop_b_front: bool,
    /// A game just launched, waiting to come to the front: the program
    /// that does is that game's executable.
    awaiting: Option<(String, Instant)>,
    /// The game whose page is open.
    detail: Option<String>,
    /// Releases the game page's images once it has faded out.
    detail_timer: Timer,
    /// When the backdrop last changed colour.
    backdrop_at: Option<Instant>,
    /// The backdrop's shape for the window size it was drawn for.
    backdrop_mask: Option<(u32, u32, backdrop::Mask)>,
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
fn roots(tools_dir: &Path) -> (Roots, bool) {
    match std::env::var_os("CANAPE_LIBRARY_FIXTURE") {
        Some(dir) if cfg!(debug_assertions) => (Roots::fixture(&PathBuf::from(dir)), true),
        _ => {
            let mut roots = Roots::system();
            let exe = tools::legendary_path(tools_dir);
            roots.legendary = canape_library::legendary::config_dir()
                .filter(|_| exe.is_file())
                .map(|config| (config, exe));
            (roots, false)
        }
    }
}

impl State {
    /// Builds the shelves again and keeps the focus on `keep` (a shelf and
    /// a game on it) when it is still there, else on the same kind of
    /// shelf. Unchanged shelves keep their models, so their covers stay.
    fn rebuild(&mut self, keep: Option<(Kind, String)>) {
        let keep = keep.or_else(|| self.current());
        let previous: HashMap<Kind, String> = self
            .shelves
            .iter()
            .zip(&self.picks)
            .filter_map(|(shelf, &i)| shelf.games.get(i).map(|g| (shelf.kind, g.id.clone())))
            .collect();
        let to_install = canape_library::not_installed(&self.owned, &self.all);
        let shelves = shelves::build(&self.all, to_install);
        let now = canape_library::now();

        let same_kinds = shelves.len() == self.shelves.len()
            && shelves
                .iter()
                .zip(&self.shelves)
                .all(|(a, b)| a.kind == b.kind);
        if !same_kinds {
            self.models = shelves
                .iter()
                .map(|_| Rc::new(VecModel::default()))
                .collect();
        }
        let mut picks = Vec::with_capacity(shelves.len());
        let mut rows = Vec::with_capacity(shelves.len());
        for (s, shelf) in shelves.iter().enumerate() {
            let views: Vec<GameView> = shelf.games.iter().map(|g| self.game_view(g, now)).collect();
            let model = &self.models[s];
            let changed = model.row_count() != views.len()
                || views
                    .iter()
                    .enumerate()
                    .any(|(i, v)| model.row_data(i).is_none_or(|old| !same_view(&old, v)));
            if changed {
                model.set_vec(views);
            }
            let pick = previous
                .get(&shelf.kind)
                .and_then(|id| shelf.games.iter().position(|g| &g.id == id))
                .unwrap_or(0);
            picks.push(pick);
            rows.push(ShelfView {
                kind: shelf.kind.index(),
                store: shelf.kind.store().into(),
                games: ModelRc::from(model.clone()),
                selected: pick as i32,
                first: pick.saturating_sub(1) as i32,
            });
        }

        let focus = keep
            .as_ref()
            .and_then(|(kind, _)| shelves.iter().position(|s| s.kind == *kind))
            .or_else(|| {
                // The kind in front is gone (the recent shelf of a library
                // never played): the installed games.
                keep.as_ref()
                    .and_then(|_| shelves.iter().position(|s| s.kind == Kind::Installed))
            })
            .unwrap_or(0);
        if let Some((_, id)) = &keep
            && let Some(i) = shelves[focus].games.iter().position(|g| &g.id == id)
        {
            picks[focus] = i;
            rows[focus].selected = i as i32;
            rows[focus].first = i.saturating_sub(1) as i32;
        }

        let rows_changed = !same_kinds
            || rows.iter().enumerate().any(|(i, r)| {
                self.shelf_model
                    .row_data(i)
                    .is_none_or(|old| old.selected != r.selected || old.first != r.first)
            });
        self.shelves = shelves;
        self.firsts = rows.iter().map(|r| r.first as usize).collect();
        self.picks = picks;
        self.focus = focus;
        if !same_kinds {
            self.shelf_model.set_vec(rows);
        } else if rows_changed {
            for (i, row) in rows.into_iter().enumerate() {
                self.shelf_model.set_row_data(i, row);
            }
        }
        if let Some(ui) = self.ui.upgrade() {
            let library = ui.global::<Library>();
            library.set_total(self.all.len() as i32);
            library.set_shelf(self.focus as i32);
            library.set_first(self.firsts[self.focus] as i32);
            library.set_selected(self.picks[self.focus] as i32);
        }
        self.refresh_covers();
        self.show_backdrop();
    }

    fn game_view(&self, g: &Game, now: u64) -> GameView {
        GameView {
            id: g.id.as_str().into(),
            title: g.title.as_str().into(),
            source: g.source.label().into(),
            cover: self.covers.get(&g.id).cloned().unwrap_or_default(),
            art: g.cover.is_some(),
            played: played_index(g, now, self.offset),
            installed: !matches!(g.launch, Launch::Install { .. }),
            progress: self.installing.get(&g.id).copied().unwrap_or(-1.0),
        }
    }

    /// The shelf in front and its selected game.
    fn current(&self) -> Option<(Kind, String)> {
        let shelf = self.shelves.get(self.focus)?;
        let game = shelf.games.get(*self.picks.get(self.focus)?)?;
        Some((shelf.kind, game.id.clone()))
    }

    fn selected_id(&self) -> Option<String> {
        self.current().map(|(_, id)| id)
    }

    /// Brings the shelf `index` to the front, on the game it had selected.
    fn focus_shelf(&mut self, index: usize) {
        if index >= self.shelves.len() || index == self.focus {
            return;
        }
        // The shelf left behind shows where it was.
        let old = self.focus;
        if let Some(mut row) = self.shelf_model.row_data(old)
            && (row.selected, row.first) != (self.picks[old] as i32, self.firsts[old] as i32)
        {
            row.selected = self.picks[old] as i32;
            row.first = self.firsts[old] as i32;
            self.shelf_model.set_row_data(old, row);
        }
        self.focus = index;
        if let Some(ui) = self.ui.upgrade() {
            let library = ui.global::<Library>();
            library.set_shelf(index as i32);
            library.set_first(self.firsts[index] as i32);
            library.set_selected(self.picks[index] as i32);
        }
        if self.shelves[index].kind == Kind::ToInstall {
            self.refresh_owned(false);
        }
        self.refresh_covers();
        self.backdrop_later();
    }

    /// The game `index` of the shelf in front was selected; `first` is the
    /// first game in view.
    fn select(&mut self, index: usize, first: usize) {
        if let Some(pick) = self.picks.get_mut(self.focus) {
            *pick = index;
        }
        if let Some(f) = self.firsts.get_mut(self.focus) {
            *f = first;
        }
        self.refresh_covers();
        self.backdrop_later();
    }

    /// Changes the backdrop once the selection rests: its crossfade
    /// redraws most of the window, too much on every step of a fast scroll.
    fn backdrop_later(&mut self) {
        self.backdrop_timer
            .start(TimerMode::SingleShot, BACKDROP_DELAY, || {
                with_state(State::show_backdrop)
            });
    }

    /// Queues the covers around the selection of the shelf in front and of
    /// its neighbours, and drops the others.
    fn refresh_covers(&mut self) {
        let Some(view) = self.view else { return };
        let mut wanted: Vec<&Game> = Vec::new();
        let mut kept: HashSet<&str> = HashSet::new();
        let near = self.focus.saturating_sub(1)..(self.focus + 2).min(self.shelves.len());
        for s in near {
            let games = &self.shelves[s].games;
            let first =
                self.firsts[s].min(games.len().saturating_sub(view.visible.saturating_sub(1)));
            let rows = grid::wanted_rows(first, view.visible, games.len());
            for g in &games[grid::kept_rows(&rows, games.len())] {
                kept.insert(&g.id);
            }
            wanted.extend(&games[rows]);
        }

        let dropped: HashSet<String> = self
            .covers
            .keys()
            .filter(|id| !kept.contains(id.as_str()))
            .cloned()
            .collect();
        if !dropped.is_empty() {
            self.covers.retain(|id, _| !dropped.contains(id));
            for (shelf, model) in self.shelves.iter().zip(&self.models) {
                for (i, game) in shelf.games.iter().enumerate() {
                    if dropped.contains(&game.id)
                        && let Some(mut row) = model.row_data(i)
                    {
                        row.cover = Image::default();
                        model.set_row_data(i, row);
                    }
                }
            }
        }
        let scale = self
            .ui
            .upgrade()
            .map_or(1.0, |ui| ui.window().scale_factor());
        let mut wanted_ids = self.wanted.lock().unwrap();
        wanted_ids.clear();
        for game in wanted {
            let Some(path) = &game.cover else { continue };
            wanted_ids.insert(game.id.clone());
            if !self.covers.contains_key(&game.id) && self.queued.insert(game.id.clone()) {
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

    /// Tints the page with the selected game's colour.
    fn show_backdrop(&mut self) {
        let Some(id) = self.selected_id() else { return };
        let color = self
            .tints
            .get(&id)
            .copied()
            .unwrap_or_else(|| backdrop::fallback(&id));
        let Some(ui) = self.ui.upgrade() else { return };
        let size = ui.window().size();
        let (width, height) = if size.width > 0 && size.height > 0 {
            (size.width, size.height)
        } else {
            let scale = ui.window().scale_factor();
            ((1280.0 * scale) as u32, (720.0 * scale) as u32)
        };
        if self
            .backdrop_mask
            .as_ref()
            .is_none_or(|(w, h, _)| (*w, *h) != (width, height))
        {
            self.backdrop_mask = Some((width, height, backdrop::mask(width, height)));
            self.backdrop = None;
        }
        if self.backdrop.as_ref() == Some(&(id.clone(), color)) {
            return;
        }
        if let Some((_, _, mask)) = &self.backdrop_mask {
            let pixels = backdrop::paint(mask, color, BACKGROUND);
            let buffer =
                SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(&pixels, mask.width, mask.height);
            let image = Image::from_rgb8(buffer);
            let library = ui.global::<Library>();
            library.set_backdrop_x(mask.x as i32);
            // The first colour shows at once, and so does each colour while
            // browsing: a crossfade redraws most of the window on every
            // frame. After a pause, the new colour fades in over the last.
            let browsing = self
                .backdrop_at
                .is_some_and(|at| at.elapsed() < BACKDROP_FADE_AFTER);
            if self.backdrop.is_none() {
                library.set_backdrop_a(image.clone());
                library.set_backdrop_b(image);
            } else if browsing {
                if self.backdrop_b_front {
                    library.set_backdrop_b(image);
                } else {
                    library.set_backdrop_a(image);
                }
            } else if self.backdrop_b_front {
                library.set_backdrop_a(image);
                library.set_backdrop_b_front(false);
            } else {
                library.set_backdrop_b(image);
                library.set_backdrop_b_front(true);
            }
            self.backdrop_b_front = library.get_backdrop_b_front();
            self.backdrop_at = Some(Instant::now());
        }
        self.backdrop = Some((id, color));
    }

    /// Opens the page of the game `index` on the shelf in front: what is
    /// known at once, then its large cover and wide art from a thread.
    fn open_detail(&mut self, index: usize) {
        let Some(game) = self
            .shelves
            .get(self.focus)
            .and_then(|s| s.games.get(index))
            .cloned()
        else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else { return };
        self.detail_timer.stop();
        // Minutes played come from the owned list (Steam), for installed
        // games too.
        let minutes = game.playtime.or_else(|| {
            self.owned
                .iter()
                .find(|g| g.id == game.id)
                .and_then(|g| g.playtime)
        });
        let library = ui.global::<Library>();
        library.set_detail_view(DetailView {
            cover: Image::default(),
            size_tenths: game.size.map_or(-1, |b| (b as f64 / 1e8).round() as i32),
            minutes: minutes.map_or(-1, |m| m as i32),
            exe: self
                .recent
                .exes
                .get(&game.id)
                .cloned()
                .unwrap_or_default()
                .into(),
        });
        library.set_detail_art(Image::default());
        library.set_detail(true);
        self.detail = Some(game.id.clone());

        let scale = ui.window().scale_factor();
        let width = ui.window().size().width.max(1);
        let hero = match (&self.roots.steam, game.id.strip_prefix("steam:")) {
            (Some(steam), Some(appid)) => appid
                .parse()
                .ok()
                .and_then(|appid| canape_library::steam::hero(steam, appid)),
            _ => None,
        };
        let cover = game.cover.clone();
        let id = game.id;
        thread::spawn(move || {
            let (cw, ch) = ((240.0 * scale) as u32, (360.0 * scale) as u32);
            let cover = cover.and_then(|path| {
                canape_library::cover::load(&path, cw, ch)
                    .map_err(|err| crate::log(&format!("library: cover {}: {err}", path.display())))
                    .ok()
                    .map(|rgb| {
                        (
                            canape_library::cover::rounded(&rgb, 12.0 * scale),
                            rgb.width,
                            rgb.height,
                        )
                    })
            });
            // Steam's art is about 3.1 times as wide as high.
            let art = hero.and_then(|path| {
                canape_library::cover::load(&path, width, (width as f32 / 3.1) as u32)
                    .map_err(|err| crate::log(&format!("library: art {}: {err}", path.display())))
                    .ok()
                    .map(|mut rgb| {
                        backdrop::fade_art(&mut rgb.pixels, rgb.width, BACKGROUND);
                        rgb
                    })
            });
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    if state.detail.as_deref() != Some(id.as_str()) {
                        return;
                    }
                    let Some(ui) = state.ui.upgrade() else { return };
                    let library = ui.global::<Library>();
                    if let Some((rgba, w, h)) = cover {
                        let mut view = library.get_detail_view();
                        view.cover = Image::from_rgba8(
                            SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h),
                        );
                        library.set_detail_view(view);
                    }
                    if let Some(rgb) = art {
                        library.set_detail_art(Image::from_rgb8(
                            SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
                                &rgb.pixels,
                                rgb.width,
                                rgb.height,
                            ),
                        ));
                    }
                });
            });
        });
    }

    /// `exe` came to the front: right after a launch, it is the game's.
    fn learn_exe(&mut self, exe: &str) {
        let Some((id, at)) = self.awaiting.take() else {
            return;
        };
        if at.elapsed() > LEARN_WITHIN {
            return;
        }
        // The store's client first, then the game: keep waiting.
        if canape_library::is_store_program(exe) {
            self.awaiting = Some((id, at));
            return;
        }
        if self.recent.exes.get(&id).map(String::as_str) == Some(exe) {
            return;
        }
        crate::log(&format!("library: {id} runs as {exe}"));
        self.recent.learn(&id, exe);
        if let Err(err) = self.recent.save(&self.recent_path) {
            crate::log(&format!("library: saving recent launches: {err}"));
        }
    }

    /// Back to the shelf; the page's images go once it has faded out.
    fn close_detail(&mut self) {
        self.detail = None;
        let Some(ui) = self.ui.upgrade() else { return };
        ui.global::<Library>().set_detail(false);
        let ui = ui.as_weak();
        self.detail_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(400),
            move || {
                if let Some(ui) = ui.upgrade() {
                    let library = ui.global::<Library>();
                    if !library.get_detail() {
                        library.set_detail_art(Image::default());
                        let mut view = library.get_detail_view();
                        view.cover = Image::default();
                        library.set_detail_view(view);
                    }
                }
            },
        );
    }

    fn cover_ready(&mut self, id: &str, image: Image, tint: [u8; 3]) {
        self.queued.remove(id);
        self.tints.insert(id.to_owned(), tint);
        if self.backdrop.as_ref().is_some_and(|(shown, _)| shown == id) {
            self.show_backdrop();
        }
        // Scrolled away while it was decoded.
        if !self.wanted.lock().unwrap().contains(id) {
            return;
        }
        self.covers.insert(id.to_owned(), image.clone());
        self.update_rows(id, |row| row.cover = image.clone());
    }

    /// Changes the rows showing the game `id`, on every shelf.
    fn update_rows(&self, id: &str, change: impl Fn(&mut GameView)) {
        for (shelf, model) in self.shelves.iter().zip(&self.models) {
            for (i, game) in shelf.games.iter().enumerate() {
                if game.id == id
                    && let Some(mut row) = model.row_data(i)
                {
                    change(&mut row);
                    model.set_row_data(i, row);
                }
            }
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
            // Launcher installs handed to Legendary launch without the
            // launcher from now on: scan again to pick them up.
            if !fixture && adopt_epic_installs(&roots) {
                games = canape_library::scan(&roots);
            }
            online::attach(&mut games, &covers);
            crate::log(&format!(
                "library: {} games found in {} ms",
                games.len(),
                started.elapsed().as_millis()
            ));
            publish(&cache_path, games.clone());
            // A fixture never goes online.
            if !fixture && download_covers(&games, &HashMap::new(), &covers) {
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
        let Some(game) = self
            .shelves
            .get(self.focus)
            .and_then(|shelf| shelf.games.get(index))
            .cloned()
        else {
            return;
        };
        if let Launch::Install { install } = &game.launch {
            self.install(&game, install.clone());
            return;
        }
        library.set_launching(game.id.as_str().into());
        library.set_failed("".into());
        self.recent.record(&game.id, canape_library::now());
        self.awaiting = Some((game.id.clone(), Instant::now()));
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

    /// What the installs need: Steam's folder and Legendary, when present.
    fn tools(&self) -> Tools {
        let legendary = tools::legendary_path(&self.tools_dir);
        Tools {
            steam: self.roots.steam.clone(),
            legendary: legendary.is_file().then_some(legendary),
        }
    }

    /// Starts installing `game` on its own thread.
    fn install(&mut self, game: &Game, install: Install) {
        if self.installing.contains_key(&game.id) {
            return;
        }
        crate::log(&format!("library: installing {} ({install:?})", game.id));
        self.installing.insert(game.id.clone(), 0.0);
        self.show_progress(&game.id);
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_install_error("".into());
        }
        self.error_timer.stop();
        let (id, title, fixture, tools) = (
            game.id.clone(),
            game.title.clone(),
            self.fixture,
            self.tools(),
        );
        thread::spawn(move || {
            let post = |event: Event| {
                let (id, title) = (id.clone(), title.clone());
                let _ = slint::invoke_from_event_loop(move || {
                    with_state(|state| state.install_event(&id, &title, event));
                });
            };
            if fixture {
                // Development: a pretend download of a minute.
                for step in 1..=120 {
                    thread::sleep(Duration::from_millis(500));
                    post(Event::Progress(step as f32 / 120.0));
                }
                post(Event::Done);
            } else {
                install::run(&install, &tools, post);
            }
        });
    }

    fn install_event(&mut self, id: &str, title: &str, event: Event) {
        match event {
            Event::Progress(share) => {
                self.installing.insert(id.to_owned(), share);
                self.show_progress(id);
            }
            Event::Done => {
                crate::log(&format!("library: {id} installed"));
                self.installing.remove(id);
                // The new game shows up in the installed list.
                self.just_installed = Some(id.to_owned());
                self.scan();
            }
            Event::Failed(message) => {
                crate::log(&format!("library: installing {id}: {message}"));
                self.installing.remove(id);
                self.show_progress(id);
                self.show_error(format!("{title}: {message}"));
            }
        }
    }

    /// Shows an install or store error in the header for a while.
    fn show_error(&mut self, message: String) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.global::<Library>().set_install_error(message.into());
        let ui = ui.as_weak();
        self.error_timer
            .start(TimerMode::SingleShot, ERROR_SHOWN, move || {
                if let Some(ui) = ui.upgrade() {
                    ui.global::<Library>().set_install_error("".into());
                }
            });
    }

    /// Updates the progress shown on `id`'s tile.
    fn show_progress(&mut self, id: &str) {
        let progress = self.installing.get(id).copied().unwrap_or(-1.0);
        self.update_rows(id, |row| row.progress = progress);
    }

    /// Asks the stores for the owned games, unless done recently.
    fn refresh_owned(&mut self, force: bool) {
        if self.owned_loading
            || (!force && self.owned_at.is_some_and(|t| t.elapsed() < OWNED_REFRESH))
        {
            return;
        }
        self.owned_loading = true;
        self.show_accounts();
        let (fixture, steam, tools, covers, owned_path) = (
            self.fixture,
            self.roots.steam.clone(),
            self.tools(),
            self.covers_dir.clone(),
            self.owned_path.clone(),
        );
        // A fixture's owned games: `<fixture>/owned.json`.
        let fixture_owned = std::env::var_os("CANAPE_LIBRARY_FIXTURE")
            .map(|dir| PathBuf::from(dir).join("owned.json"))
            .unwrap_or_default();
        thread::spawn(move || {
            let settings = crate::i18n::ShellSettings::load();
            let mut owned = if fixture {
                Owned {
                    games: cache::load(&fixture_owned),
                    ..Owned::default()
                }
            } else {
                fetch_owned(&settings, steam.as_deref(), tools.legendary.as_deref())
            };
            // A failed store keeps the last list rather than emptying it.
            if owned.error.is_empty()
                && let Err(err) = cache::save(&owned_path, &owned.games)
            {
                crate::log(&format!("library: saving owned games: {err}"));
            }
            online::attach(&mut owned.games, &covers);
            post_owned(owned.clone(), false);
            if !fixture && download_covers(&owned.games, &owned.art, &covers) {
                online::attach(&mut owned.games, &covers);
                post_owned(owned, true);
            }
        });
    }

    /// Shows which stores can list owned games.
    fn show_accounts(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        let settings = crate::i18n::ShellSettings::load();
        library.set_steam_key(!settings.steam_api_key.trim().is_empty());
        library.set_owned_loading(self.owned_loading);
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
                    // The game now leads the recent shelf.
                    state.rebuild(Some((Kind::Recent, id.clone())));
                });
            });
    }
}

/// Whether a row needs no update: the same game in the same state, its
/// cover aside (covers come and go with the scroll).
fn same_view(old: &GameView, new: &GameView) -> bool {
    old.id == new.id
        && old.title == new.title
        && old.art == new.art
        && old.played == new.played
        && old.installed == new.installed
        && old.progress == new.progress
}

/// Hands the Epic Games Launcher's installs to Legendary, so they launch
/// without the launcher; once per game and session, and only with an Epic
/// account signed in (Legendary asks Epic about the game). True if any was
/// taken over.
fn adopt_epic_installs(roots: &Roots) -> bool {
    static TRIED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let (Some(dir), Some((config, exe))) = (&roots.epic, &roots.legendary) else {
        return false;
    };
    let taken: HashSet<String> = canape_library::legendary::scan(config, exe)
        .into_iter()
        .map(|g| g.id)
        .collect();
    let todo: Vec<(String, PathBuf)> = canape_library::epic::installs(dir)
        .into_iter()
        .filter(|(app, _)| !taken.contains(&format!("epic:{app}")))
        .filter(|(app, _)| !TRIED.lock().unwrap().contains(app))
        .collect();
    if todo.is_empty() || !matches!(install::epic_account(exe), Ok(Some(_))) {
        return false;
    }
    let mut adopted = false;
    for (app, path) in todo {
        TRIED.lock().unwrap().push(app.clone());
        match install::epic_import(exe, &app, &path) {
            Ok(()) => {
                crate::log(&format!("library: {app} handed to Legendary"));
                adopted = true;
            }
            Err(err) => crate::log(&format!("library: {err}")),
        }
    }
    adopted
}

/// Owned games from every store set up: Steam through the Web API, Epic
/// through Legendary. Returns them, the art links Epic gave, and the
/// signed-in Epic account.
fn fetch_owned(
    settings: &crate::i18n::ShellSettings,
    steam: Option<&Path>,
    legendary: Option<&Path>,
) -> Owned {
    let (mut owned, mut art, mut account) = (Vec::new(), HashMap::new(), None);
    let mut errors = Vec::new();
    let key = settings.steam_api_key.trim();
    #[cfg(windows)]
    if !key.is_empty()
        && let Some(steam) = steam
    {
        use canape_library::online::Http;
        let result = canape_library::steam::local_steam_id(steam)
            .ok_or_else(|| std::io::Error::other("no Steam account signed in"))
            .and_then(|id| {
                let http = canape_library::http::WinHttp::new()?;
                http.get(&canape_library::steam::owned_url(key, id), None)
            })
            .and_then(|json| {
                canape_library::steam::parse_owned(&json)
                    .ok_or_else(|| std::io::Error::other("unreadable answer (check the API key)"))
            });
        match result {
            Ok(games) => owned.extend(games),
            Err(err) => {
                crate::log(&format!("library: Steam owned games: {err}"));
                errors.push(format!("Steam: {err}"));
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (key, steam);
    if let Some(exe) = legendary {
        match install::epic_account(exe) {
            Ok(Some(name)) => {
                account = Some(name);
                match install::epic_owned(exe) {
                    Ok(list) => {
                        for item in list {
                            if let Some(url) = item.art_url {
                                art.insert(item.game.id.clone(), url);
                            }
                            owned.push(item.game);
                        }
                    }
                    Err(err) => {
                        crate::log(&format!("library: Epic owned games: {err}"));
                        errors.push(format!("Epic: {err}"));
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                crate::log(&format!("library: Epic account: {err}"));
                errors.push(format!("Epic: {err}"));
            }
        }
    }
    Owned {
        games: owned,
        art,
        account,
        error: errors.join(" · "),
    }
}

/// What the stores answered.
#[derive(Clone, Default)]
struct Owned {
    games: Vec<Game>,
    /// Art links Epic gave, by game id.
    art: HashMap<String, String>,
    /// Signed-in Epic account.
    account: Option<String>,
    /// Why a store could not be listed, "" when all went well.
    error: String,
}

fn post_owned(owned: Owned, covers_only: bool) {
    let _ = slint::invoke_from_event_loop(move || {
        with_state(|state| {
            state.owned = owned.games;
            state.art = owned.art;
            if !covers_only {
                state.owned_loading = false;
                state.owned_at = Some(Instant::now());
                if !owned.error.is_empty() {
                    state.show_error(owned.error);
                }
            }
            if let Some(ui) = state.ui.upgrade() {
                ui.global::<Library>()
                    .set_epic_account(owned.account.unwrap_or_default().into());
            }
            state.show_accounts();
            state.rebuild(None);
        })
    });
}

/// Downloads Legendary if needed, then opens its Epic sign-in window and
/// lists the owned games again.
fn epic_login() {
    let dir = canape_core::log::dir("LOCALAPPDATA").join("tools");
    let busy = |on: bool, error: String| {
        let _ = slint::invoke_from_event_loop(move || {
            with_state(|state| {
                if let Some(ui) = state.ui.upgrade() {
                    let library = ui.global::<Library>();
                    library.set_epic_busy(on);
                }
                if !error.is_empty() {
                    state.show_error(error);
                }
                if !on {
                    state.refresh_owned(true);
                }
            })
        });
    };
    busy(true, String::new());
    thread::spawn(move || {
        let exe = tools::legendary_path(&dir);
        #[cfg(windows)]
        let ready = if exe.is_file() {
            Ok(exe)
        } else {
            canape_library::http::WinHttp::new()
                .and_then(|http| tools::fetch_legendary(&http, &dir))
        };
        #[cfg(not(windows))]
        let ready: std::io::Result<PathBuf> = Err(std::io::Error::other(format!(
            "Epic sign-in needs Windows ({})",
            exe.display()
        )));
        let result = ready.and_then(|exe| install::epic_login(&exe));
        match result {
            Ok(_) => busy(false, String::new()),
            Err(err) => {
                crate::log(&format!("library: Epic sign-in: {err}"));
                busy(false, format!("Epic: {err}"));
            }
        }
    });
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
            let mut keep = None;
            // Leave the games to install for the one just installed, rather
            // than land on another one A would install.
            if let Some(id) = state.just_installed.take()
                && state.all.iter().any(|g| g.id == id)
            {
                keep = Some((Kind::Installed, id));
            }
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

/// Downloads the covers of games that have none (`art`: links the stores
/// gave); true if any arrived.
fn download_covers(games: &[Game], art: &HashMap<String, String>, dir: &Path) -> bool {
    let settings = online_settings(&crate::i18n::ShellSettings::load());
    if !settings.enabled() {
        return false;
    }
    #[cfg(windows)]
    let result = canape_library::http::WinHttp::new()
        .and_then(|http| online::download_missing(games, art, dir, &settings, &http));
    #[cfg(not(windows))]
    let result: std::io::Result<Vec<String>> = {
        let _ = (games, art, dir);
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
                    let tint = backdrop::tint(&rgb.pixels);
                    let rgba = canape_library::cover::rounded(&rgb, job.radius);
                    let _ = slint::invoke_from_event_loop(move || {
                        let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                            &rgba, rgb.width, rgb.height,
                        );
                        with_state(|state| state.cover_ready(&id, Image::from_rgba8(buffer), tint));
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

/// The game in front changed (see foreground.rs).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn foreground_seen(exe: Option<String>) {
    let Some(exe) = exe else { return };
    let _ = slint::invoke_from_event_loop(move || with_state(|state| state.learn_exe(&exe)));
}

/// Tells whether the library page is the one shown.
pub fn set_shown(shown: bool) {
    LIBRARY_SHOWN.store(shown, Ordering::Relaxed);
}

/// Shows the cached library, wires the page, and starts a scan.
pub fn start(ui: &AppWindow) {
    let dir = canape_core::log::dir("LOCALAPPDATA");
    let tools_dir = dir.join("tools");
    let (roots, fixture) = roots(&tools_dir);
    // A fixture must not overwrite the real cache.
    let suffix = if fixture { "-fixture" } else { "" };
    let cache_path = dir.join(format!("library{suffix}.json"));
    let recent_path = dir.join(format!("recent{suffix}.json"));
    let covers_dir = dir.join(format!("covers{suffix}"));
    let owned_path = dir.join(format!("owned{suffix}.json"));
    let owned = cache::load(&owned_path);
    let recent = Recent::load(&recent_path);
    let cached = cache::load(&cache_path);

    let shelf_model = Rc::new(VecModel::<ShelfView>::default());
    let library = ui.global::<Library>();
    library.set_shelves(ModelRc::from(shelf_model.clone()));
    library.set_ready(!cached.is_empty());

    let wanted = Arc::new(Mutex::new(HashSet::new()));
    let mut state = State {
        ui: ui.as_weak(),
        all: canape_library::merge(cached, &recent.launched),
        shelves: Vec::new(),
        models: Vec::new(),
        shelf_model,
        picks: Vec::new(),
        firsts: Vec::new(),
        backdrop_timer: Timer::default(),
        focus: 0,
        recent,
        view: None,
        covers: HashMap::new(),
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
        owned,
        art: HashMap::new(),
        owned_at: None,
        owned_loading: false,
        owned_path,
        installing: HashMap::new(),
        tools_dir,
        error_timer: Timer::default(),
        just_installed: None,
        tints: HashMap::new(),
        backdrop: None,
        backdrop_b_front: false,
        backdrop_mask: None,
        backdrop_at: None,
        detail: None,
        detail_timer: Timer::default(),
        awaiting: None,
    };
    state.rebuild(None);
    state.scan();
    // The shelf of games to install comes last.
    state.refresh_owned(false);
    state.show_accounts();
    STATE.with(|s| *s.borrow_mut() = Some(state));

    library.on_view(|visible, width, height| {
        let view = View {
            visible: visible.max(1) as usize,
            width: width.round().max(1.0) as u32,
            height: height.round().max(1.0) as u32,
        };
        with_state(|state| {
            // The window may have changed size.
            state.show_backdrop();
            if state.view == Some(view) {
                return;
            }
            let resized = state
                .view
                .is_some_and(|v| (v.width, v.height) != (view.width, view.height));
            state.view = Some(view);
            if resized {
                // Covers decoded for another size: start again.
                state.covers.clear();
                state.queued.clear();
                for model in &state.models {
                    for i in 0..model.row_count() {
                        if let Some(mut row) = model.row_data(i) {
                            row.cover = Image::default();
                            model.set_row_data(i, row);
                        }
                    }
                }
                state.refresh_covers();
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
    library.on_select(|index, first| {
        if let (Ok(index), Ok(first)) = (usize::try_from(index), usize::try_from(first)) {
            with_state(|state| state.select(index, first));
        }
    });
    library.on_focus_shelf(|index| {
        if let Ok(index) = usize::try_from(index) {
            with_state(|state| state.focus_shelf(index));
        }
    });
    library.on_epic_login(epic_login);
    library.on_open_detail(|index| {
        if let Ok(index) = usize::try_from(index) {
            with_state(|state| state.open_detail(index));
        }
    });
    library.on_close_detail(|| with_state(State::close_detail));
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
