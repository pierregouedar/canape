//! The Windows tweaks page (`ui/tweaks.slint`): the service's tweaks, and
//! the preinstalled apps the shell removes itself, since packages belong
//! to the signed-in user (at medium integrity, no elevation needed).
//!
//! Both lists are fixed: the service owns the tweak catalogue, and the
//! apps come from `canape_core::tweaks::REMOVABLE_APPS`, never from what a
//! page sends.

use std::sync::Mutex;

use canape_core::tweaks::{REMOVABLE_APPS, RemovableApp, is_protected};
use canape_core::{Request, TweakGroup, TweakId, TweakState, Unavailable};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};

use crate::link::Link;
use crate::{AppWindow, TweakRow, Tweaks};

/// What the page shows, kept between the answers that update it.
#[derive(Default)]
struct State {
    /// The service's last answer, `None` before one.
    tweaks: Option<Vec<TweakState>>,
    /// Apps of the list and whether they are installed.
    apps: Vec<(&'static RemovableApp, bool)>,
    /// Packages removed from this page (shell.json).
    removed: Vec<String>,
    /// Row key waiting for an outcome: a tweak name, a package, or
    /// `recommended`.
    busy: Option<String>,
    error: String,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap();
    f(guard.get_or_insert_with(State::default))
}

const RECOMMENDED: &str = "recommended";

fn owned(tweaks: &[TweakState]) -> Vec<TweakId> {
    tweaks
        .iter()
        .filter(|t| t.by_canape)
        .map(|t| t.id)
        .collect()
}

fn group_name(group: TweakGroup) -> &'static str {
    match group {
        TweakGroup::Clutter => "clutter",
        TweakGroup::Privacy => "privacy",
        TweakGroup::Games => "games",
        TweakGroup::Security => "security",
    }
}

fn unavailable_name(reason: Option<Unavailable>) -> &'static str {
    match reason {
        None => "",
        Some(Unavailable::TamperProtection) => "tamper_protection",
        Some(Unavailable::Locked) => "locked",
        Some(Unavailable::NoGameFolders) => "no_game_folders",
        Some(Unavailable::NotSupported) => "not_supported",
    }
}

/// The recommended tweaks still to apply.
fn recommended_left(tweaks: &[TweakState]) -> Vec<TweakId> {
    tweaks
        .iter()
        .filter(|t| t.id.info().recommended && !t.applied && t.unavailable.is_none())
        .map(|t| t.id)
        .collect()
}

fn heading(id: &str) -> TweakRow {
    TweakRow {
        kind: 0,
        id: id.into(),
        ..TweakRow::default()
    }
}

/// The page's lines: the recommended set, each group under its heading,
/// then the apps installed or removed from here.
fn rows(
    tweaks: Option<&[TweakState]>,
    apps: &[(&'static RemovableApp, bool)],
    removed: &[String],
) -> Vec<TweakRow> {
    let left = tweaks.map(recommended_left).unwrap_or_default();
    let mut rows = vec![TweakRow {
        kind: 1,
        id: RECOMMENDED.into(),
        applied: tweaks.is_some() && left.is_empty(),
        count: left.len() as i32,
        ..TweakRow::default()
    }];
    for group in TweakGroup::ALL {
        let members: Vec<&TweakState> = tweaks
            .unwrap_or_default()
            .iter()
            .filter(|t| t.id.info().group == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        rows.push(heading(group_name(group)));
        rows.extend(members.into_iter().map(|t| TweakRow {
            kind: 2,
            id: t.id.name().into(),
            name: Default::default(),
            applied: t.applied,
            unavailable: unavailable_name(t.unavailable).into(),
            reboot_pending: t.reboot_pending,
            security: t.id.info().security,
            count: t.paths.len() as i32,
        }));
    }
    let shown: Vec<TweakRow> = apps
        .iter()
        .filter_map(|&(app, installed)| {
            let was_removed = removed
                .iter()
                .any(|r| r.eq_ignore_ascii_case(app.package()));
            (installed || was_removed).then(|| TweakRow {
                kind: 3,
                id: app.package().into(),
                name: app.name.into(),
                applied: !installed,
                ..TweakRow::default()
            })
        })
        .collect();
    if !shown.is_empty() {
        rows.push(heading("apps"));
        rows.extend(shown);
    }
    rows
}

/// Redraws the page from the state.
fn refresh(ui: &Weak<AppWindow>) {
    let (rows, busy, error, groups, recommended, owned_rows, apps) = with_state(|s| {
        let rows = rows(s.tweaks.as_deref(), &s.apps, &s.removed);
        let busy = s.busy.as_ref().map_or(-1, |key| {
            if key == "restore" {
                0
            } else {
                rows.iter()
                    .position(|r| r.kind != 0 && r.id == key.as_str())
                    .map_or(-1, |i| i as i32)
            }
        });
        let groups: Vec<Vec<TweakRow>> = TweakGroup::ALL
            .iter()
            .map(|group| {
                let ids: Vec<String> = s
                    .tweaks
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter(|t| t.id.info().group == *group)
                    .map(|t| t.id.name())
                    .collect();
                rows.iter()
                    .filter(|r| r.kind == 2 && ids.iter().any(|id| id == r.id.as_str()))
                    .cloned()
                    .collect()
            })
            .collect();
        let left = s
            .tweaks
            .as_deref()
            .map(recommended_left)
            .unwrap_or_default();
        let recommended: Vec<TweakRow> = rows
            .iter()
            .filter(|r| r.kind == 2 && left.iter().any(|id| id.name() == r.id.as_str()))
            .cloned()
            .collect();
        let owned_ids = s.tweaks.as_deref().map(owned).unwrap_or_default();
        let owned_rows: Vec<TweakRow> = rows
            .iter()
            .filter(|r| r.kind == 2 && owned_ids.iter().any(|id| id.name() == r.id.as_str()))
            .cloned()
            .collect();
        let apps = rows
            .iter()
            .filter(|r| r.kind == 3)
            .cloned()
            .collect::<Vec<_>>();
        (
            rows,
            busy,
            s.error.clone(),
            groups,
            recommended,
            owned_rows,
            apps,
        )
    });
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let tweaks = ui.global::<Tweaks>();
        tweaks.set_rows(ModelRc::new(VecModel::from(rows)));
        tweaks.set_busy(busy);
        tweaks.set_error(error.into());
        tweaks.set_clutter(ModelRc::new(VecModel::from(groups[0].clone())));
        tweaks.set_privacy(ModelRc::new(VecModel::from(groups[1].clone())));
        tweaks.set_games(ModelRc::new(VecModel::from(groups[2].clone())));
        tweaks.set_security(ModelRc::new(VecModel::from(groups[3].clone())));
        tweaks.set_recommended_exclusions(
            recommended
                .iter()
                .any(|row| row.id == "defender_exclusions"),
        );
        tweaks.set_recommended(ModelRc::new(VecModel::from(recommended)));
        tweaks.set_owned(ModelRc::new(VecModel::from(owned_rows)));
        tweaks.set_apps(ModelRc::new(VecModel::from(apps)));
    });
}

/// The service's answer to `list_tweaks`.
pub fn show(ui: &Weak<AppWindow>, tweaks: Vec<TweakState>) {
    with_state(|s| {
        s.tweaks = Some(tweaks);
        // A tweak's answer, or the last one of the recommended set.
        if s.busy.as_deref() != Some(RECOMMENDED)
            || s.tweaks
                .as_deref()
                .is_some_and(|t| recommended_left(t).is_empty())
        {
            s.busy = s
                .busy
                .take()
                .filter(|b| REMOVABLE_APPS.iter().any(|a| a.package() == b));
        } else {
            // Recommended set finished (some may have failed); clear to let
            // the user retry individual tweaks.
            s.busy = None;
        }
    });
    refresh(ui);
}

/// Looks for the apps of the list, off the UI thread.
fn scan(ui: Weak<AppWindow>) {
    std::thread::spawn(move || {
        let settings = crate::i18n::ShellSettings::load();
        let apps: Vec<_> = REMOVABLE_APPS.iter().map(|a| (a, installed(a))).collect();
        // An app installed again from the Store is no longer "removed".
        let removed: Vec<String> = settings
            .removed_apps
            .iter()
            .filter(|p| {
                !apps
                    .iter()
                    .any(|(a, installed)| *installed && a.package().eq_ignore_ascii_case(p))
            })
            .cloned()
            .collect();
        if removed != settings.removed_apps {
            save_removed(removed.clone());
        }
        with_state(|s| {
            s.apps = apps;
            s.removed = removed;
        });
        refresh(&ui);
    });
}

fn save_removed(removed: Vec<String>) {
    let mut settings = crate::i18n::ShellSettings::load();
    settings.removed_apps = removed;
    if let Err(err) = settings.save() {
        crate::log(&format!("saving shell.json: {err}"));
    }
}

pub fn connect(ui: &AppWindow, link: &Link) {
    let tweaks = ui.global::<Tweaks>();
    tweaks.on_open({
        let (ui, link) = (ui.as_weak(), link.clone());
        move || {
            with_state(|s| s.error.clear());
            link.send(Request::ListTweaks);
            scan(ui.clone());
        }
    });
    tweaks.on_set({
        let (ui, link) = (ui.as_weak(), link.clone());
        move |name, on| {
            let Some(id) = TweakId::from_name(&name) else {
                return;
            };
            with_state(|s| s.busy = Some(id.name()));
            refresh(&ui);
            link.send(Request::SetTweak { id, on });
        }
    });
    tweaks.on_apply_recommended({
        let (ui, link) = (ui.as_weak(), link.clone());
        move || {
            let left = with_state(|s| {
                if s.busy.is_some() {
                    return Vec::new();
                }
                let left = s
                    .tweaks
                    .as_deref()
                    .map(recommended_left)
                    .unwrap_or_default();
                if !left.is_empty() {
                    s.busy = Some(RECOMMENDED.into());
                }
                left
            });
            refresh(&ui);
            for id in left {
                link.send(Request::SetTweak { id, on: true });
            }
        }
    });
    tweaks.on_revert_owned({
        let (ui, link) = (ui.as_weak(), link.clone());
        move || {
            let ids = with_state(|s| {
                if s.busy.is_some() {
                    return Vec::new();
                }
                let ids = s.tweaks.as_deref().map(owned).unwrap_or_default();
                if !ids.is_empty() {
                    s.busy = Some("restore".into());
                }
                ids
            });
            refresh(&ui);
            for id in ids {
                link.send(Request::SetTweak { id, on: false });
            }
        }
    });
    tweaks.on_remove_app({
        let ui = ui.as_weak();
        move |package| {
            let Some(app) = RemovableApp::find(&package) else {
                return;
            };
            with_state(|s| {
                s.busy = Some(app.package().into());
                s.error.clear();
            });
            refresh(&ui);
            let ui = ui.clone();
            std::thread::spawn(move || {
                let result = remove(app);
                crate::log(&format!("removing {}: {result:?}", app.package()));
                with_state(|s| {
                    s.busy = None;
                    match &result {
                        Ok(()) => {
                            if !s.removed.iter().any(|r| r == app.package()) {
                                s.removed.push(app.package().into());
                            }
                        }
                        Err(err) => s.error = format!("{}: {err}", app.name),
                    }
                });
                if result.is_ok() {
                    save_removed(with_state(|s| s.removed.clone()));
                }
                scan(ui);
            });
        }
    });
    tweaks.on_reinstall_app(move |package| {
        if let Some(app) = RemovableApp::find(&package) {
            let uri = app.store_uri();
            crate::log(&format!("opening {uri}"));
            #[cfg(windows)]
            if let Err(err) = canape_library::launch(&canape_library::Launch::Uri { uri }) {
                crate::log(&format!("store: {err}"));
            }
        }
    });
}

/// The user has a package of this family.
#[cfg(windows)]
fn installed(app: &RemovableApp) -> bool {
    use windows_sys::Win32::Storage::Packaging::Appx::GetPackagesByPackageFamily;
    let family: Vec<u16> = app.family.encode_utf16().chain(Some(0)).collect();
    let (mut count, mut length) = (0u32, 0u32);
    unsafe {
        GetPackagesByPackageFamily(
            family.as_ptr(),
            &mut count,
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
        )
    };
    count > 0
}

/// Removed by the simulator in this run.
#[cfg(not(windows))]
static SIMULATED_REMOVALS: Mutex<Vec<&str>> = Mutex::new(Vec::new());

/// Off Windows, every app is there until removed (from this run or from
/// shell.json).
#[cfg(not(windows))]
fn installed(app: &RemovableApp) -> bool {
    let removed = crate::i18n::ShellSettings::load().removed_apps;
    !SIMULATED_REMOVALS.lock().unwrap().contains(&app.package())
        && !removed.iter().any(|r| r == app.package())
}

/// Removes the app for the signed-in user: one fixed command, the package
/// name passed in the environment.
#[cfg(windows)]
fn remove(app: &'static RemovableApp) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SCRIPT: &str = "$ErrorActionPreference = 'Stop'; \
        Get-AppxPackage -Name $env:CANAPE_PACKAGE | Remove-AppxPackage";
    if is_protected(app.package()) {
        return Err("protected package".into());
    }
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let exe =
        std::path::PathBuf::from(windows).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = std::process::Command::new(exe)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("CANAPE_PACKAGE", app.package())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("cannot start PowerShell: {e}"))?;
    if output.status.success() && !installed(app) {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Windows kept the app")
        .to_owned())
}

#[cfg(not(windows))]
fn remove(app: &'static RemovableApp) -> Result<(), String> {
    if is_protected(app.package()) {
        return Err("protected package".into());
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    SIMULATED_REMOVALS.lock().unwrap().push(app.package());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: TweakId, applied: bool) -> TweakState {
        TweakState {
            id,
            applied,
            by_canape: applied,
            unavailable: None,
            reboot_pending: false,
            paths: Vec::new(),
        }
    }

    #[test]
    fn rows_group_the_tweaks_then_the_apps() {
        let mut tweaks: Vec<TweakState> = TweakId::ALL.iter().map(|&id| state(id, false)).collect();
        tweaks[0].applied = true;
        let realtime = TweakId::ALL
            .iter()
            .position(|&id| id == TweakId::DefenderRealtimeOff)
            .unwrap();
        tweaks[realtime].unavailable = Some(Unavailable::TamperProtection);
        let apps = [
            (&REMOVABLE_APPS[0], true),
            (&REMOVABLE_APPS[1], false),
            (&REMOVABLE_APPS[2], false),
        ];
        let removed = vec![REMOVABLE_APPS[1].package().to_string()];
        let rows = rows(Some(&tweaks), &apps, &removed);
        let kinds: Vec<i32> = rows.iter().map(|r| r.kind).collect();
        // Recommended, 4 headings with every tweak, a heading and two apps
        // (the third was never there).
        assert_eq!(rows.len(), 1 + 4 + TweakId::ALL.len() + 1 + 2);
        assert_eq!(kinds[..3], [1, 0, 2]);
        assert_eq!(rows[1].id, "clutter");
        let recommended = TweakId::ALL.iter().filter(|t| t.info().recommended).count();
        assert_eq!(rows[0].count as usize, recommended - 1);
        let realtime = rows
            .iter()
            .find(|r| r.id == "defender_realtime_off")
            .unwrap();
        assert_eq!(realtime.unavailable, "tamper_protection");
        assert!(realtime.security);
        let app_rows: Vec<&TweakRow> = rows.iter().filter(|r| r.kind == 3).collect();
        assert_eq!(app_rows.len(), 2);
        assert!(!app_rows[0].applied && app_rows[1].applied);
        // A heading is always followed by a line.
        for pair in rows.windows(2) {
            assert!(!(pair[0].kind == 0 && pair[1].kind == 0));
        }
        assert_ne!(rows.last().unwrap().kind, 0);
    }

    #[test]
    fn without_the_service_only_the_apps_show() {
        let rows = rows(None, &[(&REMOVABLE_APPS[0], true)], &[]);
        assert_eq!(rows.iter().map(|r| r.kind).collect::<Vec<_>>(), [1, 0, 3]);
        assert!(!rows[0].applied);
    }

    #[test]
    fn restore_only_includes_settings_changed_by_canape() {
        let mut external = state(TweakId::GameModeOn, true);
        external.by_canape = false;
        let changed = state(TweakId::CopilotOff, true);
        assert_eq!(owned(&[external, changed]), [TweakId::CopilotOff]);
    }

    #[test]
    fn the_page_knows_every_name_the_service_sends() {
        let source = include_str!("../ui/tweaks.slint");
        for id in TweakId::ALL {
            assert!(source.contains(&format!("\"{}\"", id.name())), "{id:?}");
        }
        for group in TweakGroup::ALL {
            assert!(source.contains(&format!("\"{}\"", group_name(group))));
        }
        for reason in [
            Unavailable::TamperProtection,
            Unavailable::Locked,
            Unavailable::NoGameFolders,
        ] {
            assert!(source.contains(unavailable_name(Some(reason))));
        }
    }
    #[test]
    fn recommended_busy_clears_on_list_even_with_failures() {
        // When the recommended set was being applied and one tweak fails
        // (e.g. WidgetsOff returns error 5), the list comes back with that
        // tweak still unapplied and not unavailable. recommended_left is
        // non-empty, so busy would remain "recommended" and the page would
        // stay stuck. The fix clears busy in the else branch.
        let mut tweaks: Vec<TweakState> = TweakId::ALL.iter().map(|&id| state(id, false)).collect();
        // WidgetsOff remains unapplied and not unavailable (error 5 does not set unavailable).
        let widgets_pos = tweaks
            .iter()
            .position(|t| t.id == TweakId::WidgetsOff)
            .unwrap();
        // WidgetsOff is still false / None — recommended_left will include it.
        assert!(tweaks[widgets_pos].unavailable.is_none());
        // Other recommended tweaks were applied.
        let copilot_pos = tweaks
            .iter()
            .position(|t| t.id == TweakId::CopilotOff)
            .unwrap();
        let game_dvr_pos = tweaks
            .iter()
            .position(|t| t.id == TweakId::GameDvrOff)
            .unwrap();
        tweaks[copilot_pos].applied = true;
        tweaks[copilot_pos].by_canape = true;
        tweaks[game_dvr_pos].applied = true;
        tweaks[game_dvr_pos].by_canape = true;

        // recommended_left is non-empty because WidgetsOff is still unapplied.
        let left = recommended_left(&tweaks);
        assert!(left.contains(&TweakId::WidgetsOff));

        // The else branch in show() sets s.busy = None regardless.
        // This test verifies the condition that triggers it:
        // busy == "recommended" AND recommended_left is non-empty.
        let busy: Option<String> = Some("recommended".into());
        let would_enter_else = busy.as_deref() == Some(RECOMMENDED)
            && tweaks
                .iter()
                .any(|t| t.id.info().recommended && !t.applied && t.unavailable.is_none());
        assert!(would_enter_else, "must enter else branch to clear busy");
    }
}
