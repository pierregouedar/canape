//! Installing an owned game without opening its store's launcher.
//!
//! - **Steam**: `steam.exe -silent steam://install/<appid>`. Steam shows its
//!   small install dialog (folder, shortcuts), then downloads in the
//!   background; the progress is read from the game's manifest.
//! - **Epic**: `legendary info` then `legendary install <app> -y` into the
//!   games folder, progress read from its output.
//! - **GOG**: `gogdl info` then `gogdl download`, progress read from its
//!   output; the install is recorded for the scan (see [`crate::gogdl`]).
//!
//! Epic and GOG installs check first that the game fits in the games
//! folder ([`size_needed`], [`crate::disk`]); Steam asks in its own dialog.
//!
//! [`run`] blocks until the install ends: run it on its own thread.

use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::gogdl::{self, Credentials};
use crate::steam::{self, Download};
use crate::{disk, legendary};

/// What installing an owned game takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "store", rename_all = "snake_case")]
pub enum Install {
    Steam { appid: u32 },
    Epic { app: String },
    Gog { id: String },
}

/// News from a running install.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Share done, 0 to 1.
    Progress(f32),
    Done,
    Failed(String),
}

/// What the installs need.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    /// Steam's install folder.
    pub steam: Option<PathBuf>,
    /// `legendary.exe`.
    pub legendary: Option<PathBuf>,
    /// gogdl's executable and Canapé's GOG folder (tokens, install record).
    pub gogdl: Option<(PathBuf, PathBuf)>,
    /// Where Legendary and gogdl install games.
    pub games_dir: PathBuf,
    /// Interface language (`fr`, `zh_CN`…): GOG games download in it when
    /// they have it.
    pub language: String,
}

/// How often a Steam manifest is read.
const STEAM_POLL: Duration = Duration::from_secs(2);
/// Steam has this long to create the manifest (the user confirms the
/// dialog first); after that the install counts as cancelled.
const STEAM_START: Duration = Duration::from_secs(10 * 60);

/// Installs `install`, calling `on` with its progress and outcome.
pub fn run(install: &Install, tools: &Tools, mut on: impl FnMut(Event)) {
    let result = match install {
        Install::Steam { appid } => match &tools.steam {
            Some(steam) => run_steam(steam, *appid, &mut on),
            None => Err(io::Error::other("Steam is not installed")),
        },
        Install::Epic { app } => match &tools.legendary {
            Some(exe) => run_legendary(exe, app, &tools.games_dir, &mut on),
            None => Err(io::Error::other("Legendary is not available")),
        },
        Install::Gog { id } => match &tools.gogdl {
            Some((exe, dir)) => run_gogdl(exe, dir, id, tools, &mut on),
            None => Err(io::Error::other("gogdl is not available")),
        },
    };
    on(match result {
        Ok(()) => Event::Done,
        Err(err) => Event::Failed(err.to_string()),
    });
}

fn run_steam(steam: &Path, appid: u32, on: &mut impl FnMut(Event)) -> io::Result<()> {
    // Through steam.exe: if Steam is not running yet it starts in the
    // tray (-silent) instead of opening its window; if it is, the URI goes
    // to the running client.
    crate::launch(&steam::silent(steam, format!("steam://install/{appid}")))?;

    let started = Instant::now();
    let mut seen = false;
    loop {
        std::thread::sleep(STEAM_POLL);
        match steam::download(steam, appid) {
            Some(Download::Installed) => return Ok(()),
            Some(Download::Progress(share)) => {
                seen = true;
                on(Event::Progress(share));
            }
            None if seen => return Err(io::Error::other("cancelled in Steam")),
            None if started.elapsed() > STEAM_START => {
                return Err(io::Error::other("Steam did not start the download"));
            }
            None => {}
        }
    }
}

/// A Legendary command that opens no console window.
pub fn legendary_command(exe: &Path) -> Command {
    hidden(exe)
}

/// A command for a helper program, without a console window or input.
fn hidden(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Bytes the game takes once installed, when its store tells before
/// installing (Epic and GOG; asks the store: a few seconds). `None` for
/// Steam, whose own dialog shows the size and the free space.
pub fn size_needed(install: &Install, tools: &Tools) -> io::Result<Option<u64>> {
    match install {
        Install::Steam { .. } => Ok(None),
        Install::Epic { app } => match &tools.legendary {
            Some(exe) => epic_size(exe, app).map(Some),
            None => Err(io::Error::other("Legendary is not available")),
        },
        Install::Gog { id } => match &tools.gogdl {
            Some((exe, dir)) => gog_info(exe, dir, id, &tools.language).map(|i| Some(i.disk_size)),
            None => Err(io::Error::other("gogdl is not available")),
        },
    }
}

/// Fails when `need` bytes do not fit in `games_dir`.
fn check_space(need: u64, games_dir: &Path) -> io::Result<()> {
    match disk::shortage(need, disk::free_space(games_dir), games_dir) {
        Some(why) => Err(io::Error::other(why)),
        None => Ok(()),
    }
}

/// Bytes an Epic game takes once installed (`legendary info`).
pub fn epic_size(exe: &Path, app: &str) -> io::Result<u64> {
    legendary::parse_info_size(&legendary_json(exe, &["info", app, "--json"])?)
        .ok_or_else(|| io::Error::other("legendary info: no size"))
}

fn run_legendary(
    exe: &Path,
    app: &str,
    games_dir: &Path,
    on: &mut impl FnMut(Event),
) -> io::Result<()> {
    check_space(epic_size(exe, app)?, games_dir)?;
    std::fs::create_dir_all(games_dir)?;
    let mut child = legendary_command(exe)
        .args(["install", app, "-y", "--skip-sdl", "--base-path"])
        .arg(games_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut last = String::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(share) = legendary::parse_progress(&line) {
                on(Event::Progress(share));
            } else if !line.trim().is_empty() {
                last = line;
            }
        }
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("legendary: {last}")))
    }
}

/// The output of a Legendary command that prints JSON.
fn legendary_json(exe: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let output = legendary_command(exe)
        .args(args)
        .stderr(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(io::Error::other(format!(
            "legendary {} failed ({})",
            args.join(" "),
            output.status
        )))
    }
}

/// Hands a game installed by the Epic Games Launcher to Legendary, which
/// then launches it without the launcher. The files stay where they are;
/// Legendary only records the install.
pub fn epic_import(exe: &Path, app: &str, path: &Path) -> io::Result<()> {
    let output = legendary_command(exe)
        .arg("import")
        .arg("--skip-dlcs")
        .arg(app)
        .arg(path)
        .stdout(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        Err(io::Error::other(format!("legendary import {app}: {last}")))
    }
}

/// The signed-in Epic account, if any.
pub fn epic_account(exe: &Path) -> io::Result<Option<String>> {
    Ok(legendary::parse_account(&legendary_json(
        exe,
        &["status", "--json", "--offline"],
    )?))
}

/// The Epic games the account owns (asks Epic: takes a few seconds).
pub fn epic_owned(exe: &Path) -> io::Result<Vec<crate::Owned>> {
    legendary::parse_list(&legendary_json(exe, &["list", "--json"])?)
        .ok_or_else(|| io::Error::other("legendary list: unreadable answer"))
}

/// Opens Legendary's Epic sign-in window and waits for it to close. Its
/// console stays visible: without WebView2 Legendary falls back to asking
/// for a code there.
pub fn epic_login(exe: &Path) -> io::Result<bool> {
    let status = Command::new(exe).arg("auth").status()?;
    Ok(status.success())
}

/// The tokens gogdl keeps, in Canapé's GOG folder `dir`.
pub fn gog_auth_file(dir: &Path) -> PathBuf {
    dir.join("auth.json")
}

/// Canapé's record of the games installed with gogdl.
pub fn gog_record(dir: &Path) -> PathBuf {
    dir.join("installed.json")
}

/// A gogdl command whose tokens and manifests live in `dir`.
fn gogdl_command(exe: &Path, dir: &Path) -> Command {
    let mut command = hidden(exe);
    command
        .env("GOGDL_CONFIG_PATH", dir)
        .arg("--auth-config-path")
        .arg(gog_auth_file(dir));
    command
}

/// The output of a gogdl command that prints JSON.
fn gogdl_json(exe: &Path, dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let output = gogdl_command(exe, dir).args(args).output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(io::Error::other(format!(
            "gogdl {}: {}",
            args.first().unwrap_or(&""),
            last_line(&output.stderr)
        )))
    }
}

/// The last non-empty line of a program's output.
fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// Trades the code of GOG's sign-in page for tokens, which gogdl keeps.
pub fn gog_login(exe: &Path, dir: &Path, code: &str) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let out = gogdl_json(exe, dir, &["auth", "--code", code])?;
    gogdl::parse_credentials(&out)
        .map(drop)
        .ok_or_else(|| io::Error::other("GOG refused the sign-in code"))
}

/// The signed-in account's tokens, refreshed if needed; `None` when nobody
/// is signed in or the session ended.
pub fn gog_credentials(exe: &Path, dir: &Path) -> io::Result<Option<Credentials>> {
    if !gog_auth_file(dir).is_file() {
        return Ok(None);
    }
    Ok(gogdl::parse_credentials(&gogdl_json(exe, dir, &["auth"])?))
}

/// What installing the GOG game `id` takes (asks GOG: a few seconds).
pub fn gog_info(exe: &Path, dir: &Path, id: &str, language: &str) -> io::Result<gogdl::Info> {
    let out = gogdl_json(
        exe,
        dir,
        &[
            "info",
            id,
            "--platform",
            "windows",
            "--lang",
            gogdl::LANGUAGE,
            "--skip-dlcs",
        ],
    )?;
    gogdl::parse_info(&out, language)
        .ok_or_else(|| io::Error::other("gogdl info: unreadable answer"))
}

fn run_gogdl(
    exe: &Path,
    dir: &Path,
    id: &str,
    tools: &Tools,
    on: &mut impl FnMut(Event),
) -> io::Result<()> {
    let games_dir = &tools.games_dir;
    let info = gog_info(exe, dir, id, &tools.language)?;
    check_space(info.disk_size, games_dir)?;
    std::fs::create_dir_all(games_dir)?;
    let mut child = gogdl_command(exe, dir)
        .args(["download", id, "--platform", "windows", "--lang"])
        .arg(&info.language)
        .arg("--skip-dlcs")
        .arg("--path")
        .arg(games_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // stdout only gets a line or two (a refusal for lack of space): read
    // on its own thread so that neither pipe can fill up and block gogdl.
    let stdout = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = io::Read::read_to_string(&mut out, &mut text);
            text
        })
    });
    let mut last = String::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(share) = gogdl::parse_progress(&line) {
                on(Event::Progress(share));
            } else if line.contains("ERROR") || line.contains("Error") {
                last = line;
            }
        }
    }
    let out = stdout
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    let status = child.wait()?;
    if !status.success() {
        let why = last_line(out.as_bytes());
        return Err(io::Error::other(format!(
            "gogdl: {}",
            if why.is_empty() { last } else { why }
        )));
    }
    let path = games_dir.join(&info.folder);
    if !path.join(format!("goggame-{id}.info")).is_file() {
        return Err(io::Error::other("gogdl: the download did not finish"));
    }
    gogdl::record_install(
        &gog_record(dir),
        gogdl::Installed {
            id: id.to_owned(),
            path,
            size: info.disk_size,
        },
    )
}
