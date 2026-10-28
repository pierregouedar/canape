//! Easy Anti-Cheat, the Epic Online Services edition, for the games that
//! ship its setup: `EasyAntiCheat\EasyAntiCheat_EOS_Setup.exe` next to
//! `EasyAntiCheat\Settings.json`, which names the game's `productid`.
//!
//! A store's launcher installs the anti-cheat service with such a game;
//! Legendary and gogdl only download the files, so the first launch fails.
//! The shell then runs `EasyAntiCheat_EOS_Setup.exe install <productid>`
//! elevated, which Windows asks the user to allow (UAC). It is never run by
//! the service: a SYSTEM process must not start a program that an
//! unprivileged user can place in a game folder. Once the
//! `EasyAntiCheat_EOS` service exists, nothing is asked again.
//!
//! Finding and reading the files is pure and tested; the service check
//! and the elevated run are Windows-only.

use std::path::{Path, PathBuf};

use crate::{Game, Launch, Roots, Source};

/// Name of the Windows service the setup installs.
pub const SERVICE: &str = "EasyAntiCheat_EOS";
/// The setup's folder in a game's folder.
pub const FOLDER: &str = "EasyAntiCheat";
pub const SETUP_EXE: &str = "EasyAntiCheat_EOS_Setup.exe";
pub const SETTINGS: &str = "Settings.json";

/// A game's anti-cheat setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub exe: PathBuf,
    pub product_id: String,
}

impl Setup {
    /// The setup's command line.
    pub fn args(&self) -> String {
        format!("install {}", self.product_id)
    }
}

/// A product id that is safe on a command line: letters, digits, `-`
/// and `_` only.
fn valid_product_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The `productid` of a `Settings.json` (key case ignored), if valid.
pub fn parse_settings(text: &str) -> Option<String> {
    let text = text.trim_start_matches('\u{feff}');
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let id = value
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("productid"))?
        .1
        .as_str()?
        .trim();
    valid_product_id(id).then(|| id.to_owned())
}

/// The entry of `dir` named `name`, ignoring case (Windows does; the
/// tests run on Linux).
fn entry(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
}

/// The anti-cheat setup in the game folder `dir`, if it has one.
pub fn find(dir: &Path) -> Option<Setup> {
    let folder = entry(dir, FOLDER)?;
    let exe = entry(&folder, SETUP_EXE).filter(|p| p.is_file())?;
    let settings = std::fs::read(entry(&folder, SETTINGS)?).ok()?;
    let product_id = parse_settings(&String::from_utf8_lossy(&settings))?;
    Some(Setup { exe, product_id })
}

/// Folders that may hold `game`'s files: where Legendary or gogdl put it,
/// where the Epic launcher did, the folder it starts in and the one above.
pub fn game_dirs(game: &Game, roots: &Roots) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match game.source {
        Source::Epic => {
            let app = game.id.strip_prefix("epic:").unwrap_or_default();
            if let Some((config, _)) = &roots.legendary {
                dirs.extend(crate::legendary::install_path(config, app));
            }
            if let Some(manifests) = &roots.epic {
                dirs.extend(
                    crate::epic::installs(manifests)
                        .into_iter()
                        .filter(|(a, _)| a == app)
                        .map(|(_, path)| path),
                );
            }
        }
        Source::Gog => {
            let id = game.id.strip_prefix("gog:").unwrap_or_default();
            if let Some(record) = &roots.gogdl {
                dirs.extend(
                    crate::gogdl::load_installed(record)
                        .into_iter()
                        .filter(|e| e.id == id)
                        .map(|e| e.path),
                );
            }
        }
        _ => {}
    }
    if let Launch::Exe { cwd, .. } = &game.launch
        && !cwd.as_os_str().is_empty()
    {
        dirs.push(cwd.clone());
        dirs.extend(cwd.parent().map(Path::to_path_buf));
    }
    dirs.dedup();
    dirs
}

/// `game`'s anti-cheat setup, if it ships one (Epic and GOG games only:
/// the other stores install it themselves).
pub fn setup_for(game: &Game, roots: &Roots) -> Option<Setup> {
    if !matches!(game.source, Source::Epic | Source::Gog) {
        return None;
    }
    game_dirs(game, roots).iter().find_map(|dir| find(dir))
}

/// How an elevated setup run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// The user said no at the elevation prompt.
    Refused,
    Failed(String),
}

/// Whether the anti-cheat service is installed. When Windows cannot tell
/// (no access to the service manager), it counts as installed: nothing is
/// asked rather than asked for nothing.
#[cfg(windows)]
pub fn service_installed() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, GetLastError};
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
    };

    let name: Vec<u16> = SERVICE.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return true;
        }
        let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
        let missing = service.is_null() && GetLastError() == ERROR_SERVICE_DOES_NOT_EXIST;
        if !service.is_null() {
            CloseServiceHandle(service);
        }
        CloseServiceHandle(manager);
        !missing
    }
}

/// Longest wait for the setup.
#[cfg(windows)]
const SETUP_WAIT_MS: u32 = 5 * 60 * 1000;

/// Runs `setup` elevated (Windows asks the user first) and waits for it.
/// One run at a time; a run that finds the service already installed (by
/// a run that was waiting before it) asks nothing.
#[cfg(windows)]
pub fn install(setup: &Setup) -> Outcome {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, GetLastError, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    if service_installed() {
        return Outcome::Installed;
    }
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let verb = wide("runas");
    let file = wide(&setup.exe.to_string_lossy());
    let args = wide(&setup.args());
    let dir = setup
        .exe
        .parent()
        .map(|d| wide(&d.to_string_lossy()))
        .unwrap_or_else(|| wide(""));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: args.as_ptr(),
        lpDirectory: dir.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    let com = unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    let started = unsafe { ShellExecuteExW(&mut info) } != 0;
    let error = unsafe { GetLastError() };
    if com >= 0 {
        unsafe { CoUninitialize() };
    }
    if !started {
        return if error == ERROR_CANCELLED {
            Outcome::Refused
        } else {
            Outcome::Failed(format!("could not start the setup (error {error})"))
        };
    }
    if info.hProcess.is_null() {
        return Outcome::Failed("no process handle".into());
    }
    let outcome = unsafe {
        if WaitForSingleObject(info.hProcess, SETUP_WAIT_MS) != WAIT_OBJECT_0 {
            Outcome::Failed("the setup did not finish in 5 minutes".into())
        } else {
            let mut code = 0u32;
            if GetExitCodeProcess(info.hProcess, &mut code) == 0 {
                Outcome::Failed("unknown exit code".into())
            } else if code == 0 {
                Outcome::Installed
            } else {
                Outcome::Failed(format!("the setup exited with code {code}"))
            }
        }
    };
    unsafe { CloseHandle(info.hProcess) };
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("canape-eac-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn settings_give_a_safe_product_id() {
        let text = "\u{feff}{\n  \"productid\": \"0a1b2c3d4e5f60718293a4b5c6d7e8f9\",\n  \"sandboxid\": \"s\",\n  \"deploymentid\": \"d\",\n  \"title\": \"Fall Guys\"\n}";
        assert_eq!(
            parse_settings(text).as_deref(),
            Some("0a1b2c3d4e5f60718293a4b5c6d7e8f9")
        );
        assert_eq!(
            parse_settings(r#"{"ProductId": "abc_12-3"}"#).as_deref(),
            Some("abc_12-3")
        );
        // Anything that could add arguments is refused.
        assert_eq!(parse_settings(r#"{"productid": "abc --x"}"#), None);
        assert_eq!(parse_settings(r#"{"productid": "a\"b"}"#), None);
        assert_eq!(parse_settings(r#"{"productid": ""}"#), None);
        assert_eq!(parse_settings(r#"{"productid": 12}"#), None);
        assert_eq!(parse_settings(r#"{"title": "x"}"#), None);
        assert_eq!(parse_settings("not json"), None);
        let setup = Setup {
            exe: PathBuf::from("x.exe"),
            product_id: "abc".into(),
        };
        assert_eq!(setup.args(), "install abc");
    }

    #[test]
    fn the_setup_is_found_in_the_game_folder() {
        let dir = temp("find");
        let eac = dir.join("easyanticheat");
        std::fs::create_dir_all(&eac).unwrap();
        assert_eq!(find(&dir), None);
        std::fs::write(eac.join("EasyAntiCheat_EOS_Setup.exe"), b"MZ").unwrap();
        assert_eq!(find(&dir), None, "no Settings.json yet");
        std::fs::write(eac.join("settings.json"), br#"{"productid":"p1"}"#).unwrap();
        let setup = find(&dir).unwrap();
        assert_eq!(
            setup.exe.canonicalize().unwrap(),
            eac.join("EasyAntiCheat_EOS_Setup.exe")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(setup.product_id, "p1");

        // A Legendary install is found through its installed.json.
        let config = dir.join("legendary");
        std::fs::create_dir_all(&config).unwrap();
        let installed = serde_json::json!({
            "FG": {"app_name": "FG", "title": "Fall Guys", "install_path": dir, "is_dlc": false}
        });
        std::fs::write(config.join("installed.json"), installed.to_string()).unwrap();
        let roots = Roots {
            legendary: Some((config.clone(), PathBuf::from("legendary.exe"))),
            ..Roots::default()
        };
        let game = crate::legendary::scan(&config, Path::new("legendary.exe"))
            .pop()
            .unwrap();
        assert_eq!(setup_for(&game, &roots).unwrap().product_id, "p1");
        // Steam installs the anti-cheat itself.
        let steam = Game {
            source: Source::Steam,
            ..game
        };
        assert_eq!(setup_for(&steam, &roots), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
