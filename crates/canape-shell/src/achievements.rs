//! Achievements on the game page, below the fold: Steam's, with the Web API
//! key of shell.json (`steam_api_key`) and the account signed in to Steam
//! (see [`canape_library::achievements`]). Other stores: none.
//!
//! Lists are kept in `achievements.json` per game id and shown at once
//! when the page opens; a list older than an hour, or in another language,
//! is asked for again on a thread, at most one request chain per game at a
//! time, none for [`PAUSE`] after a network error. A game without
//! achievements, or a private profile, shows nothing and logs one line.
//!
//! Icons are downloaded to a folder next to the cache and decoded at the
//! size shown, only for the tiles in view, by [`WORKERS`] threads.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use canape_library::achievements::{self, Achievement, Answer, Cache};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel, Weak,
};

use crate::{AchievementView, AppWindow, Library};

/// Icons downloaded or decoded at once.
const WORKERS: usize = 3;
/// No list is asked for this long after a network error.
const PAUSE: Duration = Duration::from_secs(10 * 60);
/// Corner radius of an icon in library.slint, as a share of its side.
const ICON_ROUNDING: f32 = 0.16;

/// An icon to show: where it comes from, where it is kept, and its size.
struct Job {
    game: String,
    url: String,
    path: PathBuf,
    size: u32,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    /// URLs a worker is on.
    busy: HashSet<String>,
}

/// The icon workers' queue.
struct Icons {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// A development fixture never downloads.
    offline: bool,
}

struct State {
    ui: Weak<AppWindow>,
    cache: Cache,
    cache_path: PathBuf,
    icons_dir: PathBuf,
    offline: bool,
    /// The game whose page is open.
    current: Option<String>,
    /// Its achievements, in the model's order.
    list: Vec<Achievement>,
    model: Rc<VecModel<AchievementView>>,
    /// Decoded icons of the open page, by URL.
    images: HashMap<String, Image>,
    /// Icons that could not be had, by URL, not asked again on this page.
    failed: HashSet<String>,
    /// Games being asked for.
    fetching: HashSet<String>,
    paused_until: Option<Instant>,
    icons: Arc<Icons>,
    /// The tiles in view: first index, count, icon side in pixels.
    view: (usize, usize, u32),
    /// Local time offset, seconds, for the unlock dates.
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

/// `YYYY-MM-DD` of unix seconds shifted by `offset`.
fn date(unix: u64, offset: i64) -> String {
    let days = (unix as i64 + offset).div_euclid(86_400);
    // Days to civil date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// What a tile shows.
fn view(a: &Achievement, icon: Option<&Image>, offset: i64) -> AchievementView {
    AchievementView {
        name: a.name.as_str().into(),
        description: a.description.as_str().into(),
        hidden: a.hidden,
        icon: icon.cloned().unwrap_or_default(),
        unlocked: a.unlocked.is_some(),
        date: a
            .unlocked
            .filter(|t| *t > 0)
            .map(|t| date(t, offset))
            .unwrap_or_default()
            .into(),
        rarity_tenths: a.percent.map_or(-1, |p| (p * 10.0).round() as i32),
    }
}

impl State {
    /// Shows `list` for the open page.
    fn show(&mut self, list: Vec<Achievement>) {
        self.list = list;
        let rows: Vec<_> = self
            .list
            .iter()
            .map(|a| view(a, self.images.get(a.shown_icon()), self.offset))
            .collect();
        self.model.set_vec(rows);
        if let Some(ui) = self.ui.upgrade() {
            let unlocked = self.list.iter().filter(|a| a.unlocked.is_some()).count();
            ui.global::<Library>()
                .set_achievements_unlocked(unlocked as i32);
        }
        self.queue_icons();
    }

    /// Asks the workers for the icons in view, and only those.
    fn queue_icons(&mut self) {
        let Some(game) = self.current.clone() else {
            return;
        };
        let (first, count, size) = self.view;
        let mut queue = self.icons.queue.lock().unwrap();
        queue.jobs.clear();
        if size == 0 {
            return;
        }
        let end = (first + count).min(self.list.len());
        for a in self.list.get(first.min(end)..end).unwrap_or_default() {
            let url = a.shown_icon();
            if url.is_empty()
                || self.images.contains_key(url)
                || self.failed.contains(url)
                || queue.busy.contains(url)
                || queue.jobs.iter().any(|j| j.url == url)
            {
                continue;
            }
            queue.jobs.push_back(Job {
                game: game.clone(),
                url: url.to_owned(),
                path: self.icons_dir.join(achievements::icon_file(url)),
                size,
            });
        }
        drop(queue);
        self.icons.wake.notify_all();
    }

    fn icon_ready(&mut self, game: &str, url: &str, image: Option<Image>) {
        if self.current.as_deref() != Some(game) {
            return;
        }
        let Some(image) = image else {
            self.failed.insert(url.to_owned());
            return;
        };
        for (i, a) in self.list.iter().enumerate() {
            if a.shown_icon() == url
                && let Some(mut row) = self.model.row_data(i)
            {
                row.icon = image.clone();
                self.model.set_row_data(i, row);
            }
        }
        self.images.insert(url.to_owned(), image);
    }

    /// Asks Steam for the list of `id` when it is not fresh.
    fn fetch(&mut self, id: &str, steam: Option<&Path>, language: &'static str) {
        let now = Instant::now();
        let Some(appid) = id
            .strip_prefix("steam:")
            .and_then(|a| a.parse::<u32>().ok())
        else {
            return;
        };
        if self.offline
            || self.fetching.contains(id)
            || self.paused_until.is_some_and(|until| now < until)
            || self.cache.fresh(id, language, canape_library::now())
        {
            return;
        }
        let key = crate::i18n::ShellSettings::load().steam_api_key;
        let key = key.trim().to_owned();
        let Some(steam) = steam.map(Path::to_path_buf) else {
            return;
        };
        if key.is_empty() {
            return;
        }
        self.fetching.insert(id.to_owned());
        let id = id.to_owned();
        thread::spawn(move || {
            let result = canape_library::steam::local_steam_id(&steam)
                .ok_or_else(|| io::Error::other("no Steam account signed in"))
                .and_then(|steam_id| run(&key, steam_id, appid, language));
            let _ = slint::invoke_from_event_loop(move || {
                with_state(|state| state.fetched(&id, language, result));
            });
        });
    }

    fn fetched(&mut self, id: &str, language: &str, result: io::Result<Answer>) {
        self.fetching.remove(id);
        let list = match result {
            Ok(Answer::List(list)) => Some(list),
            Ok(Answer::Nothing(reason)) => {
                crate::log(&format!("library: achievements of {id}: {reason}"));
                None
            }
            Err(err) => {
                // A status or a system message: never the URL, which holds
                // the key.
                crate::log(&format!("library: achievements of {id}: {err}"));
                self.paused_until = Some(Instant::now() + PAUSE);
                return;
            }
        };
        self.cache
            .record(id, language, list.clone(), canape_library::now());
        if let Err(err) = self.cache.save(&self.cache_path) {
            crate::log(&format!("library: saving achievements.json: {err}"));
        }
        if self.current.as_deref() == Some(id) {
            self.show(list.unwrap_or_default());
        }
    }
}

#[cfg(windows)]
fn run(key: &str, steam_id: u64, appid: u32, language: &str) -> io::Result<Answer> {
    let http = canape_library::http::WinHttp::new()?;
    achievements::fetch(&http, key, steam_id, appid, language)
}

#[cfg(not(windows))]
fn run(_: &str, _: u64, _: u32, _: &str) -> io::Result<Answer> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no HTTP here"))
}

/// Downloads the icon of `job` if it is not on disk yet.
#[cfg(windows)]
fn download(job: &Job, http: &mut Option<canape_library::http::WinHttp>) -> io::Result<()> {
    use canape_library::online::Http;
    if http.is_none() {
        *http = Some(canape_library::http::WinHttp::new()?);
    }
    let Some(client) = http.as_ref() else {
        return Err(io::Error::other("no HTTP session"));
    };
    let mut last = io::Error::other("no address");
    let mut found = None;
    for url in achievements::icon_urls(&job.url) {
        match client.get_limited(&url, None, achievements::ICON_MAX_BYTES) {
            Ok(bytes) => {
                found = Some(bytes);
                break;
            }
            Err(err) => last = err,
        }
    }
    let bytes = found.ok_or(last)?;
    if let Some(dir) = job.path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = job.path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &job.path)
}

/// Takes jobs off the queue until the program ends.
fn worker(icons: Arc<Icons>) {
    #[cfg(windows)]
    let mut http = None;
    loop {
        let job = {
            let mut queue = icons.queue.lock().unwrap();
            loop {
                if let Some(job) = queue.jobs.pop_front() {
                    queue.busy.insert(job.url.clone());
                    break job;
                }
                queue = icons.wake.wait(queue).unwrap();
            }
        };
        let mut result = Ok(());
        if !job.path.is_file() {
            result = if icons.offline {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "not in the fixture",
                ))
            } else {
                #[cfg(windows)]
                {
                    download(&job, &mut http)
                }
                #[cfg(not(windows))]
                {
                    Err(io::Error::new(io::ErrorKind::Unsupported, "no HTTP here"))
                }
            };
        }
        let decoded = result
            .and_then(|()| canape_library::cover::load(&job.path, job.size, job.size))
            .map(|rgb| {
                let radius = job.size as f32 * ICON_ROUNDING;
                (
                    canape_library::cover::rounded(&rgb, radius),
                    rgb.width,
                    rgb.height,
                )
            });
        icons.queue.lock().unwrap().busy.remove(&job.url);
        let decoded = match decoded {
            Ok(decoded) => Some(decoded),
            Err(err) => {
                crate::log(&format!("library: achievement icon: {err}"));
                None
            }
        };
        let _ = slint::invoke_from_event_loop(move || {
            let image = decoded.map(|(rgba, w, h)| {
                Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    &rgba, w, h,
                ))
            });
            with_state(|state| state.icon_ready(&job.game, &job.url, image));
        });
    }
}

/// Wires the achievements of the game page. `suffix` is `-fixture` for a
/// development fixture, which reads its own files and never goes online.
pub fn start(ui: &AppWindow, dir: &Path, suffix: &str, offline: bool, offset: i64) {
    let cache_path = dir.join(format!("achievements{suffix}.json"));
    let icons = Arc::new(Icons {
        queue: Mutex::new(Queue::default()),
        wake: Condvar::new(),
        offline,
    });
    for _ in 0..WORKERS {
        let icons = icons.clone();
        thread::spawn(move || worker(icons));
    }
    let model = Rc::new(VecModel::<AchievementView>::default());
    let library = ui.global::<Library>();
    library.set_achievements(ModelRc::from(model.clone()));
    let state = State {
        ui: ui.as_weak(),
        cache: Cache::load(&cache_path),
        cache_path,
        icons_dir: dir.join(format!("achievement-icons{suffix}")),
        offline,
        current: None,
        list: Vec::new(),
        model,
        images: HashMap::new(),
        failed: HashSet::new(),
        fetching: HashSet::new(),
        paused_until: None,
        icons,
        view: (0, 0, 0),
        offset,
    };
    STATE.with(|s| *s.borrow_mut() = Some(state));
    library.on_achievements_view(|first, count, size| {
        let view = (
            usize::try_from(first).unwrap_or(0),
            usize::try_from(count).unwrap_or(0),
            size.round().clamp(0.0, 512.0) as u32,
        );
        with_state(|state| {
            if view.2 != state.view.2 {
                // Decoded for another size.
                state.images.clear();
            }
            state.view = view;
            state.queue_icons();
        });
    });
}

/// The page of `id` opened: its cached list at once, a fresh one later.
/// `steam` is Steam's folder, for the signed-in account.
pub fn open(id: &str, steam: Option<&Path>) {
    let language = achievements::steam_language(crate::i18n::current());
    with_state(|state| {
        state.current = Some(id.to_owned());
        state.images.clear();
        state.failed.clear();
        let cached = state
            .cache
            .games
            .get(id)
            .and_then(|e| e.list.clone())
            .unwrap_or_default();
        state.show(cached);
        state.fetch(id, steam, language);
    });
}

/// The page closed: its icons go.
pub fn close() {
    with_state(|state| {
        state.current = None;
        state.list.clear();
        state.images.clear();
        state.model.set_vec(Vec::new());
        state.icons.queue.lock().unwrap().jobs.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_local_calendar_days() {
        assert_eq!(date(0, 0), "1970-01-01");
        assert_eq!(date(1_700_000_000, 0), "2023-11-14");
        // 23:30 UTC on 2024-02-28, two hours east: the next day.
        assert_eq!(date(1_709_163_000, 7200), "2024-02-29");
        assert_eq!(date(1_709_163_000, -7200), "2024-02-28");
        assert_eq!(date(951_782_400, 0), "2000-02-29");
    }
}
