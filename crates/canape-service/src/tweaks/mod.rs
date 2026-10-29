//! Windows tweaks: the fixed catalogue of what each one writes, and the
//! engine that applies, reverts and checks them through a [`System`].
//!
//! Clients only name a [`TweakId`]; every key, value, folder and command
//! comes from here. Applying a tweak records what each value held before,
//! so that reverting puts it back (`state.json` keeps the record).

mod power;
mod services;
mod sim;
#[cfg(windows)]
mod windows;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use canape_core::{TweakId, TweakState, Unavailable};
use serde::{Deserialize, Serialize};

pub use power::{PowerRecord, PowerSetting, PowerSource};
pub use services::{ASUS_SERVICES, START_DISABLED, SavedService, ServiceState};
pub use sim::SimSystem;

/// Which registry hive a value lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// `HKEY_LOCAL_MACHINE`.
    Machine,
    /// The hive of the user signed in at the console.
    User,
}

/// Registry data, as read or written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegData {
    Dword(u32),
    Sz(String),
    MultiSz(Vec<String>),
    /// Any other type, kept as is to be written back: the `REG_*` type and
    /// the bytes.
    Raw {
        kind: u32,
        bytes: Vec<u8>,
    },
}

/// What the tweaks need from Windows. The service holds the real one; tests
/// and other systems use [`SimSystem`].
pub trait System: Send {
    fn read(&self, scope: Scope, key: &str, name: &str) -> Result<Option<RegData>, String>;
    /// Writes `data`, or deletes the value for `None`.
    fn write(
        &mut self,
        scope: Scope,
        key: &str,
        name: &str,
        data: Option<&RegData>,
    ) -> Result<(), String>;
    /// Tamper Protection in Windows Security; `None` when unknown.
    fn tamper_protected(&self) -> Option<bool>;
    /// Folders games are installed in, before [`exclusion_folders`] sorts
    /// out the ones that may be excluded.
    fn game_folders(&self) -> Vec<PathBuf>;
    /// Folders Microsoft Defender excludes from its scans.
    fn defender_exclusions(&self) -> Vec<String>;
    fn add_defender_exclusions(&mut self, paths: &[String]) -> Result<(), String>;
    fn remove_defender_exclusions(&mut self, paths: &[String]) -> Result<(), String>;
    /// Real-time protection is off.
    fn realtime_disabled(&self) -> bool;
    fn set_realtime_disabled(&mut self, disabled: bool) -> Result<(), String>;
    /// GUID of the active power plan.
    fn power_scheme(&self) -> Result<String, String>;
    /// A processor setting of a plan, for AC or DC.
    fn power_get(
        &self,
        scheme: &str,
        setting: PowerSetting,
        source: PowerSource,
    ) -> Result<u32, String>;
    fn power_set(
        &mut self,
        scheme: &str,
        setting: PowerSetting,
        source: PowerSource,
        value: u32,
    ) -> Result<(), String>;
    /// Makes Windows apply the plan's current values.
    fn power_commit(&mut self, scheme: &str) -> Result<(), String>;
    /// A service by name; `None` when it is not installed.
    fn service(&self, name: &str) -> Option<ServiceState>;
    /// Sets how the service starts (`start` is a `SERVICE_*_START` value).
    fn set_service_start(&mut self, name: &str, start: u32, delayed: bool) -> Result<(), String>;
    fn stop_service(&mut self, name: &str) -> Result<(), String>;
    fn start_service(&mut self, name: &str) -> Result<(), String>;
}

/// The real system on Windows, unless `CANAPE_MOCK_TWEAKS` is set; the
/// simulator elsewhere.
pub fn system() -> Box<dyn System> {
    #[cfg(windows)]
    if std::env::var_os("CANAPE_MOCK_TWEAKS").is_none() {
        return Box::new(windows::WinSystem);
    }
    Box::new(SimSystem::from_env())
}

/// One registry value of a tweak.
#[derive(Debug, Clone, Copy)]
struct Value {
    scope: Scope,
    key: &'static str,
    name: &'static str,
    on: Data,
    /// Part of the "is applied" check. Values Windows may refuse to change
    /// on some builds are written when possible and not checked.
    checked: bool,
}

#[derive(Debug, Clone, Copy)]
enum Data {
    Dword(u32),
    /// The packaged apps still allowed to run in the background.
    BackgroundAllowList,
    /// One `name=value;` entry inside a string value.
    Entry(&'static str, &'static str),
}

/// What a tweak does besides registry values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    DefenderExclusions,
    DefenderRealtime,
    AsusServices,
}

/// What a refused registry write says (`ERROR_ACCESS_DENIED`).
const ACCESS_DENIED: &str = "error 5";

struct Def {
    id: TweakId,
    values: &'static [Value],
    action: Action,
}

const fn machine(key: &'static str, name: &'static str, on: u32) -> Value {
    Value {
        scope: Scope::Machine,
        key,
        name,
        on: Data::Dword(on),
        checked: true,
    }
}

const fn user(key: &'static str, name: &'static str, on: u32) -> Value {
    Value {
        scope: Scope::User,
        key,
        name,
        on: Data::Dword(on),
        checked: true,
    }
}

const fn unchecked(value: Value) -> Value {
    Value {
        checked: false,
        ..value
    }
}

const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const CONTENT: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const DATA_COLLECTION: &str = r"SOFTWARE\Policies\Microsoft\Windows\DataCollection";
const APP_PRIVACY: &str = r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";
const DEVICE_GUARD: &str = r"SYSTEM\CurrentControlSet\Control\DeviceGuard";
const HVCI: &str =
    r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity";
const DEVICE_GUARD_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DeviceGuard";
const DEFENDER_REALTIME_POLICY: &str =
    r"SOFTWARE\Policies\Microsoft\Windows Defender\Real-Time Protection";
const GPU_PREFERENCES: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";

/// Packaged apps that keep running in the background with
/// [`TweakId::BackgroundAppsOff`]: the Xbox apps and the Game Bar, the
/// Store, Windows Security; the widget's family is added at run time.
const BACKGROUND_ALLOWED: &[&str] = &[
    "Microsoft.XboxGamingOverlay_8wekyb3d8bbwe",
    "Microsoft.GamingApp_8wekyb3d8bbwe",
    "Microsoft.GamingServices_8wekyb3d8bbwe",
    "Microsoft.XboxIdentityProvider_8wekyb3d8bbwe",
    "Microsoft.Xbox.TCUI_8wekyb3d8bbwe",
    "Microsoft.WindowsStore_8wekyb3d8bbwe",
    "Microsoft.SecHealthUI_8wekyb3d8bbwe",
];

/// What each tweak writes. Dword values unless noted; reverting restores
/// the value recorded at apply time, or deletes it (the Windows default).
static CATALOGUE: &[Def] = &[
    Def {
        id: TweakId::CopilotOff,
        values: &[
            // Group policy "Turn off Windows Copilot", machine and user.
            machine(
                r"SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot",
                "TurnOffWindowsCopilot",
                1,
            ),
            user(
                r"Software\Policies\Microsoft\Windows\WindowsCopilot",
                "TurnOffWindowsCopilot",
                1,
            ),
            // Taskbar button.
            unchecked(user(ADVANCED, "ShowCopilotButton", 0)),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::WidgetsOff,
        values: &[
            // Group policy "Allow widgets": disabled.
            machine(
                r"SOFTWARE\Policies\Microsoft\Dsh",
                "AllowNewsAndInterests",
                0,
            ),
            // Taskbar button; recent builds may refuse this one.
            unchecked(user(ADVANCED, "TaskbarDa", 0)),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::WebSearchOff,
        values: &[
            // No Bing results or suggestions in Start search.
            user(
                r"Software\Policies\Microsoft\Windows\Explorer",
                "DisableSearchBoxSuggestions",
                1,
            ),
            user(
                r"Software\Microsoft\Windows\CurrentVersion\Search",
                "BingSearchEnabled",
                0,
            ),
            // Group policy "Allow search highlights": disabled.
            machine(
                r"SOFTWARE\Policies\Microsoft\Windows\Windows Search",
                "EnableDynamicContentInWSB",
                0,
            ),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::SuggestionsOff,
        values: &[
            // Suggested apps in Start and Settings, promoted apps installed
            // silently.
            user(CONTENT, "SystemPaneSuggestionsEnabled", 0),
            user(CONTENT, "SubscribedContent-338388Enabled", 0),
            user(CONTENT, "SubscribedContent-338393Enabled", 0),
            user(CONTENT, "SubscribedContent-353694Enabled", 0),
            user(CONTENT, "SubscribedContent-353696Enabled", 0),
            user(CONTENT, "SilentInstalledAppsEnabled", 0),
            // Start: recommendations for tips, shortcuts and new apps, and
            // account notifications.
            user(ADVANCED, "Start_IrisRecommendations", 0),
            user(ADVANCED, "Start_AccountNotifications", 0),
            // File Explorer: sync provider notifications.
            user(ADVANCED, "ShowSyncProviderNotifications", 0),
            // Advertising ID, per user and by policy.
            user(
                r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
                "Enabled",
                0,
            ),
            machine(
                r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo",
                "DisabledByGroupPolicy",
                1,
            ),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::LockScreenTipsOff,
        values: &[
            user(CONTENT, "RotatingLockScreenOverlayEnabled", 0),
            user(CONTENT, "SubscribedContent-338387Enabled", 0),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::TipsOff,
        values: &[
            // Tips and suggestions notifications.
            user(CONTENT, "SubscribedContent-338389Enabled", 0),
            user(CONTENT, "SoftLandingEnabled", 0),
            // Welcome experience after updates.
            user(CONTENT, "SubscribedContent-310093Enabled", 0),
            // "Finish setting up your device".
            user(
                r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement",
                "ScoobeSystemSettingEnabled",
                0,
            ),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::TelemetryMin,
        values: &[
            // Group policy "Allow diagnostic data": 0 is the lowest level;
            // Home and Pro treat it as "required" data.
            machine(DATA_COLLECTION, "AllowTelemetry", 0),
            machine(DATA_COLLECTION, "DoNotShowFeedbackNotifications", 1),
            user(
                r"Software\Microsoft\Windows\CurrentVersion\Privacy",
                "TailoredExperiencesWithDiagnosticDataEnabled",
                0,
            ),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::BackgroundAppsOff,
        values: &[
            // Group policy "Let Windows apps run in the background": force
            // deny, except the apps listed (REG_MULTI_SZ).
            machine(APP_PRIVACY, "LetAppsRunInBackground", 2),
            Value {
                scope: Scope::Machine,
                key: APP_PRIVACY,
                name: "LetAppsRunInBackground_ForceAllowTheseApps",
                on: Data::BackgroundAllowList,
                checked: true,
            },
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::GameModeOn,
        values: &[
            user(r"Software\Microsoft\GameBar", "AutoGameModeEnabled", 1),
            user(r"Software\Microsoft\GameBar", "AllowAutoGameMode", 1),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::GpuSchedulingOn,
        values: &[
            // 2 = on, 1 = off.
            machine(
                r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
                "HwSchMode",
                2,
            ),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::WindowedOptimizationsOn,
        values: &[Value {
            scope: Scope::User,
            key: GPU_PREFERENCES,
            name: "DirectXUserGlobalSettings",
            on: Data::Entry("SwapEffectUpgradeEnable", "1"),
            checked: true,
        }],
        action: Action::None,
    },
    Def {
        id: TweakId::GameDvrOff,
        values: &[
            // Background recording and its capture settings, per user. The
            // `AllowGameDVR` policy is left alone: it also takes the Game
            // Bar away.
            user(r"System\GameConfigStore", "GameDVR_Enabled", 0),
            user(
                r"Software\Microsoft\Windows\CurrentVersion\GameDVR",
                "AppCaptureEnabled",
                0,
            ),
        ],
        action: Action::None,
    },
    Def {
        id: TweakId::AsusServicesOff,
        values: &[],
        action: Action::AsusServices,
    },
    Def {
        id: TweakId::DefenderExclusions,
        values: &[],
        action: Action::DefenderExclusions,
    },
    Def {
        id: TweakId::DefenderRealtimeOff,
        values: &[
            // Group policy "Turn off real-time protection": keeps it off
            // after a restart. Ignored while Tamper Protection is on, so
            // the check reads the protection itself.
            unchecked(machine(
                DEFENDER_REALTIME_POLICY,
                "DisableRealtimeMonitoring",
                1,
            )),
        ],
        action: Action::DefenderRealtime,
    },
    Def {
        id: TweakId::MemoryIntegrityOff,
        values: &[
            // The Windows Security "Memory integrity" switch.
            machine(HVCI, "Enabled", 0),
            // Virtualization-based security as a whole.
            machine(DEVICE_GUARD, "EnableVirtualizationBasedSecurity", 0),
        ],
        action: Action::None,
    },
];

fn def(id: TweakId) -> &'static Def {
    CATALOGUE
        .iter()
        .find(|d| d.id == id)
        .expect("every tweak is in the catalogue")
}

/// What Canapé replaced when it applied a tweak.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Applied {
    pub prior: Vec<Prior>,
    /// Defender exclusions Canapé added (and only those are removed).
    pub paths: Vec<String>,
    /// ASUS services Canapé disabled, as they were.
    pub services: Vec<SavedService>,
}

/// A registry value before Canapé changed it; `data: None` when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prior {
    pub scope: Scope,
    pub key: String,
    pub name: String,
    pub data: Option<RegData>,
}

/// The engine: the system, what Canapé applied, and what held at startup
/// (to tell a restart is pending).
pub struct Tweaks {
    system: Box<dyn System>,
    applied: BTreeMap<TweakId, Applied>,
    at_start: HashMap<TweakId, bool>,
    widget_family: String,
    /// Tweaks whose values Windows refused to write (access denied: a
    /// protected policy); they are reported as locked from then on.
    blocked: HashSet<TweakId>,
    /// What Canapé replaced in the power plan (CPU settings).
    power: PowerRecord,
}

impl Tweaks {
    pub fn new(system: Box<dyn System>) -> Self {
        Self {
            system,
            applied: BTreeMap::new(),
            at_start: HashMap::new(),
            widget_family: canape_ipc::widget_family_name(),
            blocked: HashSet::new(),
            power: PowerRecord::default(),
        }
    }

    /// What to persist.
    pub fn saved(&self) -> BTreeMap<TweakId, Applied> {
        self.applied.clone()
    }

    /// Takes back the record of a previous run. Nothing is written:
    /// Windows keeps the settings.
    pub fn restore(&mut self, applied: BTreeMap<TweakId, Applied>) {
        self.applied = applied;
    }

    pub fn list(&mut self) -> Vec<TweakState> {
        let folders = self.folders();
        TweakId::ALL
            .iter()
            .map(|&id| self.state(id, &folders))
            .collect()
    }

    pub fn set(&mut self, id: TweakId, on: bool) -> Result<(), String> {
        self.remember_start(id);
        if on { self.apply(id) } else { self.revert(id) }
    }

    /// Reverts every tweak Canapé applied, and the CPU settings of the
    /// power plan; the errors, joined, if any.
    pub fn revert_all(&mut self) -> Result<(), String> {
        let ids: Vec<TweakId> = self.applied.keys().copied().collect();
        let mut errors: Vec<String> = ids
            .into_iter()
            .filter_map(|id| self.revert(id).err().map(|e| format!("{}: {e}", id.name())))
            .collect();
        if let Err(e) = self.set_cpu_power(&canape_core::CpuPower::default()) {
            errors.push(format!("power plan: {e}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn remember_start(&mut self, id: TweakId) {
        if !self.at_start.contains_key(&id) {
            let applied = self.is_applied(id, &self.folders());
            self.at_start.insert(id, applied);
        }
    }

    fn folders(&self) -> Vec<String> {
        exclusion_folders(&self.system.game_folders())
    }

    fn state(&mut self, id: TweakId, folders: &[String]) -> TweakState {
        let applied = self.is_applied(id, folders);
        let at_start = *self.at_start.entry(id).or_insert(applied);
        let by_canape = self.applied.contains_key(&id);
        let paths = match id {
            TweakId::DefenderExclusions => folders.to_vec(),
            TweakId::AsusServicesOff => self.asus_services().into_iter().map(|(n, _)| n).collect(),
            _ => Vec::new(),
        };
        TweakState {
            id,
            applied,
            by_canape,
            unavailable: if applied {
                None
            } else {
                self.unavailable(id, folders)
            },
            reboot_pending: id.info().reboot && applied != at_start,
            paths,
        }
    }

    fn unavailable(&self, id: TweakId, folders: &[String]) -> Option<Unavailable> {
        if self.blocked.contains(&id) {
            return Some(Unavailable::Locked);
        }
        match id {
            TweakId::DefenderRealtimeOff if self.system.tamper_protected() == Some(true) => {
                Some(Unavailable::TamperProtection)
            }
            TweakId::DefenderExclusions if folders.is_empty() => Some(Unavailable::NoGameFolders),
            TweakId::AsusServicesOff if self.asus_services().is_empty() => {
                Some(Unavailable::NotSupported)
            }
            TweakId::MemoryIntegrityOff => {
                let locked = self
                    .system
                    .read(Scope::Machine, HVCI, "Locked")
                    .ok()
                    .flatten()
                    == Some(RegData::Dword(1));
                let by_policy = self
                    .system
                    .read(
                        Scope::Machine,
                        DEVICE_GUARD_POLICY,
                        "EnableVirtualizationBasedSecurity",
                    )
                    .ok()
                    .flatten()
                    .is_some();
                (locked || by_policy).then_some(Unavailable::Locked)
            }
            _ => None,
        }
    }

    fn on_data(&self, data: Data, current: Option<&RegData>) -> RegData {
        match data {
            Data::Dword(v) => RegData::Dword(v),
            Data::BackgroundAllowList => RegData::MultiSz(
                BACKGROUND_ALLOWED
                    .iter()
                    .map(|s| s.to_string())
                    .chain(Some(self.widget_family.clone()))
                    .collect(),
            ),
            Data::Entry(name, value) => {
                let text = match current {
                    Some(RegData::Sz(text)) => text.as_str(),
                    _ => "",
                };
                RegData::Sz(set_entry(text, name, Some(value)))
            }
        }
    }

    fn value_applied(&self, value: &Value) -> bool {
        let current = self
            .system
            .read(value.scope, value.key, value.name)
            .ok()
            .flatten();
        match value.on {
            Data::Entry(name, wanted) => match &current {
                Some(RegData::Sz(text)) => entry(text, name) == Some(wanted),
                _ => false,
            },
            data => current.as_ref() == Some(&self.on_data(data, None)),
        }
    }

    fn is_applied(&self, id: TweakId, folders: &[String]) -> bool {
        let def = def(id);
        let values = def
            .values
            .iter()
            .filter(|v| v.checked)
            .all(|v| self.value_applied(v));
        values
            && match def.action {
                Action::None => true,
                Action::DefenderExclusions => {
                    let excluded = self.system.defender_exclusions();
                    !folders.is_empty()
                        && folders
                            .iter()
                            .all(|f| excluded.iter().any(|e| same_folder(e, f)))
                }
                Action::DefenderRealtime => self.system.realtime_disabled(),
                Action::AsusServices => {
                    let present = self.asus_services();
                    !present.is_empty() && present.iter().all(|(_, s)| s.start == START_DISABLED)
                }
            }
    }

    /// The ASUS services installed, with their state.
    fn asus_services(&self) -> Vec<(String, ServiceState)> {
        ASUS_SERVICES
            .iter()
            .filter(|name| !services::is_kept(name))
            .filter_map(|name| Some((name.to_string(), self.system.service(name)?)))
            .collect()
    }

    fn apply(&mut self, id: TweakId) -> Result<(), String> {
        let folders = self.folders();
        if let Some(reason) = self.unavailable(id, &folders) {
            return Err(unavailable_message(reason).into());
        }
        let def = def(id);
        let mut record = self.applied.get(&id).cloned().unwrap_or_default();
        let mut error = None;
        for value in def.values {
            let current = self
                .system
                .read(value.scope, value.key, value.name)
                .map_err(|e| format!("reading {}\\{}: {e}", value.key, value.name));
            let current = match current {
                Ok(current) => current,
                Err(e) if value.checked => {
                    error.get_or_insert(e);
                    continue;
                }
                Err(_) => continue,
            };
            let known = record
                .prior
                .iter()
                .any(|p| p.scope == value.scope && p.key == value.key && p.name == value.name);
            if !known {
                record.prior.push(Prior {
                    scope: value.scope,
                    key: value.key.into(),
                    name: value.name.into(),
                    data: current.clone(),
                });
            }
            let data = self.on_data(value.on, current.as_ref());
            if let Err(e) = self
                .system
                .write(value.scope, value.key, value.name, Some(&data))
                && value.checked
            {
                if e == ACCESS_DENIED {
                    self.blocked.insert(id);
                }
                error.get_or_insert(format!("writing {}\\{}: {e}", value.key, value.name));
            }
        }
        match def.action {
            Action::None => {}
            Action::DefenderExclusions => {
                let excluded = self.system.defender_exclusions();
                let missing: Vec<String> = folders
                    .iter()
                    .filter(|f| !excluded.iter().any(|e| same_folder(e, f)))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    match self.system.add_defender_exclusions(&missing) {
                        Ok(()) => record.paths.extend(missing),
                        Err(e) => {
                            error.get_or_insert(e);
                        }
                    }
                }
            }
            Action::DefenderRealtime => {
                let result = self.system.set_realtime_disabled(true);
                if !self.system.realtime_disabled() {
                    error.get_or_insert(match result {
                        Err(e) => e,
                        Ok(()) => "Windows kept real-time protection on: turn Tamper \
                                   Protection off in Windows Security first"
                            .into(),
                    });
                }
            }
            Action::AsusServices => {
                for (name, state) in self.asus_services() {
                    if !record.services.iter().any(|s| s.name == name) {
                        record.services.push(SavedService {
                            name: name.clone(),
                            start: state.start,
                            delayed: state.delayed,
                            running: state.running,
                        });
                    }
                    if let Err(e) = self.system.set_service_start(&name, START_DISABLED, false) {
                        error.get_or_insert(format!("service {name}: {e}"));
                        continue;
                    }
                    // A service that will not stop is off after a restart.
                    if state.running {
                        let _ = self.system.stop_service(&name);
                    }
                }
            }
        }
        self.applied.insert(id, record);
        error.map_or(Ok(()), Err)
    }

    fn revert(&mut self, id: TweakId) -> Result<(), String> {
        let def = def(id);
        let record = self.applied.get(&id).cloned();
        let mut error = None;
        for value in def.values {
            let prior = record.as_ref().and_then(|r| {
                r.prior
                    .iter()
                    .find(|p| p.scope == value.scope && p.key == value.key && p.name == value.name)
            });
            let data = match (prior, value.on) {
                (Some(prior), _) => prior.data.clone(),
                // Not applied by Canapé: drop only its own entry.
                (None, Data::Entry(name, _)) => {
                    match self.system.read(value.scope, value.key, value.name) {
                        Ok(Some(RegData::Sz(text))) => {
                            let text = set_entry(&text, name, None);
                            (!text.is_empty()).then_some(RegData::Sz(text))
                        }
                        _ => None,
                    }
                }
                (None, _) => None,
            };
            if let Err(e) = self
                .system
                .write(value.scope, value.key, value.name, data.as_ref())
                && value.checked
            {
                error.get_or_insert(format!("writing {}\\{}: {e}", value.key, value.name));
            }
        }
        match def.action {
            Action::None => {}
            Action::DefenderExclusions => {
                let paths = record.as_ref().map(|r| r.paths.clone()).unwrap_or_default();
                if !paths.is_empty()
                    && let Err(e) = self.system.remove_defender_exclusions(&paths)
                {
                    error.get_or_insert(e);
                }
            }
            Action::DefenderRealtime => {
                if self.system.realtime_disabled()
                    && let Err(e) = self.system.set_realtime_disabled(false)
                {
                    error.get_or_insert(e);
                }
            }
            Action::AsusServices => {
                let mut left = Vec::new();
                for saved in record.iter().flat_map(|r| r.services.iter()) {
                    // Gone since (uninstalled): nothing to put back.
                    if self.system.service(&saved.name).is_none() {
                        continue;
                    }
                    if let Err(e) =
                        self.system
                            .set_service_start(&saved.name, saved.start, saved.delayed)
                    {
                        error.get_or_insert(format!("service {}: {e}", saved.name));
                        left.push(saved.clone());
                        continue;
                    }
                    if saved.running {
                        let _ = self.system.start_service(&saved.name);
                    }
                }
                if !left.is_empty() {
                    let mut kept = record.clone().unwrap_or_default();
                    kept.services = left;
                    self.applied.insert(id, kept);
                }
            }
        }
        match error {
            Some(e) => Err(e),
            None => {
                self.applied.remove(&id);
                Ok(())
            }
        }
    }
}

pub fn unavailable_message(reason: Unavailable) -> &'static str {
    match reason {
        Unavailable::TamperProtection => {
            "Tamper Protection is on: turn it off in Windows Security first"
        }
        Unavailable::Locked => "locked by the firmware or by a policy",
        Unavailable::NoGameFolders => "no game folder found",
        Unavailable::NotSupported => "not supported on this system",
    }
}

/// The value of `name` in a `name=value;name=value;` string.
pub fn entry<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.split(';')
        .filter_map(|part| part.split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

/// `text` with `name` set to `value` (added last when missing), or
/// removed for `None`; the other entries kept in order.
pub fn set_entry(text: &str, name: &str, value: Option<&str>) -> String {
    let mut out = String::new();
    let mut done = false;
    for part in text.split(';').filter(|p| !p.trim().is_empty()) {
        let key = part.split_once('=').map_or(part, |(k, _)| k).trim();
        if key.eq_ignore_ascii_case(name) {
            if let (Some(v), false) = (value, done) {
                out.push_str(&format!("{name}={v};"));
                done = true;
            }
        } else {
            out.push_str(part.trim());
            out.push(';');
        }
    }
    if let (Some(v), false) = (value, done) {
        out.push_str(&format!("{name}={v};"));
    }
    out
}

/// Windows compares folders ignoring case, either slash and a trailing
/// separator.
pub fn same_folder(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// Lower-case components of a Windows path, the drive first (`c:`).
fn parts(path: &str) -> Vec<String> {
    path.replace('/', "\\")
        .split('\\')
        .filter(|p| !p.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Folders that must never be excluded, whatever a store says: a drive
/// root, Windows, the program and data roots, the users folder, a user's
/// profile and its usual places for downloads and documents.
pub fn safe_to_exclude(path: &str) -> bool {
    let p = parts(path);
    let absolute = p.first().is_some_and(|d| d.len() == 2 && d.ends_with(':'));
    if !absolute || p.len() < 2 || p.iter().any(|c| c == "..") {
        return false;
    }
    match p[1].as_str() {
        "windows" | "programdata" | "$recycle.bin" | "system volume information" => false,
        "program files" | "program files (x86)" => p.len() >= 3,
        "users" => {
            p.len() >= 4
                && !matches!(
                    p[3].as_str(),
                    "appdata" | "downloads" | "desktop" | "documents" | "onedrive"
                )
        }
        _ => true,
    }
}

/// The folders to exclude from `found`: the safe ones, each once, without
/// those inside another, sorted.
pub fn exclusion_folders(found: &[PathBuf]) -> Vec<String> {
    let mut folders: Vec<String> = Vec::new();
    for path in found {
        let text = path.to_string_lossy().replace('/', "\\");
        let text = text.trim_end_matches('\\').to_owned();
        if safe_to_exclude(&text) && !folders.iter().any(|f| same_folder(f, &text)) {
            folders.push(text);
        }
    }
    let inside = |child: &str, parent: &str| {
        let (c, p) = (parts(child), parts(parent));
        c.len() > p.len() && c[..p.len()] == p[..]
    };
    let kept: Vec<String> = folders
        .iter()
        .filter(|f| !folders.iter().any(|other| inside(f, other)))
        .cloned()
        .collect();
    let mut kept = kept;
    kept.sort_by_key(|f| f.to_lowercase());
    kept
}

/// Where games are installed: each Steam library's `steamapps\common`, the
/// folders holding Epic and GOG installs, and `Games` at a drive's root
/// (where Canapé installs by default). Only existing folders.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn find_game_folders(
    steam_libraries: &[PathBuf],
    installs: &[PathBuf],
    drives: &[PathBuf],
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = steam_libraries
        .iter()
        .map(|lib| lib.join("steamapps").join("common"))
        .collect();
    found.extend(canape_library::disk::parents(installs));
    found.extend(drives.iter().map(|d| d.join(canape_library::disk::GAMES)));
    found.retain(|p| p.is_dir());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> (Tweaks, SimSystem) {
        let sim = SimSystem::new(true);
        (Tweaks::new(Box::new(sim.clone())), sim)
    }

    #[test]
    fn catalogue_covers_every_tweak_once() {
        for id in TweakId::ALL {
            assert_eq!(CATALOGUE.iter().filter(|d| d.id == id).count(), 1, "{id:?}");
            let def = def(id);
            assert!(
                !def.values.is_empty() || def.action != Action::None,
                "{id:?} does nothing"
            );
            assert!(
                def.values.iter().any(|v| v.checked) || def.action != Action::None,
                "{id:?} cannot be checked"
            );
        }
        assert_eq!(CATALOGUE.len(), TweakId::ALL.len());
    }

    #[test]
    fn every_tweak_applies_and_reverts_to_what_was_there() {
        let (mut tweaks, sim) = engine();
        sim.set_tamper(false);
        // Something set before Canapé: put back, not deleted.
        sim.put(
            Scope::Machine,
            r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
            "HwSchMode",
            RegData::Dword(1),
        );
        sim.put(
            Scope::User,
            GPU_PREFERENCES,
            "DirectXUserGlobalSettings",
            RegData::Sz("VRROptimizeEnable=0;".into()),
        );
        let before = sim.registry();
        let services_before = sim.services();
        for id in TweakId::ALL {
            assert_eq!(tweaks.set(id, true), Ok(()), "{id:?}");
        }
        let states = tweaks.list();
        for state in &states {
            assert!(state.applied && state.by_canape, "{state:?}");
        }
        assert_eq!(
            sim.read(Scope::User, GPU_PREFERENCES, "DirectXUserGlobalSettings")
                .unwrap(),
            Some(RegData::Sz(
                "VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;".into()
            ))
        );
        assert!(
            states
                .iter()
                .find(|s| s.id == TweakId::GpuSchedulingOn)
                .unwrap()
                .reboot_pending
        );
        let excluded_before = sim.defender_exclusions();
        assert_eq!(excluded_before.len(), 3);
        assert_eq!(tweaks.revert_all(), Ok(()));
        assert_eq!(sim.registry(), before);
        assert_eq!(sim.services(), services_before);
        assert!(sim.defender_exclusions().is_empty());
        assert!(!sim.realtime_disabled());
        for state in tweaks.list() {
            assert!(!state.by_canape && !state.reboot_pending, "{state:?}");
        }
    }

    #[test]
    fn game_recording_off_keeps_the_game_bar_policy_alone() {
        let (mut tweaks, sim) = engine();
        assert_eq!(tweaks.set(TweakId::GameDvrOff, true), Ok(()));
        assert_eq!(
            sim.read(Scope::User, r"System\GameConfigStore", "GameDVR_Enabled")
                .unwrap(),
            Some(RegData::Dword(0))
        );
        assert_eq!(
            sim.read(
                Scope::User,
                r"Software\Microsoft\Windows\CurrentVersion\GameDVR",
                "AppCaptureEnabled"
            )
            .unwrap(),
            Some(RegData::Dword(0))
        );
        let written = sim.registry();
        assert!(
            written
                .keys()
                .all(|(_, key, _)| !key.contains("windows\\gamedvr")),
            "the AllowGameDVR policy must stay untouched: {written:?}"
        );
    }

    #[test]
    fn asus_services_are_disabled_and_restored_as_they_were() {
        let (mut tweaks, sim) = engine();
        let before = sim.services();
        let state = |t: &mut Tweaks| {
            t.list()
                .into_iter()
                .find(|s| s.id == TweakId::AsusServicesOff)
                .unwrap()
        };
        let listed = state(&mut tweaks);
        assert!(!listed.applied);
        assert_eq!(listed.paths.len(), before.len());
        assert_eq!(tweaks.set(TweakId::AsusServicesOff, true), Ok(()));
        assert!(state(&mut tweaks).applied);
        for (name, service) in sim.services() {
            assert_eq!(service.start, START_DISABLED, "{name}");
            assert!(!service.running, "{name}");
        }
        // Nothing else than the listed names is ever touched.
        assert!(
            sim.services()
                .keys()
                .all(|n| ASUS_SERVICES.contains(&n.as_str()))
        );
        assert_eq!(tweaks.set(TweakId::AsusServicesOff, false), Ok(()));
        assert_eq!(sim.services(), before);
        assert!(!state(&mut tweaks).by_canape);
    }

    #[test]
    fn asus_services_are_unavailable_without_them() {
        let sim = SimSystem::new(false);
        let mut tweaks = Tweaks::new(Box::new(sim.clone()));
        for name in sim.services().keys() {
            sim.remove_service(name);
        }
        let state = tweaks
            .list()
            .into_iter()
            .find(|s| s.id == TweakId::AsusServicesOff)
            .unwrap();
        assert_eq!(state.unavailable, Some(Unavailable::NotSupported));
        assert!(tweaks.set(TweakId::AsusServicesOff, true).is_err());
    }

    #[test]
    fn a_protected_policy_makes_the_tweak_locked_not_stuck() {
        let (mut tweaks, sim) = engine();
        sim.deny(r"SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests");
        let error = tweaks.set(TweakId::WidgetsOff, true).unwrap_err();
        assert!(error.contains("error 5"), "{error}");
        let state = |tweaks: &mut Tweaks| {
            tweaks
                .list()
                .into_iter()
                .find(|s| s.id == TweakId::WidgetsOff)
                .unwrap()
        };
        assert_eq!(state(&mut tweaks).unavailable, Some(Unavailable::Locked));
        // Asked again, it says why at once instead of trying.
        let again = tweaks.set(TweakId::WidgetsOff, true).unwrap_err();
        assert!(again.contains("locked"), "{again}");
        // The other tweaks are not affected.
        assert!(tweaks.set(TweakId::GameDvrOff, true).is_ok());
    }

    #[test]
    fn real_time_protection_needs_tamper_protection_off() {
        let (mut tweaks, sim) = engine();
        let state = |t: &mut Tweaks| {
            t.list()
                .into_iter()
                .find(|s| s.id == TweakId::DefenderRealtimeOff)
                .unwrap()
        };
        assert_eq!(
            state(&mut tweaks).unavailable,
            Some(Unavailable::TamperProtection)
        );
        let err = tweaks.set(TweakId::DefenderRealtimeOff, true).unwrap_err();
        assert!(err.contains("Tamper Protection"), "{err}");
        assert!(!sim.realtime_disabled());
        sim.set_tamper(false);
        assert_eq!(state(&mut tweaks).unavailable, None);
        assert_eq!(tweaks.set(TweakId::DefenderRealtimeOff, true), Ok(()));
        assert!(state(&mut tweaks).applied);
    }

    #[test]
    fn exclusions_only_remove_what_canape_added() {
        let (mut tweaks, mut sim) = engine();
        let folders = tweaks.folders();
        sim.add_defender_exclusions(&[folders[0].clone(), r"E:\Work".into()])
            .unwrap();
        tweaks.set(TweakId::DefenderExclusions, true).unwrap();
        assert_eq!(sim.defender_exclusions().len(), 4);
        tweaks.set(TweakId::DefenderExclusions, false).unwrap();
        let left = sim.defender_exclusions();
        assert_eq!(left, vec![folders[0].clone(), r"E:\Work".to_string()]);
    }

    #[test]
    fn reverting_what_canape_did_not_apply_restores_defaults() {
        let (mut tweaks, sim) = engine();
        sim.put(
            Scope::User,
            ADVANCED,
            "Start_IrisRecommendations",
            RegData::Dword(0),
        );
        assert_eq!(tweaks.set(TweakId::SuggestionsOff, false), Ok(()));
        assert_eq!(
            sim.read(Scope::User, ADVANCED, "Start_IrisRecommendations")
                .unwrap(),
            None
        );
    }

    #[test]
    fn a_locked_memory_integrity_is_unavailable() {
        let (mut tweaks, sim) = engine();
        sim.put(Scope::Machine, HVCI, "Locked", RegData::Dword(1));
        assert!(tweaks.set(TweakId::MemoryIntegrityOff, true).is_err());
        assert_eq!(sim.read(Scope::Machine, HVCI, "Enabled").unwrap(), None);
    }

    #[test]
    fn entries_of_a_settings_string() {
        assert_eq!(
            entry("A=1;SwapEffectUpgradeEnable=0;", "swapeffectupgradeenable"),
            Some("0")
        );
        assert_eq!(entry("", "A"), None);
        assert_eq!(set_entry("", "S", Some("1")), "S=1;");
        assert_eq!(set_entry("A=1;S=0;B=2", "S", Some("1")), "A=1;S=1;B=2;");
        assert_eq!(set_entry("A=1;S=0;", "S", None), "A=1;");
        assert_eq!(set_entry("S=1;", "S", None), "");
    }

    #[test]
    fn only_game_folders_may_be_excluded() {
        for bad in [
            r"C:\",
            r"C:\Windows\System32",
            r"C:\Program Files",
            r"C:\ProgramData\Epic",
            r"C:\Users",
            r"C:\Users\pierre",
            r"C:\Users\pierre\Downloads\x",
            r"C:\Users\pierre\AppData\Local\Games",
            r"relative\path",
            r"D:\Games\..\Windows",
        ] {
            assert!(!safe_to_exclude(bad), "{bad}");
        }
        for good in [
            r"D:\Games",
            r"C:\Program Files (x86)\Steam\steamapps\common",
            r"C:\Program Files\Epic Games",
            r"C:\Users\pierre\Games",
        ] {
            assert!(safe_to_exclude(good), "{good}");
        }
        let found = [
            PathBuf::from(r"D:\SteamLibrary\steamapps\common"),
            PathBuf::from(r"D:\Games"),
            PathBuf::from(r"D:\Games\Hades"),
            PathBuf::from(r"d:\games\"),
            PathBuf::from(r"C:\"),
            PathBuf::from(r"C:\Program Files\Epic Games"),
        ];
        assert_eq!(
            exclusion_folders(&found),
            vec![
                r"C:\Program Files\Epic Games".to_string(),
                r"D:\Games".to_string(),
                r"D:\SteamLibrary\steamapps\common".to_string(),
            ]
        );
    }
}
