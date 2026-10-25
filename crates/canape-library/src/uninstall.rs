//! Uninstalling an installed game, each store's own way:
//!
//! - **Steam**: `steam.exe -silent steam://uninstall/<appid>`. Steam asks
//!   for its own confirmation; done once the game's manifest is gone.
//! - **Epic through Legendary**: `legendary uninstall <app> -y`, with no
//!   window. A game installed by the Epic Games Launcher and not handed to
//!   Legendary yet is first imported (`legendary import`), then
//!   uninstalled the same way. Legendary exits with 0 even when it could
//!   not uninstall: done once its list no longer has the game.
//! - **GOG through gogdl**: the game's folder is deleted (only if it holds
//!   the game's `goggame-<id>.info`) and Canapé's record updated.
//! - **GOG Galaxy and GOG's installers**: the game's own uninstaller
//!   (`unins000.exe` in its folder), through `ShellExecuteW`; Windows asks
//!   for the administrator's consent when the game sits in Program Files.
//!   Done once the game's registry key or uninstaller is gone.
//! - **Xbox app**: Windows' settings page of the package
//!   (`ms-settings:appsfeatures-app?<family name>`), where the user
//!   uninstalls it; done once the game's folder no longer lists it.
//!
//! Games of `games.json` are not uninstalled: they can only be taken out
//! of the list. [`run`] blocks until the uninstall ends: run it on its own
//! thread.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::install::{self, Event};
use crate::{Game, GogSource, Launch, Roots, Source, gog, gogdl, legendary, steam, xbox};

/// How an installed game is uninstalled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uninstall {
    Steam {
        appid: u32,
    },
    /// Through Legendary; `import`: a launcher install, in this folder, to
    /// hand to Legendary first.
    Legendary {
        app: String,
        import: Option<PathBuf>,
    },
    /// A launcher install while Legendary is not set up (no Epic sign-in
    /// in Canapé yet): nothing can be done from here.
    EpicLauncher {
        app: String,
    },
    /// Installed with gogdl in `path`.
    Gogdl {
        id: String,
        path: PathBuf,
    },
    /// Installed by GOG Galaxy or GOG's installer in `dir`.
    GogUninstaller {
        id: String,
        dir: PathBuf,
    },
    Xbox {
        family: String,
    },
}

/// How long the user has to confirm a store's own uninstall (Steam's
/// dialog, the game's uninstaller, Windows' settings).
const CONFIRM_WAIT: Duration = Duration::from_secs(15 * 60);
/// How often the end of an uninstall is checked.
const POLL: Duration = Duration::from_secs(3);

/// Windows' settings page of an app package.
pub fn settings_uri(family: &str) -> String {
    format!("ms-settings:appsfeatures-app?{family}")
}

/// The package family name of an Xbox app game, from its launch URI
/// (`shell:AppsFolder\<family>!<app>`).
fn xbox_family(launch: &Launch) -> Option<String> {
    let Launch::Uri { uri } = launch else {
        return None;
    };
    let rest = uri.strip_prefix(r"shell:AppsFolder\")?;
    let family = rest.split('!').next()?;
    (!family.is_empty()).then(|| family.to_owned())
}

/// Whether `launch` goes through Legendary.
fn through_legendary(launch: &Launch) -> bool {
    // Compared as text: a Windows path, read the same on every system.
    matches!(launch, Launch::Exe { path, .. }
        if path.to_string_lossy().to_lowercase().rsplit(['\\', '/']).next() == Some("legendary.exe"))
}

/// How `game` is uninstalled, found under `roots`; `None` for a game that
/// is not installed, or of `games.json`.
pub fn plan(game: &Game, roots: &Roots) -> Option<Uninstall> {
    if matches!(game.launch, Launch::Install { .. }) {
        return None;
    }
    let (_, store_id) = game.id.split_once(':')?;
    match game.source {
        Source::Steam => Some(Uninstall::Steam {
            appid: store_id.parse().ok()?,
        }),
        Source::Epic => {
            if through_legendary(&game.launch) {
                return Some(Uninstall::Legendary {
                    app: store_id.to_owned(),
                    import: None,
                });
            }
            let path = roots
                .epic
                .as_deref()
                .map(crate::epic::installs)
                .unwrap_or_default()
                .into_iter()
                .find(|(app, _)| app == store_id)
                .map(|(_, path)| path);
            match (&roots.legendary, path) {
                (Some(_), Some(path)) => Some(Uninstall::Legendary {
                    app: store_id.to_owned(),
                    import: Some(path),
                }),
                _ => Some(Uninstall::EpicLauncher {
                    app: store_id.to_owned(),
                }),
            }
        }
        Source::Gog => {
            let recorded = roots.gogdl.as_deref().and_then(|record| {
                gogdl::load_installed(record)
                    .into_iter()
                    .find(|e| e.id == store_id)
            });
            if let Some(entry) = recorded {
                return Some(Uninstall::Gogdl {
                    id: entry.id,
                    path: entry.path,
                });
            }
            gog::install_dir(&roots.gog, store_id).map(|dir| Uninstall::GogUninstaller {
                id: store_id.to_owned(),
                dir,
            })
        }
        Source::Xbox => xbox_family(&game.launch).map(|family| Uninstall::Xbox { family }),
        Source::Other => None,
    }
}

/// Whether `path` may be deleted as the gogdl install of `id`: a folder
/// at least two levels below a drive or root, holding the game's
/// `goggame-<id>.info`.
pub fn deletable(path: &Path, id: &str) -> bool {
    let depth = path
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();
    depth >= 2 && path.join(format!("goggame-{id}.info")).is_file()
}

/// Uninstalls `game` as `plan` says, calling `on` with its outcome
/// (`Event::Done` or `Event::Failed`) and notes for the log.
pub fn run(plan: &Uninstall, roots: &Roots, mut on: impl FnMut(Event)) {
    let result = match plan {
        Uninstall::Steam { appid } => match &roots.steam {
            Some(dir) => run_steam(dir, *appid, &mut on),
            None => Err(io::Error::other("Steam is not installed")),
        },
        Uninstall::Legendary { app, import } => match &roots.legendary {
            Some((config, exe)) => run_legendary(config, exe, app, import.as_deref(), &mut on),
            None => Err(io::Error::other("Legendary is not available")),
        },
        Uninstall::EpicLauncher { .. } => Err(io::Error::other(
            "Legendary is not available: sign in to Epic in Canapé first",
        )),
        Uninstall::Gogdl { id, path } => run_gogdl(roots.gogdl.as_deref(), id, path),
        Uninstall::GogUninstaller { id, dir } => run_gog_uninstaller(&roots.gog, id, dir, &mut on),
        Uninstall::Xbox { family } => run_xbox(&roots.xbox, family, &mut on),
    };
    on(match result {
        Ok(()) => Event::Done,
        Err(err) => Event::Failed(err.to_string()),
    });
}

/// Waits up to `limit` for `gone`.
fn wait_until(limit: Duration, mut gone: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        std::thread::sleep(POLL);
        if gone() {
            return true;
        }
    }
    false
}

fn run_steam(dir: &Path, appid: u32, on: &mut impl FnMut(Event)) -> io::Result<()> {
    crate::launch(&steam::silent(dir, format!("steam://uninstall/{appid}")))?;
    on(Event::Note("waiting for Steam's confirmation".into()));
    if wait_until(CONFIRM_WAIT, || !steam::has_manifest(dir, appid)) {
        Ok(())
    } else {
        Err(io::Error::other("Steam did not uninstall the game"))
    }
}

fn run_legendary(
    config: &Path,
    exe: &Path,
    app: &str,
    import: Option<&Path>,
    on: &mut impl FnMut(Event),
) -> io::Result<()> {
    let id = format!("epic:{app}");
    let listed = || legendary::scan(config, exe).iter().any(|g| g.id == id);
    if let Some(path) = import
        && !listed()
    {
        install::epic_import(exe, app, path)?;
        on(Event::Note("handed to Legendary to uninstall".into()));
    }
    let output = install::legendary_command(exe)
        .args(["uninstall", app, "-y"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned();
    if output.status.success() && !listed() {
        if !last.is_empty() {
            on(Event::Note(format!("legendary: {last}")));
        }
        Ok(())
    } else {
        Err(io::Error::other(format!("legendary uninstall: {last}")))
    }
}

fn run_gogdl(record: Option<&Path>, id: &str, path: &Path) -> io::Result<()> {
    if !deletable(path, id) {
        return Err(io::Error::other(format!(
            "{} does not hold goggame-{id}.info: not deleted",
            path.display()
        )));
    }
    let removed = std::fs::remove_dir_all(path);
    // Partly deleted: the game is gone all the same.
    let gone = !path.join(format!("goggame-{id}.info")).is_file();
    if gone && let Some(record) = record {
        gogdl::forget_install(record, id)?;
    }
    removed
}

fn run_gog_uninstaller(
    source: &GogSource,
    id: &str,
    dir: &Path,
    on: &mut impl FnMut(Event),
) -> io::Result<()> {
    let exe = gog::uninstaller(dir)
        .ok_or_else(|| io::Error::other(format!("no uninstaller in {}", dir.display())))?;
    crate::launch(&Launch::Exe {
        path: exe.clone(),
        args: String::new(),
        cwd: dir.to_path_buf(),
    })?;
    on(Event::Note(format!("{} started", exe.display())));
    if wait_until(CONFIRM_WAIT, || {
        gog::install_dir(source, id).is_none() || !exe.is_file()
    }) {
        Ok(())
    } else {
        Err(io::Error::other("the game's uninstaller did not finish"))
    }
}

fn run_xbox(roots: &[PathBuf], family: &str, on: &mut impl FnMut(Event)) -> io::Result<()> {
    crate::launch(&Launch::Uri {
        uri: settings_uri(family),
    })?;
    on(Event::Note(
        "Windows' settings opened on the package".into(),
    ));
    let id = format!("xbox:{family}");
    if wait_until(CONFIRM_WAIT, || {
        !roots
            .iter()
            .any(|root| xbox::scan(root).iter().any(|g| g.id == id))
    }) {
        Ok(())
    } else {
        Err(io::Error::other("the package was not uninstalled"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, source: Source, launch: Launch) -> Game {
        Game {
            id: id.into(),
            title: id.into(),
            source,
            cover: None,
            launch,
            last_played: None,
            size: None,
            playtime: None,
        }
    }

    fn uri(uri: &str) -> Launch {
        Launch::Uri { uri: uri.into() }
    }

    #[test]
    fn each_store_its_way() {
        let roots = Roots::default();
        let steam = game("steam:620", Source::Steam, uri("steam://rungameid/620"));
        assert_eq!(plan(&steam, &roots), Some(Uninstall::Steam { appid: 620 }));
        let legendary = game(
            "epic:Anemone",
            Source::Epic,
            Launch::Exe {
                path: r"C:\Canape\tools\legendary.exe".into(),
                args: "launch Anemone".into(),
                cwd: r"D:\Games\Anemone".into(),
            },
        );
        assert_eq!(
            plan(&legendary, &roots),
            Some(Uninstall::Legendary {
                app: "Anemone".into(),
                import: None
            })
        );
        // A launcher install, Legendary not set up.
        let launcher = game(
            "epic:Fortnite",
            Source::Epic,
            uri("com.epicgames.launcher://x"),
        );
        assert_eq!(
            plan(&launcher, &roots),
            Some(Uninstall::EpicLauncher {
                app: "Fortnite".into()
            })
        );
        let xbox = game(
            "xbox:Contoso.Racing_8wekyb3d8bbwe",
            Source::Xbox,
            uri(r"shell:AppsFolder\Contoso.Racing_8wekyb3d8bbwe!RacingApp"),
        );
        assert_eq!(
            plan(&xbox, &roots),
            Some(Uninstall::Xbox {
                family: "Contoso.Racing_8wekyb3d8bbwe".into()
            })
        );
        assert_eq!(
            settings_uri("Contoso.Racing_8wekyb3d8bbwe"),
            "ms-settings:appsfeatures-app?Contoso.Racing_8wekyb3d8bbwe"
        );
        // Not installed, or of games.json: nothing to uninstall.
        let other = game("other:1", Source::Other, uri("itch://x"));
        assert_eq!(plan(&other, &roots), None);
        let owned = game(
            "steam:1",
            Source::Steam,
            Launch::Install {
                install: install::Install::Steam {
                    appid: 1,
                    name: String::new(),
                },
            },
        );
        assert_eq!(plan(&owned, &roots), None);
    }

    #[test]
    fn launcher_installs_go_through_legendary_when_it_is_there() {
        let dir = crate::test_util::fixture("epic");
        let roots = Roots {
            epic: Some(dir),
            legendary: Some(("config".into(), "legendary.exe".into())),
            ..Roots::default()
        };
        let launcher = game(
            "epic:Fortnite",
            Source::Epic,
            uri("com.epicgames.launcher://x"),
        );
        assert_eq!(
            plan(&launcher, &roots),
            Some(Uninstall::Legendary {
                app: "Fortnite".into(),
                import: Some(r"C:\Program Files\Epic Games\Fortnite".into())
            })
        );
    }

    #[test]
    fn gog_installs_by_gogdl_or_galaxy() {
        let dir = std::env::temp_dir().join(format!("canape-uninstall-{}", std::process::id()));
        let games = dir.join("Games");
        let mine = games.join("Mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("goggame-1.info"), "{}").unwrap();
        let record = dir.join("installed.json");
        gogdl::record_install(
            &record,
            gogdl::Installed {
                id: "1".into(),
                path: mine.clone(),
                size: 0,
            },
        )
        .unwrap();
        let registry = dir.join("gog.json");
        std::fs::write(
            &registry,
            r#"[{"key": "2", "values": {"gameName": "B", "path": "C:\\GOG Games\\B"}}]"#,
        )
        .unwrap();
        let roots = Roots {
            gog: GogSource::File(registry),
            gogdl: Some(record.clone()),
            ..Roots::default()
        };
        let exe = |p: &str| Launch::Exe {
            path: p.into(),
            args: String::new(),
            cwd: PathBuf::new(),
        };
        let by_gogdl = game("gog:1", Source::Gog, exe("a.exe"));
        let plan_a = plan(&by_gogdl, &roots).unwrap();
        assert_eq!(
            plan_a,
            Uninstall::Gogdl {
                id: "1".into(),
                path: mine.clone()
            }
        );
        let by_galaxy = game("gog:2", Source::Gog, exe("b.exe"));
        assert_eq!(
            plan(&by_galaxy, &roots),
            Some(Uninstall::GogUninstaller {
                id: "2".into(),
                dir: r"C:\GOG Games\B".into()
            })
        );
        assert_eq!(
            plan(&game("gog:3", Source::Gog, exe("c.exe")), &roots),
            None
        );

        // A folder without the game's info file is never deleted.
        assert!(!deletable(&games, "1"));
        assert!(!deletable(Path::new("/"), "1"));
        assert!(deletable(&mine, "1"));
        let mut events = Vec::new();
        run(&plan_a, &roots, |e| events.push(e));
        assert_eq!(events, [Event::Done]);
        assert!(!mine.exists());
        assert!(games.exists());
        assert!(gogdl::load_installed(&record).is_empty());
        let mut events = Vec::new();
        run(&plan_a, &roots, |e| events.push(e));
        assert!(matches!(&events[..], [Event::Failed(_)]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
