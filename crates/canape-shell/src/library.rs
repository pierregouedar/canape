//! The library page: games from `canape-library`, shown at once from the
//! last scan's cache while a new scan runs in the background; covers
//! decoded on a worker thread at the size they are shown, only for the
//! games on screen and around them.
//!
//! A second shelf lists the games the user owns but has not installed
//! (Steam through the Web API key in shell.json, Epic through Legendary); A
//! installs them without opening the store's launcher. One shelf at a time
//! holds the model and its covers; the other only shows its first titles.
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
use canape_library::{Game, Launch, Played, Roots, Sort, online, tools};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel, Weak,
};

use crate::backdrop;
use crate::grid;
use crate::{AppWindow, GameView, Library};

/// How long "Launching…" shows: the game's window usually covers the shell
/// before that.
const LAUNCH_SHOWN: Duration = Duration::from_secs(5);
/// Corner radius of the cover tiles in library.slint, logical pixels.
const COVER_RADIUS: f32 = 10.0;
/// Coming back to the page rescans if the last scan is older than this.
const RESCAN_AFTER: Duration = Duration::from_secs(60);
/// How long an install or store error stays in the header.
const ERROR_SHOWN: Duration = Duration::from_secs(10);
/// The owned lists are asked for again after this.
const OWNED_REFRESH: Duration = Duration::from_secs(10 * 60);
/// Titles of the other shelf shown under the current one.
const OTHER_SHOWN: usize = 5;

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
    /// Showing the owned games that are not installed.
    to_install: bool,
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
    /// Colour of each decoded cover, for the backdrop.
    tints: HashMap<String, [u8; 3]>,
    /// Game whose colour the backdrop shows, and that colour.
    backdrop: Option<(String, [u8; 3])>,
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
    fn rebuild(&mut self, keep: Option<String>) {
        let to_install = canape_library::not_installed(&self.owned, &self.all);
        let (mut shown, mut other) = if self.to_install {
            (to_install, self.all.clone())
        } else {
            (self.all.clone(), to_install)
        };
        canape_library::sort(&mut shown, Sort::Recent);
        canape_library::sort(&mut other, Sort::Recent);
        let other_titles: Vec<SharedString> = other
            .iter()
            .take(OTHER_SHOWN)
            .map(|g| g.title.as_str().into())
            .collect();
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
                installed: !matches!(g.launch, Launch::Install { .. }),
                progress: self.installing.get(&g.id).copied().unwrap_or(-1.0),
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
            library.set_other_count(other.len() as i32);
            library.set_other_titles(ModelRc::new(VecModel::from(other_titles)));
        }
        self.refresh_covers();
        self.show_backdrop();
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

    /// Tints the page with the selected game's colour.
    fn show_backdrop(&mut self) {
        let Some(id) = self.selected_id() else { return };
        let color = self
            .tints
            .get(&id)
            .copied()
            .unwrap_or_else(|| backdrop::fallback(&id));
        if self.backdrop.as_ref() == Some(&(id.clone(), color)) {
            return;
        }
        let (pixels, width, height) = backdrop::render(color);
        if let Some(ui) = self.ui.upgrade() {
            let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&pixels, width, height);
            ui.global::<Library>()
                .set_backdrop(Image::from_rgba8(buffer));
        }
        self.backdrop = Some((id, color));
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
        let Some(game) = self.shown.get(index).cloned() else {
            return;
        };
        if let Launch::Install { install } = &game.launch {
            self.install(&game, install.clone());
            return;
        }
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
        let Some(i) = self.shown.iter().position(|g| g.id == id) else {
            return;
        };
        if let Some(mut row) = self.model.row_data(i) {
            row.progress = self.installing.get(id).copied().unwrap_or(-1.0);
            self.model.set_row_data(i, row);
        }
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
                    state.rebuild(Some(id.clone()));
                });
            });
    }
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
            if state.to_install {
                let keep = state.selected_id();
                state.rebuild(keep);
            }
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
        to_install: false,
        owned,
        art: HashMap::new(),
        owned_at: None,
        owned_loading: false,
        owned_path,
        installing: HashMap::new(),
        tools_dir,
        error_timer: Timer::default(),
        tints: HashMap::new(),
        backdrop: None,
    };
    state.rebuild(None);
    state.scan();
    // The shelf of games to install shows under the installed ones.
    state.refresh_owned(false);
    state.show_accounts();
    STATE.with(|s| *s.borrow_mut() = Some(state));

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
    library.on_select(|_| with_state(State::show_backdrop));
    library.on_set_mode({
        let ui = ui.as_weak();
        move |mode| {
            with_state(|state| {
                state.to_install = mode == 1;
                if state.to_install {
                    state.refresh_owned(false);
                }
                state.rebuild(None);
            });
            if let Some(ui) = ui.upgrade() {
                ui.global::<Library>().set_mode(mode);
            }
        }
    });
    library.on_epic_login(epic_login);
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
