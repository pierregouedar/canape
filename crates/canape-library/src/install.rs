//! Installing an owned game without opening its store's launcher.
//!
//! - **Steam**: `steam.exe -silent steam://install/<appid>`. Steam shows its
//!   small install dialog (folder, shortcuts), then downloads in the
//!   background; the progress is read from the game's manifest.
//! - **Epic**: `legendary install <app> -y`, progress read from its output.
//!
//! [`run`] blocks until the install ends: run it on its own thread.

use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::steam::{self, Download};
use crate::{Launch, legendary};

/// What installing an owned game takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "store", rename_all = "snake_case")]
pub enum Install {
    Steam { appid: u32 },
    Epic { app: String },
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
            Some(exe) => run_legendary(exe, app, &mut on),
            None => Err(io::Error::other("Legendary is not available")),
        },
    };
    on(match result {
        Ok(()) => Event::Done,
        Err(err) => Event::Failed(err.to_string()),
    });
}

fn run_steam(steam: &Path, appid: u32, on: &mut impl FnMut(Event)) -> io::Result<()> {
    let uri = format!("steam://install/{appid}");
    let exe = steam.join("steam.exe");
    // Through steam.exe: if Steam is not running yet it starts in the
    // tray (-silent) instead of opening its window; if it is, the URI goes
    // to the running client.
    let launch = if exe.is_file() {
        Launch::Exe {
            path: exe,
            args: format!("-silent {uri}"),
            cwd: steam.to_path_buf(),
        }
    } else {
        Launch::Uri { uri }
    };
    crate::launch(&launch)?;

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

fn run_legendary(exe: &Path, app: &str, on: &mut impl FnMut(Event)) -> io::Result<()> {
    let mut child = legendary_command(exe)
        .args(["install", app, "-y", "--skip-sdl"])
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

/// The signed-in Epic account, if any.
pub fn epic_account(exe: &Path) -> io::Result<Option<String>> {
    Ok(legendary::parse_account(&legendary_json(
        exe,
        &["status", "--json", "--offline"],
    )?))
}

/// The Epic games the account owns (asks Epic: takes a few seconds).
pub fn epic_owned(exe: &Path) -> io::Result<Vec<legendary::Owned>> {
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
