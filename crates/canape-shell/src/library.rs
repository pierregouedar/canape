//! The library page: games from `canape-library`, shown at once from the
//! last scan's cache while a new scan runs in the background; covers
//! decoded on a worker thread at the size they are shown, only for the
//! games on screen and around them.
//!
//! The games sit on shelves (shelves.rs): recent, installed, one per
//! store, and last the games the user owns but has not installed (Steam
//! through the Web API key in shell.json, Epic through Legendary, GOG
//! through gogdl), which A installs without opening the store's launcher. Each shelf has its own
//! model and remembers its selection; a cover is decoded once and shared
//! by every shelf that shows the game.
//!
//! The game page's options (Y) rename a game, put it in categories (each a
//! shelf), hide it (or take a game of games.json out of the file) and
//! uninstall it: titles, categories and hidden games are Canapé's own, in
//! `overrides.json` (see `canape_library::overrides`), never in a store's
//! files; uninstalls go through each store (`canape_library::uninstall`).
//!
//! Everything here runs on the UI thread except the scan, the decoding,
//! the launch, the owned lists, the installs and uninstalls, the
//! time-to-beat lookups (timetobeat.rs) and the achievements
//! (achievements.rs), which post their results back to the event loop.

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
use canape_library::overrides::Overrides;
use canape_library::uninstall::{self, Uninstall};
use canape_library::{Game, Launch, Played, Roots, Source, eac, legendary, online, tools};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer, Timer,
    TimerMode, VecModel, Weak,
};

use crate::backdrop;
use crate::grid;
use crate::shelves::{self, Kind, Shelf};
use crate::timetobeat::{self, Lookups};
use crate::{
    AppWindow, CategoryView, DetailView, GameView, HiddenGameView, Library, OptionsView, ShelfView,
};

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
/// How long the clipboard is watched for GOG's sign-in address.
const GOG_WAIT: Duration = Duration::from_secs(10 * 60);
/// How often the clipboard is read meanwhile.
const GOG_POLL: Duration = Duration::from_secs(1);
/// Development: how long a pretend uninstall takes.
const FIXTURE_UNINSTALL: Duration = Duration::from_secs(3);
/// Development: the fake drives, and their free space.
const FIXTURE_DRIVES: [(&str, u64); 2] = [("C:\\", 21_400_000_000), ("D:\\", 48_240_000_000)];

/// The gamepad sends arrow keys (instead of Tab / Shift+Tab) while the
/// library has the focus: the grid moves in two dimensions.
static LIBRARY_FOCUSED: AtomicBool = AtomicBool::new(false);
static LIBRARY_SHOWN: AtomicBool = AtomicBool::new(false);
/// Another page that moves with the arrow keys has the focus.
static PAGE_ARROWS: AtomicBool = AtomicBool::new(false);
/// A text field of the game page's options has the focus: left and right
/// move in the text.
static TEXT_ENTRY: AtomicBool = AtomicBool::new(false);

/// True when the D-pad should send arrow keys.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn arrow_navigation() -> bool {
    (LIBRARY_SHOWN.load(Ordering::Relaxed)
        && (LIBRARY_FOCUSED.load(Ordering::Relaxed) || TEXT_ENTRY.load(Ordering::Relaxed)))
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
    /// Scan again once the running scan is done (games.json changed).
    rescan: bool,
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
    /// Steam installs not downloading yet: Steam may show a window meanwhile
    /// (its install dialog, or its start), so Canapé comes back to the front
    /// once the download starts.
    steam_dialogs: HashSet<String>,
    /// The game in front, as the foreground poll last saw it: Steam is not
    /// restarted for an install while one runs.
    foreground: Option<String>,
    /// Canapé's helper programs (Legendary).
    tools_dir: PathBuf,
    /// Clears the install or store error after a while.
    error_timer: Timer,
    /// A game just installed: once the scan lists it, the installed shelf
    /// comes to the front with it selected.
    just_installed: Option<String>,
    /// GOG's sign-in page is open in the browser: since when, and the
    /// clipboard's text then (an older address is not taken for the new).
    gog_wait: Option<(Instant, Option<String>)>,
    /// Reads the clipboard while `gog_wait` is set.
    gog_timer: Timer,
    /// Since when Canapé waits for Epic's sign-in code, and the clipboard's
    /// text then (a code already there is not a new one).
    epic_wait: Option<(Instant, Option<String>)>,
    /// Reads the clipboard while `epic_wait` is set.
    epic_timer: Timer,
    /// Bytes each owned game takes once installed, as its store said
    /// (`None`: the store could not tell).
    sizes: HashMap<String, Option<u64>>,
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
    /// Time to beat from IGDB.
    beat: Lookups,
    /// Releases the game page's images once it has faded out.
    detail_timer: Timer,
    /// When the backdrop last changed colour.
    backdrop_at: Option<Instant>,
    /// The backdrop's shape for the window size it was drawn for.
    backdrop_mask: Option<(u32, u32, backdrop::Mask)>,
    /// Games whose anti-cheat setup the user refused this session.
    eac_refused: HashSet<String>,
    /// The user's titles, categories and hidden games.
    overrides: Overrides,
    overrides_path: PathBuf,
    /// overrides.json could not be read: it is never overwritten.
    overrides_locked: bool,
    /// The categories the options menu lists, in its order.
    option_categories: Vec<String>,
    categories_model: Rc<VecModel<CategoryView>>,
    /// The hidden games as their sub-page lists them: ids.
    hidden: Vec<String>,
    hidden_model: Rc<VecModel<HiddenGameView>>,
    /// The game being uninstalled.
    uninstalling: Option<String>,
    /// Clears the last change's error after a while.
    change_timer: Timer,
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

/// Where the games come from: the real stores and `games.json` in `dir`, or
/// in a development build the fake machine of `CANAPE_LIBRARY_FIXTURE` (see
/// [`canape_library::Roots::fixture`]).
fn roots(dir: &Path, tools_dir: &Path) -> (Roots, bool) {
    match std::env::var_os("CANAPE_LIBRARY_FIXTURE") {
        Some(dir) if cfg!(debug_assertions) => (Roots::fixture(&PathBuf::from(dir)), true),
        _ => {
            let mut roots = Roots::system();
            roots.external = Some(dir.join("games.json"));
            let exe = tools::legendary_path(tools_dir);
            roots.legendary = legendary::config_dir()
                .filter(|_| exe.is_file())
                .map(|config| (config, exe));
            roots.gogdl = Some(install::gog_record(&gog_dir()));
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
        // A shelf still on its first game stays on its first game: games
        // arriving later (the owned list before the scan) do not leave the
        // selection in the middle.
        let previous: HashMap<Kind, String> = self
            .shelves
            .iter()
            .zip(&self.picks)
            .filter(|(_, i)| **i > 0)
            .filter_map(|(shelf, &i)| {
                shelf
                    .games
                    .get(i)
                    .map(|g| (shelf.kind.clone(), g.id.clone()))
            })
            .collect();
        // Hidden games stay out of the games to install too: the list of
        // owned games is taken before they are left out.
        let to_install = canape_library::not_installed(&self.owned, &self.all);
        let shown = self.overrides.apply(&self.all);
        let to_install = self.overrides.apply(&to_install);
        let shelves = shelves::build(&shown, to_install, &self.overrides.category_map());
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
        // The page is open on a game that left that shelf (taken out of a
        // category): the shelf that still has it, the installed games first.
        let has = |s: &Shelf, id: &str| s.games.iter().any(|g| g.id == id);
        let focus = match (&keep, &self.detail) {
            (Some((_, id)), Some(open)) if id == open && !has(&shelves[focus], id) => shelves
                .iter()
                .position(|s| s.kind == Kind::Installed && has(s, id))
                .or_else(|| shelves.iter().position(|s| has(s, id)))
                .unwrap_or(focus),
            _ => focus,
        };
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
            library.set_total(shown.len() as i32);
            library.set_shelf(self.focus as i32);
            library.set_first(self.firsts[self.focus] as i32);
            library.set_selected(self.picks[self.focus] as i32);
        }
        self.refresh_covers();
        self.show_backdrop();
        self.show_hidden();
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
        Some((shelf.kind.clone(), game.id.clone()))
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
        // A game to install from Epic or GOG: its size against the free
        // space of the games folder, once its store has told.
        let install = match &game.launch {
            Launch::Install { install } if !matches!(install, Install::Steam { .. }) => {
                Some(install.clone())
            }
            _ => None,
        };
        let folder = self.games_dir();
        let known = self.sizes.get(&game.id).copied().flatten();
        let size = known.or(game.size);
        library.set_detail_view(DetailView {
            cover: Image::default(),
            size_tenths: size.map_or(-1, tenths),
            free_tenths: if install.is_some() {
                self.free_space(&folder).map_or(-1, tenths)
            } else {
                -1
            },
            folder: folder.to_string_lossy().as_ref().into(),
            minutes: minutes.map_or(-1, |m| m as i32),
            beat_rushed: self.beat.all(&game.id).rushed,
            beat_minutes: self.beat.all(&game.id).main,
            beat_complete: self.beat.all(&game.id).complete,
            exe: self
                .recent
                .exes
                .get(&game.id)
                .cloned()
                .unwrap_or_default()
                .into(),
        });
        library.set_detail_art(Image::default());
        library.set_detail_part(0);
        library.set_achievement_focus(0);
        library.set_detail(true);
        self.detail = Some(game.id.clone());
        crate::achievements::open(&game.id, self.roots.steam.as_deref());
        if let Some(install) = install
            && size.is_none()
            && !self.fixture
            && !self.sizes.contains_key(&game.id)
        {
            self.fetch_size(&game.id, install);
        }
        self.fetch_time_to_beat(&game);

        let scale = ui.window().scale_factor();
        let width = ui.window().size().width.max(1);
        let hero = match (&self.roots.steam, game.id.strip_prefix("steam:")) {
            (Some(steam), Some(appid)) => appid
                .parse()
                .ok()
                .and_then(|appid| canape_library::steam::hero(steam, appid)),
            _ => None,
        };
        // An Epic game: Epic's own key art, downloaded once.
        let epic_wide = game
            .id
            .strip_prefix("epic:")
            .filter(|_| hero.is_none())
            .map(|app| {
                (
                    self.roots
                        .legendary
                        .as_ref()
                        .map(|(config, _)| config.clone()),
                    app.to_owned(),
                )
            });
        let covers_dir = self.covers_dir.clone();
        let online = !self.fixture && crate::i18n::ShellSettings::load().online_covers;
        let cover = game.cover.clone();
        let id = game.id;
        thread::spawn(move || {
            let hero = hero.or_else(|| {
                let (config, app) = epic_wide?;
                epic_wide_art(&covers_dir, &id, config.as_deref(), &app, online)
            });
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
            // Steam's art is about 3.1 times as wide as high; Epic's (16:9)
            // is cropped to the same band.
            let art = hero.and_then(|path| {
                canape_library::cover::load_filled(&path, width, (width as f32 / 3.1) as u32)
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

    /// Asks the store how much room the game `id` takes, then shows it on
    /// its page if still open.
    /// Looks the time to beat of `game` up when it is not cached, and
    /// shows it if its page is still open.
    fn fetch_time_to_beat(&mut self, game: &Game) {
        if !self.beat.start(&game.id, Instant::now()) {
            return;
        }
        // Looked up under the store's title, not the user's.
        let title = self
            .all
            .iter()
            .chain(&self.owned)
            .find(|g| g.id == game.id)
            .map_or_else(|| game.title.clone(), |g| g.title.clone());
        let id = game.id.clone();
        let (token, token_path) = self.beat.job();
        let epic_config = self
            .roots
            .legendary
            .as_ref()
            .map(|(config, _)| config.clone());
        thread::spawn(move || {
            // Epic: the catalog ids Legendary keeps, which IGDB may link.
            let extra = match (&epic_config, id.strip_prefix("epic:")) {
                (Some(config), Some(app)) => legendary::catalog_ids(config, app),
                _ => Vec::new(),
            };
            let outcome = timetobeat::lookup(&id, &title, &extra, token, &token_path);
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    let times = state.beat.finish(&id, outcome);
                    if state.detail.as_deref() != Some(id.as_str()) {
                        return;
                    }
                    if let Some(ui) = state.ui.upgrade() {
                        let library = ui.global::<Library>();
                        let mut view = library.get_detail_view();
                        if (view.beat_rushed, view.beat_minutes, view.beat_complete)
                            != (times.rushed, times.main, times.complete)
                        {
                            view.beat_rushed = times.rushed;
                            view.beat_minutes = times.main;
                            view.beat_complete = times.complete;
                            library.set_detail_view(view);
                        }
                    }
                })
            });
        });
    }

    fn fetch_size(&mut self, id: &str, install: Install) {
        let (id, tools) = (id.to_owned(), self.tools());
        thread::spawn(move || {
            let size = install::size_needed(&install, &tools)
                .map_err(|err| crate::log(&format!("library: size of {id}: {err}")))
                .ok()
                .flatten();
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    state.sizes.insert(id.clone(), size);
                    if state.detail.as_deref() != Some(id.as_str()) {
                        return;
                    }
                    if let (Some(ui), Some(size)) = (state.ui.upgrade(), size) {
                        let library = ui.global::<Library>();
                        let mut view = library.get_detail_view();
                        view.size_tenths = tenths(size);
                        library.set_detail_view(view);
                    }
                })
            });
        });
    }

    /// The folders holding the games installed by Legendary, gogdl and the
    /// Epic launcher, the fullest first.
    fn install_folders(&self) -> Vec<PathBuf> {
        let mut installs = Vec::new();
        if let Some((config, _)) = &self.roots.legendary {
            installs.extend(canape_library::legendary::install_paths(config));
        }
        if let Some(record) = &self.roots.gogdl {
            installs.extend(
                canape_library::gogdl::load_installed(record)
                    .into_iter()
                    .map(|e| e.path),
            );
        }
        if let Some(dir) = &self.roots.epic {
            installs.extend(
                canape_library::epic::installs(dir)
                    .into_iter()
                    .map(|(_, p)| p),
            );
        }
        canape_library::disk::parents(&installs)
    }

    /// The fixed drives and their free space.
    fn drives(&self) -> Vec<(PathBuf, u64)> {
        if self.fixture {
            return FIXTURE_DRIVES
                .iter()
                .map(|(root, free)| (PathBuf::from(root), *free))
                .collect();
        }
        canape_library::disk::fixed_drives()
            .into_iter()
            .filter_map(|root| Some((root.clone(), canape_library::disk::free_space(&root)?)))
            .collect()
    }

    /// Bytes free on the drive of `dir`.
    fn free_space(&self, dir: &Path) -> Option<u64> {
        if self.fixture {
            let text = dir.to_string_lossy().to_lowercase();
            return FIXTURE_DRIVES
                .iter()
                .find(|(root, _)| text.starts_with(&root.to_lowercase()))
                .map(|(_, free)| *free);
        }
        canape_library::disk::free_space(dir)
    }

    /// Where Legendary and gogdl install games: the folder set in
    /// shell.json, else the folder already holding most of them, else
    /// `Games` on the drive with the most free space.
    fn games_dir(&self) -> PathBuf {
        let set = crate::i18n::ShellSettings::load().games_folder;
        if !set.trim().is_empty() {
            return PathBuf::from(set.trim());
        }
        let existing = if self.fixture {
            Vec::new()
        } else {
            self.install_folders()
        };
        let dir = canape_library::disk::default_games_dir(&existing, &self.drives())
            .unwrap_or_else(|| {
                let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
                home.map(PathBuf::from)
                    .unwrap_or_default()
                    .join(canape_library::disk::GAMES)
            });
        self.fixture_path(dir)
    }

    /// Development off Windows: the fake drives' folders written as on
    /// Windows (`D:\Games`, not `D:\/Games`).
    fn fixture_path(&self, path: PathBuf) -> PathBuf {
        if self.fixture && !cfg!(windows) {
            PathBuf::from(path.to_string_lossy().replace("\\/", "\\"))
        } else {
            path
        }
    }

    /// Shows the games folder and its free space in Settings.
    fn show_games_folder(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        let dir = self.games_dir();
        library.set_games_folder(dir.to_string_lossy().as_ref().into());
        library.set_games_free_tenths(self.free_space(&dir).map_or(-1, tenths));
    }

    /// Left (-1) or right (1) on the games folder line: the previous or
    /// next folder offered, saved in shell.json.
    fn step_games_folder(&mut self, step: i32) {
        let current = self.games_dir();
        let existing = if self.fixture {
            Vec::new()
        } else {
            self.install_folders()
        };
        let roots: Vec<PathBuf> = self.drives().into_iter().map(|(root, _)| root).collect();
        let Some(next) = canape_library::disk::next_choice(&current, &existing, &roots, step)
        else {
            return;
        };
        let next = self.fixture_path(next);
        let mut settings = crate::i18n::ShellSettings::load();
        settings.games_folder = next.to_string_lossy().into_owned();
        if let Err(err) = settings.save() {
            crate::log(&format!("library: saving the games folder: {err}"));
        }
        self.show_games_folder();
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
        // The page may already be open: its profile shows from now on.
        if self.detail.as_deref() == Some(id.as_str()) {
            if let Some(ui) = self.ui.upgrade() {
                let library = ui.global::<Library>();
                let mut view = library.get_detail_view();
                view.exe = exe.into();
                library.set_detail_view(view);
            }
        }
        if let Err(err) = self.recent.save(&self.recent_path) {
            crate::log(&format!("library: saving recent launches: {err}"));
        }
    }

    /// Back to the shelf; the page's images go once it has faded out.
    fn close_detail(&mut self) {
        self.detail = None;
        crate::achievements::close();
        self.close_options();
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
            self.rescan = true;
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
            if let Some(path) = &roots.external
                && let Err(err) = canape_library::external::load(path)
            {
                crate::log(&format!("library: {}: {err}", path.display()));
            }
            let mut games = canape_library::scan(&roots);
            // Launcher installs handed to Legendary launch without the
            // launcher from now on: scan again to pick them up.
            if !fixture && adopt_epic_installs(&roots) {
                games = canape_library::scan(&roots);
            }
            // Launcher manifests left behind by an uninstall elsewhere.
            if !fixture && let Some(dir) = &roots.epic {
                let missing = canape_library::epic::missing(dir);
                games.retain(|g| {
                    !(matches!(g.launch, Launch::Uri { .. })
                        && missing.iter().any(|app| g.id == format!("epic:{app}")))
                });
            }
            online::attach(&mut games, &covers);
            crate::log(&format!(
                "library: {} games found in {} ms",
                games.len(),
                started.elapsed().as_millis()
            ));
            publish(&cache_path, games.clone());
            // A fixture never goes online.
            if !fixture && download_covers(&games, &epic_covers(&games, &roots), &covers) {
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
        // A refused anti-cheat prompt is not shown again before every
        // launch of the session.
        let anticheat = !fixture && !self.eac_refused.contains(&game.id);
        let roots = self.roots.clone();
        thread::spawn(move || {
            if anticheat && let Some(outcome) = ensure_anticheat(&game, &roots) {
                let (id, title) = (id.clone(), title.clone());
                let _ = slint::invoke_from_event_loop(move || {
                    with_state(|state| state.anticheat_done(&id, &title, outcome));
                });
            }
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

    /// A game just installed from Epic or GOG that ships Easy Anti-Cheat:
    /// its setup runs now (Windows asks first) rather than at its first
    /// launch.
    fn anticheat_after_install(&mut self, game: Game) {
        if self.fixture {
            return;
        }
        let roots = self.roots.clone();
        thread::spawn(move || {
            if let Some(outcome) = ensure_anticheat(&game, &roots) {
                let _ = slint::invoke_from_event_loop(move || {
                    with_state(|state| state.anticheat_done(&game.id, &game.title, outcome));
                });
            }
        });
    }

    /// The anti-cheat setup of `id` ended (already logged).
    fn anticheat_done(&mut self, id: &str, title: &str, outcome: eac::Outcome) {
        match outcome {
            eac::Outcome::Installed => {}
            eac::Outcome::Refused => {
                self.eac_refused.insert(id.to_owned());
                self.show_error(Failure::AntiCheatRefused, title, "");
            }
            eac::Outcome::Failed(why) => self.show_error(Failure::AntiCheatFailed, title, &why),
        }
    }

    /// What the installs need: Steam's folder, Legendary and gogdl, when
    /// present, and the games folder.
    fn tools(&self) -> Tools {
        let legendary = tools::legendary_path(&self.tools_dir);
        let gogdl = tools::gogdl_path(&self.tools_dir);
        Tools {
            steam: self.roots.steam.clone(),
            legendary: legendary.is_file().then_some(legendary),
            gogdl: gogdl.is_file().then(|| (gogdl, gog_dir())),
            games_dir: self.games_dir(),
            language: crate::i18n::current().to_owned(),
            game_running: self.foreground.is_some(),
        }
    }

    /// Starts installing `game` on its own thread.
    fn install(&mut self, game: &Game, mut install: Install) {
        if self.installing.contains_key(&game.id) {
            // A Steam install waiting on Steam: its dialog may sit behind
            // Canapé.
            if self.steam_dialogs.contains(&game.id) {
                raise_steam_dialog(&game.title);
            }
            return;
        }
        // Owned lists cached before Steam installs carried a name.
        if let Install::Steam { name, .. } = &mut install
            && name.is_empty()
        {
            name.clone_from(&game.title);
        }
        crate::log(&format!("library: installing {} ({install:?})", game.id));
        self.installing.insert(game.id.clone(), 0.0);
        if matches!(install, Install::Steam { .. }) {
            self.steam_dialogs.insert(game.id.clone());
        }
        self.show_progress(&game.id);
        if let Some(ui) = self.ui.upgrade() {
            clear_error(&ui);
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
                if self.steam_dialogs.remove(id) {
                    #[cfg(windows)]
                    crate::instance::bring_to_front();
                }
                self.installing.insert(id.to_owned(), share);
                self.show_progress(id);
            }
            Event::Done => {
                self.steam_dialogs.remove(id);
                crate::log(&format!("library: {id} installed"));
                self.show_games_folder();
                self.installing.remove(id);
                // The new game shows up in the installed list.
                self.just_installed = Some(id.to_owned());
                self.scan();
            }
            Event::Note(message) => {
                crate::log(&format!("library: installing {id}: {message}"));
            }
            Event::Failed(message) => {
                self.steam_dialogs.remove(id);
                crate::log(&format!("library: installing {id}: {message}"));
                self.installing.remove(id);
                self.show_progress(id);
                self.show_error(Failure::Install, title, &message);
            }
        }
    }

    /// Shows an install, store or launch error in the header for a while:
    /// a short translated message about `subject` (a store, a program, a
    /// game), with `detail` after it when short (see [`short_detail`]).
    /// The caller logs the full detail.
    fn show_error(&mut self, failure: Failure, subject: &str, detail: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        library.set_error_subject(subject.into());
        library.set_error_detail(short_detail(detail).as_str().into());
        library.set_error_kind(failure as i32);
        let ui = ui.as_weak();
        self.error_timer
            .start(TimerMode::SingleShot, ERROR_SHOWN, move || {
                if let Some(ui) = ui.upgrade() {
                    clear_error(&ui);
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
                fetch_owned(&settings, steam.as_deref(), &tools)
            };
            // A failed store keeps the last list rather than emptying it.
            if owned.errors.is_empty()
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
        library.set_igdb_keys(
            canape_library::igdb::Credentials::new(
                &settings.igdb_client_id,
                &settings.igdb_client_secret,
            )
            .is_some(),
        );
        library.set_owned_loading(self.owned_loading);
    }

    /// A on the Epic sign-in: downloads Legendary if needed and opens
    /// Epic's sign-in page in the browser, then watches the clipboard for
    /// the code the page shows. Pressed again while waiting, it reads the
    /// clipboard at once, else opens the page again.
    fn epic_login(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        if library.get_epic_busy() {
            return;
        }
        if self.epic_wait.is_some() {
            if !self.epic_check_clipboard() && !self.fixture {
                open_page(legendary::LOGIN_URL);
            }
            return;
        }
        if self.fixture {
            // Development: the waiting state, without a browser.
            self.epic_waiting(true);
            return;
        }
        library.set_epic_busy(true);
        let dir = self.tools_dir.clone();
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
            let result = ready
                .map_err(|err| (Failure::Download, "Legendary", err))
                .and_then(|_| {
                    canape_library::launch(&Launch::Uri {
                        uri: legendary::LOGIN_URL.to_owned(),
                    })
                    .map_err(|err| (Failure::SignIn, "Epic", err))
                });
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    if let Some(ui) = state.ui.upgrade() {
                        ui.global::<Library>().set_epic_busy(false);
                    }
                    match result {
                        Ok(()) => state.epic_waiting(true),
                        Err((failure, subject, err)) => {
                            crate::log(&format!("library: Epic sign-in: {err}"));
                            state.show_error(failure, subject, &err.to_string());
                        }
                    }
                })
            });
        });
    }

    /// Starts or stops watching the clipboard for Epic's sign-in code.
    fn epic_waiting(&mut self, on: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_epic_waiting(on);
        }
        if !on {
            self.epic_wait = None;
            self.epic_timer.stop();
            return;
        }
        self.epic_wait = Some((Instant::now(), crate::clipboard::text()));
        self.epic_timer.start(TimerMode::Repeated, GOG_POLL, || {
            with_state(|state| {
                if state
                    .epic_wait
                    .as_ref()
                    .is_some_and(|(at, _)| at.elapsed() > GOG_WAIT)
                {
                    state.epic_waiting(false);
                } else {
                    state.epic_check_clipboard();
                }
            })
        });
    }

    /// Signs in to Epic if the clipboard holds a new sign-in code.
    fn epic_check_clipboard(&mut self) -> bool {
        let Some((_, before)) = &self.epic_wait else {
            return false;
        };
        let Some(text) = crate::clipboard::text().filter(|t| Some(t) != before.as_ref()) else {
            return false;
        };
        let Some(code) = legendary::parse_auth_code(&text) else {
            return false;
        };
        self.epic_waiting(false);
        let exe = tools::legendary_path(&self.tools_dir);
        crate::log("library: Epic sign-in code found");
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_epic_busy(true);
        }
        thread::spawn(move || {
            let result = install::epic_login(&exe, &code);
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    if let Some(ui) = state.ui.upgrade() {
                        ui.global::<Library>().set_epic_busy(false);
                    }
                    if let Err(err) = result {
                        crate::log(&format!("library: Epic sign-in: {err}"));
                        state.show_error(Failure::SignIn, "Epic", &err.to_string());
                    }
                    state.refresh_owned(true);
                })
            });
        });
        true
    }

    /// A on the GOG sign-in: downloads gogdl if needed and opens GOG's
    /// sign-in page in the browser, then watches the clipboard for the
    /// address GOG lands on. Pressed again while waiting, it reads the
    /// clipboard at once, else opens the page again.
    fn gog_login(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        if library.get_gog_busy() {
            return;
        }
        if self.gog_wait.is_some() {
            if !self.gog_check_clipboard() && !self.fixture {
                open_page(&canape_library::gogdl::login_url());
            }
            return;
        }
        if self.fixture {
            // Development: the waiting state, without a browser.
            self.gog_waiting(true);
            return;
        }
        library.set_gog_busy(true);
        let dir = self.tools_dir.clone();
        thread::spawn(move || {
            let exe = tools::gogdl_path(&dir);
            #[cfg(windows)]
            let ready = if exe.is_file() {
                Ok(exe)
            } else {
                canape_library::http::WinHttp::new()
                    .and_then(|http| tools::fetch_gogdl(&http, &dir))
            };
            #[cfg(not(windows))]
            let ready: std::io::Result<PathBuf> = Err(std::io::Error::other(format!(
                "GOG sign-in needs Windows ({})",
                exe.display()
            )));
            let result = ready
                .map_err(|err| (Failure::Download, "gogdl", err))
                .and_then(|_| {
                    canape_library::launch(&Launch::Uri {
                        uri: canape_library::gogdl::login_url(),
                    })
                    .map_err(|err| (Failure::SignIn, "GOG", err))
                });
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    if let Some(ui) = state.ui.upgrade() {
                        ui.global::<Library>().set_gog_busy(false);
                    }
                    match result {
                        Ok(()) => state.gog_waiting(true),
                        Err((failure, subject, err)) => {
                            crate::log(&format!("library: GOG sign-in: {err}"));
                            state.show_error(failure, subject, &err.to_string());
                        }
                    }
                })
            });
        });
    }

    /// Starts or stops watching the clipboard for GOG's sign-in address.
    fn gog_waiting(&mut self, on: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_gog_waiting(on);
        }
        if !on {
            self.gog_wait = None;
            self.gog_timer.stop();
            return;
        }
        self.gog_wait = Some((Instant::now(), crate::clipboard::text()));
        self.gog_timer.start(TimerMode::Repeated, GOG_POLL, || {
            with_state(|state| {
                if state
                    .gog_wait
                    .as_ref()
                    .is_some_and(|(at, _)| at.elapsed() > GOG_WAIT)
                {
                    state.gog_waiting(false);
                } else {
                    state.gog_check_clipboard();
                }
            })
        });
    }

    /// Signs in to GOG if the clipboard holds a new sign-in address.
    fn gog_check_clipboard(&mut self) -> bool {
        let Some((_, before)) = &self.gog_wait else {
            return false;
        };
        let Some(text) = crate::clipboard::text().filter(|t| Some(t) != before.as_ref()) else {
            return false;
        };
        let Some(code) = canape_library::gogdl::parse_code(&text) else {
            return false;
        };
        self.gog_waiting(false);
        let Some((exe, dir)) = self.tools().gogdl else {
            return false;
        };
        crate::log("library: GOG sign-in code found");
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_gog_busy(true);
        }
        thread::spawn(move || {
            let result = install::gog_login(&exe, &dir, &code);
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| {
                    if let Some(ui) = state.ui.upgrade() {
                        ui.global::<Library>().set_gog_busy(false);
                    }
                    if let Err(err) = result {
                        crate::log(&format!("library: GOG sign-in: {err}"));
                        state.show_error(Failure::SignIn, "GOG", &err.to_string());
                    }
                    state.refresh_owned(true);
                })
            });
        });
        true
    }

    /// The game whose page is open, as shown (under the user's title) and
    /// as its store lists it.
    fn detail_game(&self) -> Option<(Game, Game)> {
        let id = self.detail.as_ref()?;
        let shown = self
            .shelves
            .iter()
            .flat_map(|s| &s.games)
            .find(|g| &g.id == id)?
            .clone();
        let raw = self
            .all
            .iter()
            .chain(&self.owned)
            .find(|g| &g.id == id)
            .cloned()
            .unwrap_or_else(|| shown.clone());
        Some((shown, raw))
    }

    /// Fills the options menu for the game whose page is open.
    fn show_options(&mut self) {
        let Some((_, raw)) = self.detail_game() else {
            return;
        };
        let Some(ui) = self.ui.upgrade() else { return };
        let plan = uninstall::plan(&raw, &self.roots);
        let custom = self.overrides.title(&raw.id).unwrap_or_default().to_owned();
        let mut entries = vec![0];
        if !custom.is_empty() {
            entries.push(1);
        }
        entries.extend([2, 3]);
        if plan.is_some() && self.uninstalling.is_none() && !self.installing.contains_key(&raw.id) {
            entries.push(4);
        }
        let folder = match &plan {
            Some(Uninstall::Gogdl { path, .. }) => path.clone(),
            Some(Uninstall::GogUninstaller { dir, .. }) => dir.clone(),
            _ => PathBuf::new(),
        };
        let mine = self.overrides.categories(&raw.id).to_vec();
        let mut names = self.overrides.all_categories();
        for name in &mine {
            if !names
                .iter()
                .any(|n| n.to_lowercase() == name.to_lowercase())
            {
                names.push(name.clone());
            }
        }
        names.sort_by_key(|n| n.to_lowercase());
        let views: Vec<CategoryView> = names
            .iter()
            .map(|name| CategoryView {
                name: name.as_str().into(),
                checked: mine.iter().any(|m| m.to_lowercase() == name.to_lowercase()),
            })
            .collect();
        self.categories_model.set_vec(views);
        self.option_categories = names;
        ui.global::<Library>().set_options_view(OptionsView {
            entries: ModelRc::new(VecModel::from(entries)),
            original: raw.title.as_str().into(),
            custom: custom.into(),
            categories: mine.join(" · ").into(),
            listed: raw.source == Source::Other,
            uninstall: uninstall_kind(plan.as_ref()),
            folder: folder.to_string_lossy().as_ref().into(),
        });
    }

    /// Y on the game page.
    fn open_options(&mut self) {
        self.show_options();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_options(true);
        }
    }

    fn close_options(&mut self) {
        text_entry(false);
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_options(false);
        }
    }

    /// Shows why the last change failed (see `Library.change-error`).
    fn change_error(&mut self, code: i32, title: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let library = ui.global::<Library>();
        library.set_change_error(code);
        library.set_uninstall_failed(title.into());
        let ui = ui.as_weak();
        self.change_timer
            .start(TimerMode::SingleShot, ERROR_SHOWN, move || {
                if let Some(ui) = ui.upgrade() {
                    ui.global::<Library>().set_change_error(0);
                }
            });
    }

    /// Writes overrides.json; false (and the error shown) if it failed.
    fn save_overrides(&mut self) -> bool {
        let result = if self.overrides_locked {
            Err(std::io::Error::other("unreadable, not overwritten"))
        } else {
            self.overrides.save(&self.overrides_path)
        };
        match result {
            Ok(()) => true,
            Err(err) => {
                crate::log(&format!(
                    "library: saving {}: {err}",
                    self.overrides_path.display()
                ));
                self.change_error(1, "");
                false
            }
        }
    }

    /// The shelves and the options menu after a change.
    fn changed(&mut self) {
        self.save_overrides();
        self.rebuild(None);
        self.show_options();
    }

    fn rename(&mut self, text: &str) {
        let Some((_, raw)) = self.detail_game() else {
            return;
        };
        if self.overrides.set_title(&raw.id, text, &raw.title) {
            crate::log(&format!("library: {} renamed", raw.id));
            self.changed();
        }
    }

    fn toggle_category(&mut self, index: usize) {
        let (Some((_, raw)), Some(name)) = (
            self.detail_game(),
            self.option_categories.get(index).cloned(),
        ) else {
            return;
        };
        self.overrides.toggle_category(&raw.id, &name);
        self.changed();
    }

    fn new_category(&mut self, text: &str) {
        let Some((_, raw)) = self.detail_game() else {
            return;
        };
        if self.overrides.add_category(&raw.id, text).is_some() {
            self.changed();
        }
    }

    /// Hides the game whose page is open, or takes a game of games.json
    /// out of the file; then back to the shelves.
    fn remove_game(&mut self) {
        let Some((_, raw)) = self.detail_game() else {
            return;
        };
        if raw.source == Source::Other {
            let Some(path) = self.roots.external.clone() else {
                return;
            };
            // An unreadable file is never overwritten: the user's games are in it.
            let result = canape_library::external::load(&path).and_then(|listed| {
                let left: Vec<_> = listed
                    .into_iter()
                    .filter(|e| canape_library::external::id(e) != raw.id)
                    .collect();
                canape_library::external::save(&path, &left).map(|_| left.len())
            });
            match result {
                Ok(count) => {
                    crate::log(&format!("library: {} taken out of games.json", raw.id));
                    if let Some(ui) = self.ui.upgrade() {
                        ui.global::<Library>().set_external(count as i32);
                    }
                    self.close_detail();
                    self.all.retain(|g| g.id != raw.id);
                    self.rebuild(None);
                    self.scan();
                }
                Err(err) => {
                    crate::log(&format!("library: {}: {err}", path.display()));
                    self.change_error(2, "");
                }
            }
            return;
        }
        self.overrides.set_hidden(&raw.id, true);
        crate::log(&format!("library: {} hidden", raw.id));
        self.save_overrides();
        self.close_detail();
        self.rebuild(None);
    }

    /// A on a hidden game: back on the shelves.
    fn show_game(&mut self, index: usize) {
        let Some(id) = self.hidden.get(index).cloned() else {
            return;
        };
        self.overrides.set_hidden(&id, false);
        crate::log(&format!("library: {id} shown again"));
        self.save_overrides();
        self.rebuild(None);
    }

    /// Lists the hidden games that are still installed or owned, by title.
    fn show_hidden(&mut self) {
        let mut list: Vec<(String, String, &'static str)> = self
            .overrides
            .hidden_ids()
            .into_iter()
            .filter_map(|id| {
                let game = self.all.iter().chain(&self.owned).find(|g| g.id == id)?;
                let title = self.overrides.title(id).unwrap_or(&game.title).to_owned();
                Some((id.to_owned(), title, game.source.label()))
            })
            .collect();
        list.sort_by_key(|(_, title, _)| title.to_lowercase());
        let views: Vec<HiddenGameView> = list
            .iter()
            .map(|(_, title, source)| HiddenGameView {
                title: title.as_str().into(),
                source: (*source).into(),
            })
            .collect();
        let ids: Vec<String> = list.into_iter().map(|(id, _, _)| id).collect();
        if ids != self.hidden || self.hidden_model.row_count() != views.len() {
            self.hidden_model.set_vec(views);
        } else {
            for (i, view) in views.into_iter().enumerate() {
                if self.hidden_model.row_data(i).is_none_or(|old| old != view) {
                    self.hidden_model.set_row_data(i, view);
                }
            }
        }
        self.hidden = ids;
    }

    /// Uninstalls the game whose page is open, on its own thread.
    fn uninstall(&mut self) {
        let Some((_, raw)) = self.detail_game() else {
            return;
        };
        if self.uninstalling.is_some() || self.installing.contains_key(&raw.id) {
            return;
        }
        let Some(plan) = uninstall::plan(&raw, &self.roots) else {
            return;
        };
        if matches!(plan, Uninstall::EpicLauncher { .. }) {
            return;
        }
        crate::log(&format!("library: uninstalling {} ({plan:?})", raw.id));
        let kind = uninstall_kind(Some(&plan));
        self.uninstalling = Some(raw.id.clone());
        self.close_options();
        if let Some(ui) = self.ui.upgrade() {
            let library = ui.global::<Library>();
            library.set_uninstalling(raw.id.as_str().into());
            library.set_uninstalling_kind(kind);
            library.set_change_error(0);
        }
        let (id, title, fixture, roots) = (
            raw.id.clone(),
            self.overrides
                .title(&raw.id)
                .unwrap_or(&raw.title)
                .to_owned(),
            self.fixture,
            self.roots.clone(),
        );
        thread::spawn(move || {
            let post = |event: Event| {
                let (id, title) = (id.clone(), title.clone());
                let _ = slint::invoke_from_event_loop(move || {
                    with_state(|state| state.uninstall_event(&id, &title, kind, event));
                });
            };
            if fixture {
                // Development: a pretend uninstall; the fake game stays.
                thread::sleep(FIXTURE_UNINSTALL);
                post(Event::Note("fixture: nothing removed".into()));
                post(Event::Done);
            } else {
                uninstall::run(&plan, &roots, post);
            }
        });
    }

    fn uninstall_event(&mut self, id: &str, title: &str, kind: i32, event: Event) {
        // Steam's dialog, the game's uninstaller or Windows' settings came
        // in front: Canapé comes back once they are done.
        let dialog = matches!(kind, 1 | 5 | 6);
        match event {
            Event::Progress(_) => return,
            Event::Note(message) => {
                crate::log(&format!("library: uninstalling {id}: {message}"));
                return;
            }
            Event::Done => {
                crate::log(&format!("library: {id} uninstalled"));
                if self.detail.as_deref() == Some(id) {
                    self.close_detail();
                }
                self.scan();
            }
            Event::Failed(message) => {
                crate::log(&format!("library: uninstalling {id}: {message}"));
                self.change_error(3, title);
            }
        }
        self.uninstalling = None;
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Library>().set_uninstalling("".into());
        }
        if dialog {
            #[cfg(windows)]
            crate::instance::bring_to_front();
        }
    }

    /// The launch returned: show the outcome, then put the game first.
    fn launched(&mut self, id: String, title: String, result: std::io::Result<()>) {
        if let Err(err) = &result {
            crate::log(&format!("library: launching {id}: {err}"));
            if let Some(ui) = self.ui.upgrade() {
                let library = ui.global::<Library>();
                library.set_launching("".into());
                library.set_failed(title.as_str().into());
            }
            self.show_error(Failure::Launch, &title, &err.to_string());
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

/// How a game is uninstalled, for the page: see `OptionsView.uninstall`.
fn uninstall_kind(plan: Option<&Uninstall>) -> i32 {
    match plan {
        None => 0,
        Some(Uninstall::Steam { .. }) => 1,
        Some(Uninstall::Legendary { .. }) => 2,
        Some(Uninstall::EpicLauncher { .. }) => 3,
        Some(Uninstall::Gogdl { .. }) => 4,
        Some(Uninstall::GogUninstaller { .. }) => 5,
        Some(Uninstall::Xbox { .. }) => 6,
    }
}

/// A text field gained or lost the focus: the touch keyboard follows.
fn text_entry(on: bool) {
    if TEXT_ENTRY.swap(on, Ordering::Relaxed) == on {
        return;
    }
    if on {
        crate::touchkb::show();
    } else {
        crate::touchkb::hide();
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
        .filter(|(app, path)| !taken.contains(&format!("epic:{app}")) && path.is_dir())
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
/// through Legendary, GOG through gogdl's token. Returns them, the art
/// links Epic and GOG gave, and the signed-in accounts.
fn fetch_owned(
    settings: &crate::i18n::ShellSettings,
    steam: Option<&Path>,
    tools: &Tools,
) -> Owned {
    let (mut owned, mut art, mut account) = (Vec::new(), HashMap::new(), None);
    let mut gog_account = None;
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
                errors.push(("Steam", err.to_string()));
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (key, steam);
    if let Some(exe) = &tools.legendary {
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
                        errors.push(("Epic", err.to_string()));
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                crate::log(&format!("library: Epic account: {err}"));
                errors.push(("Epic", err.to_string()));
            }
        }
    }
    if let Some((exe, dir)) = &tools.gogdl {
        match install::gog_credentials(exe, dir) {
            Ok(Some(creds)) => {
                #[cfg(windows)]
                {
                    use canape_library::online::Http;
                    let result = canape_library::http::WinHttp::new().and_then(|http| {
                        let name = http
                            .get(canape_library::gogdl::USER_URL, Some(&creds.access_token))
                            .ok()
                            .and_then(|json| canape_library::gogdl::parse_user(&json));
                        let games = canape_library::gogdl::fetch_owned(&http, &creds.access_token)?;
                        Ok((name, games))
                    });
                    match result {
                        Ok((name, games)) => {
                            gog_account = Some(name.unwrap_or_else(|| creds.user_id.clone()));
                            for item in games {
                                if let Some(url) = item.art_url {
                                    art.insert(item.game.id.clone(), url);
                                }
                                owned.push(item.game);
                            }
                        }
                        Err(err) => {
                            crate::log(&format!("library: GOG owned games: {err}"));
                            errors.push(("GOG", err.to_string()));
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    gog_account = Some(creds.user_id);
                }
            }
            Ok(None) => {}
            Err(err) => {
                crate::log(&format!("library: GOG account: {err}"));
                errors.push(("GOG", err.to_string()));
            }
        }
    }
    Owned {
        games: owned,
        art,
        account,
        gog_account,
        errors,
    }
}

/// What the stores answered.
#[derive(Clone, Default)]
struct Owned {
    games: Vec<Game>,
    /// Art links Epic and GOG gave, by game id.
    art: HashMap<String, String>,
    /// Signed-in Epic account.
    account: Option<String>,
    /// Signed-in GOG account.
    gog_account: Option<String>,
    /// The stores that could not be listed, and why (already logged).
    errors: Vec<(&'static str, String)>,
}

/// What failed, for the short translated message of the page
/// (`Library.error-kind` in library.slint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// A helper program (Legendary, gogdl) could not be downloaded.
    Download = 1,
    SignIn = 2,
    Install = 3,
    Launch = 4,
    /// A store's list of owned games.
    Owned = 5,
    /// The anti-cheat setup was refused at Windows' elevation prompt.
    AntiCheatRefused = 6,
    AntiCheatFailed = 7,
}

/// Runs `game`'s Easy Anti-Cheat setup, elevated once the user allows it,
/// when the game ships one and the anti-cheat service is missing; `None`
/// when nothing was needed. Blocks until the setup ends: run it on a
/// thread.
fn ensure_anticheat(game: &Game, roots: &Roots) -> Option<eac::Outcome> {
    #[cfg(windows)]
    {
        let setup = eac::setup_for(game, roots)?;
        if eac::service_installed() {
            return None;
        }
        crate::log(&format!(
            "library: {}: installing Easy Anti-Cheat ({} {})",
            game.id,
            setup.exe.display(),
            setup.args()
        ));
        let outcome = eac::install(&setup);
        crate::log(&format!(
            "library: {}: Easy Anti-Cheat: {outcome:?}",
            game.id
        ));
        Some(outcome)
    }
    #[cfg(not(windows))]
    {
        let _ = (game, roots);
        None
    }
}

/// Longest technical detail shown after a message, in characters.
const DETAIL_SHOWN: usize = 80;

/// The part of an error worth showing after its translated message: its
/// first line when short, without anything that looks like a key or a
/// token; "" otherwise (shell.log has it all).
fn short_detail(detail: &str) -> String {
    let line = detail.lines().map(str::trim).find(|l| !l.is_empty());
    let Some(line) = line else {
        return String::new();
    };
    let secret_like = line
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.len() >= 24);
    if secret_like || line.chars().count() > DETAIL_SHOWN {
        String::new()
    } else {
        line.to_owned()
    }
}

/// The owned lists that failed, as one message: the stores, and the
/// detail when only one failed.
fn owned_failure(errors: &[(&str, String)]) -> Option<(String, String)> {
    let stores: Vec<&str> = errors.iter().map(|(store, _)| *store).collect();
    let detail = match errors {
        [(_, detail)] => detail.clone(),
        _ => String::new(),
    };
    (!stores.is_empty()).then(|| (stores.join(", "), detail))
}

/// Takes the error off the header.
fn clear_error(ui: &AppWindow) {
    let library = ui.global::<Library>();
    library.set_error_kind(0);
    library.set_error_subject("".into());
    library.set_error_detail("".into());
}

fn post_owned(owned: Owned, covers_only: bool) {
    let _ = slint::invoke_from_event_loop(move || {
        with_state(|state| {
            state.owned = owned.games;
            state.art = owned.art;
            if !covers_only {
                state.owned_loading = false;
                state.owned_at = Some(Instant::now());
                if let Some((stores, detail)) = owned_failure(&owned.errors) {
                    state.show_error(Failure::Owned, &stores, &detail);
                }
            }
            if let Some(ui) = state.ui.upgrade() {
                let library = ui.global::<Library>();
                library.set_epic_account(owned.account.unwrap_or_default().into());
                library.set_gog_account(owned.gog_account.unwrap_or_default().into());
            }
            state.show_accounts();
            state.rebuild(None);
        })
    });
}

/// Canapé's GOG folder: gogdl's tokens and manifests, the install record.
fn gog_dir() -> PathBuf {
    canape_core::log::dir("LOCALAPPDATA").join("gog")
}

/// Bytes in tenths of a gigabyte, for the UI.
fn tenths(bytes: u64) -> i32 {
    (bytes as f64 / 1e8).round() as i32
}

/// Opens a sign-in page in the browser.
fn open_page(uri: &str) {
    let uri = uri.to_owned();
    thread::spawn(move || {
        if let Err(err) = canape_library::launch(&Launch::Uri { uri }) {
            crate::log(&format!("library: sign-in page: {err}"));
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
                && let Some(game) = state.all.iter().find(|g| g.id == id).cloned()
            {
                keep = Some((Kind::Installed, id));
                state.anticheat_after_install(game);
            }
            state.rebuild(keep);
            if let Some(ui) = state.ui.upgrade() {
                let library = ui.global::<Library>();
                library.set_scanning(false);
                library.set_ready(true);
            }
            if std::mem::take(&mut state.rescan) {
                state.scan();
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

/// Epic's key art for the game `id` (Legendary's app `app`): downloaded
/// earlier, else (when covers may be downloaded) from the link in
/// Legendary's metadata. Blocks: run it on a thread.
fn epic_wide_art(
    dir: &Path,
    id: &str,
    config: Option<&Path>,
    app: &str,
    download: bool,
) -> Option<PathBuf> {
    if let Some(path) = online::cached_wide(dir, id) {
        return Some(path);
    }
    let url = legendary::metadata_art(config.filter(|_| download)?, app).wide?;
    #[cfg(windows)]
    {
        let http = canape_library::http::WinHttp::new().ok()?;
        online::wide_art(dir, id, Some(&url), &http)
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        None
    }
}

/// Links to Epic's box art for the installed Epic games, from Legendary's
/// metadata: their cover when they have none.
fn epic_covers(games: &[Game], roots: &Roots) -> HashMap<String, String> {
    let Some((config, _)) = &roots.legendary else {
        return HashMap::new();
    };
    games
        .iter()
        .filter(|g| g.cover.is_none())
        .filter_map(|g| {
            let app = g.id.strip_prefix("epic:")?;
            Some((g.id.clone(), legendary::metadata_art(config, app).tall?))
        })
        .collect()
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
    let _ = slint::invoke_from_event_loop(move || {
        with_state(|state| {
            if let Some(exe) = &exe {
                state.learn_exe(exe);
            }
            state.foreground = exe;
        })
    });
}

/// What the list of games to add needs: where `games.json` is, every game
/// found, and whether the library is a development fixture.
pub fn snapshot() -> Option<(PathBuf, Vec<Game>, bool)> {
    let mut out = None;
    with_state(|state| {
        out = state
            .roots
            .external
            .clone()
            .map(|path| (path, state.all.clone(), state.fixture));
    });
    out
}

/// Scans again: `games.json` changed.
pub fn rescan() {
    with_state(State::scan);
}

/// Tells whether the library page is the one shown.
pub fn set_shown(shown: bool) {
    LIBRARY_SHOWN.store(shown, Ordering::Relaxed);
}

/// Shows the cached library, wires the page, and starts a scan.
pub fn start(ui: &AppWindow) {
    let dir = canape_core::log::dir("LOCALAPPDATA");
    let tools_dir = dir.join("tools");
    let (roots, fixture) = roots(&dir, &tools_dir);
    // A fixture must not overwrite the real cache.
    let suffix = if fixture { "-fixture" } else { "" };
    let cache_path = dir.join(format!("library{suffix}.json"));
    let recent_path = dir.join(format!("recent{suffix}.json"));
    let covers_dir = dir.join(format!("covers{suffix}"));
    let owned_path = dir.join(format!("owned{suffix}.json"));
    let beat = Lookups::new(&dir, suffix, fixture);
    crate::achievements::start(
        ui,
        &dir,
        suffix,
        fixture,
        canape_library::local_utc_offset(),
    );
    let owned = cache::load(&owned_path);
    let recent = Recent::load(&recent_path);
    let overrides_path = dir.join(format!("overrides{suffix}.json"));
    let (overrides, overrides_locked) = match Overrides::load(&overrides_path) {
        Ok(overrides) => (overrides, false),
        Err(err) => {
            crate::log(&format!("library: {}: {err}", overrides_path.display()));
            (Overrides::default(), true)
        }
    };
    let cached = cache::load(&cache_path);

    let shelf_model = Rc::new(VecModel::<ShelfView>::default());
    let categories_model = Rc::new(VecModel::<CategoryView>::default());
    let hidden_model = Rc::new(VecModel::<HiddenGameView>::default());
    let library = ui.global::<Library>();
    library.set_shelves(ModelRc::from(shelf_model.clone()));
    library.set_categories(ModelRc::from(categories_model.clone()));
    library.set_hidden_games(ModelRc::from(hidden_model.clone()));
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
        rescan: false,
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
        steam_dialogs: HashSet::new(),
        foreground: None,
        tools_dir,
        error_timer: Timer::default(),
        just_installed: None,
        gog_wait: None,
        epic_wait: None,
        epic_timer: Timer::default(),
        gog_timer: Timer::default(),
        sizes: HashMap::new(),
        tints: HashMap::new(),
        backdrop: None,
        backdrop_b_front: false,
        backdrop_mask: None,
        backdrop_at: None,
        detail: None,
        beat,
        detail_timer: Timer::default(),
        awaiting: None,
        eac_refused: HashSet::new(),
        overrides,
        overrides_path,
        overrides_locked,
        option_categories: Vec::new(),
        categories_model,
        hidden: Vec::new(),
        hidden_model,
        uninstalling: None,
        change_timer: Timer::default(),
    };
    state.rebuild(None);
    state.scan();
    // The shelf of games to install comes last.
    state.refresh_owned(false);
    state.show_accounts();
    state.show_games_folder();
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
    library.on_epic_login(|| with_state(State::epic_login));
    library.on_gog_login(|| with_state(State::gog_login));
    library.on_games_folder_step(|step| with_state(|state| state.step_games_folder(step)));
    library.on_open_detail(|index| {
        if let Ok(index) = usize::try_from(index) {
            with_state(|state| state.open_detail(index));
        }
    });
    library.on_close_detail(|| with_state(State::close_detail));
    library.on_open_options(|| with_state(State::open_options));
    library.on_close_options(|| with_state(State::close_options));
    library.on_rename(|text| with_state(|state| state.rename(&text)));
    library.on_reset_name(|| with_state(|state| state.rename("")));
    library.on_toggle_category(|index| {
        if let Ok(index) = usize::try_from(index) {
            with_state(|state| state.toggle_category(index));
        }
    });
    library.on_new_category(|text| with_state(|state| state.new_category(&text)));
    library.on_remove_game(|| with_state(State::remove_game));
    library.on_uninstall_game(|| with_state(State::uninstall));
    library.on_show_game(|index| {
        if let Ok(index) = usize::try_from(index) {
            with_state(|state| state.show_game(index));
        }
    });
    library.on_text_entry(text_entry);
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

/// Brings Steam's install dialog for the game `title` to the front.
#[cfg(windows)]
fn raise_steam_dialog(title: &str) {
    // Windows hands the foreground over only after input of our own.
    crate::buttons::tap_alt();
    let windows = canape_library::steam_queue::raise_dialog(title);
    crate::log(&format!("library: Steam's windows: {windows:?}"));
}

#[cfg(not(windows))]
fn raise_steam_dialog(_title: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_are_short_and_secret_free() {
        assert_eq!(
            short_detail("no checked Legendary release found\n"),
            "no checked Legendary release found"
        );
        assert_eq!(
            short_detail("\n  legendary: first\nsecond"),
            "legendary: first"
        );
        assert_eq!(short_detail(&"x ".repeat(60)), "");
        assert_eq!(
            short_detail("legendary auth: code 0123456789abcdef0123456789abcdef refused"),
            ""
        );
        assert_eq!(short_detail(""), "");
    }

    #[test]
    fn owned_failures_name_the_stores() {
        assert_eq!(owned_failure(&[]), None);
        assert_eq!(
            owned_failure(&[("Epic", "legendary list failed".into())]),
            Some(("Epic".into(), "legendary list failed".into()))
        );
        assert_eq!(
            owned_failure(&[("Steam", "a".into()), ("GOG", "b".into())]),
            Some(("Steam, GOG".into(), String::new()))
        );
    }
}
