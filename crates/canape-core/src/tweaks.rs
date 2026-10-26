//! Windows settings Canapé can change on request: a fixed catalogue the
//! service owns (it never runs anything a client names), and the list of
//! preinstalled apps the shell may remove for the signed-in user.
//!
//! Every tweak is reversible: the service records what it replaced and puts
//! it back on revert.

use serde::{Deserialize, Serialize};

/// One tweak of the catalogue. Unknown names fail to decode, so a client
/// can only ask for these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TweakId {
    /// Copilot policy, and its taskbar button hidden.
    CopilotOff,
    /// The widgets board and its taskbar button.
    WidgetsOff,
    /// Web results and search highlights in Start search.
    WebSearchOff,
    /// Suggested apps, promoted content and the advertising ID in Start,
    /// Settings and File Explorer; no silent install of promoted apps.
    SuggestionsOff,
    /// Facts and tips over the lock screen picture.
    LockScreenTipsOff,
    /// Tips notifications, the "welcome experience" after updates and the
    /// "finish setting up your device" screen.
    TipsOff,
    /// Diagnostic data at the lowest level the edition allows; no tailored
    /// experiences, no feedback prompts.
    TelemetryMin,
    /// Store apps no longer run in the background, except the Xbox apps,
    /// the Game Bar, the Store and Canapé's widget.
    BackgroundAppsOff,
    GameModeOn,
    /// Hardware-accelerated GPU scheduling (takes effect after a restart).
    GpuSchedulingOn,
    /// Flip presentation model for DirectX 10/11 games in a window.
    WindowedOptimizationsOn,
    /// Background recording (Game DVR) off; the Game Bar itself stays.
    GameDvrOff,
    /// The ASUS background services stopped and
    /// disabled, except the driver Canapé talks to.
    AsusServicesOff,
    /// Game folders excluded from Microsoft Defender scans.
    DefenderExclusions,
    /// Microsoft Defender real-time protection off. Windows ignores it
    /// while Tamper Protection is on.
    DefenderRealtimeOff,
    /// Memory integrity (HVCI) and virtualization-based security off
    /// (takes effect after a restart).
    MemoryIntegrityOff,
}

/// How the shell groups the tweaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TweakGroup {
    Clutter,
    Privacy,
    Games,
    Security,
}

impl TweakGroup {
    pub const ALL: [TweakGroup; 4] = [
        TweakGroup::Clutter,
        TweakGroup::Privacy,
        TweakGroup::Games,
        TweakGroup::Security,
    ];
}

/// What a client needs to show a tweak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TweakInfo {
    pub id: TweakId,
    pub group: TweakGroup,
    /// Part of the "recommended" set: harmless and useful on a handheld.
    pub recommended: bool,
    /// Takes effect after a restart.
    pub reboot: bool,
    /// Lowers the device's protection: asks for confirmation.
    pub security: bool,
}

impl TweakId {
    /// The catalogue, in display order.
    pub const ALL: [TweakId; 16] = [
        TweakId::CopilotOff,
        TweakId::WidgetsOff,
        TweakId::WebSearchOff,
        TweakId::SuggestionsOff,
        TweakId::LockScreenTipsOff,
        TweakId::TipsOff,
        TweakId::TelemetryMin,
        TweakId::BackgroundAppsOff,
        TweakId::GameModeOn,
        TweakId::GpuSchedulingOn,
        TweakId::WindowedOptimizationsOn,
        TweakId::GameDvrOff,
        TweakId::AsusServicesOff,
        TweakId::DefenderExclusions,
        TweakId::DefenderRealtimeOff,
        TweakId::MemoryIntegrityOff,
    ];

    pub fn info(self) -> TweakInfo {
        use TweakGroup::*;
        use TweakId::*;
        let (group, recommended, reboot, security) = match self {
            CopilotOff | WidgetsOff | WebSearchOff | SuggestionsOff | LockScreenTipsOff
            | TipsOff => (Clutter, true, false, false),
            TelemetryMin => (Privacy, true, false, false),
            // Can stop notifications or syncing of Store apps.
            BackgroundAppsOff => (Privacy, false, false, false),
            GameModeOn | WindowedOptimizationsOn | GameDvrOff => (Games, true, false, false),
            // The ASUS apps stop working; the user chooses.
            AsusServicesOff => (Games, false, false, false),
            GpuSchedulingOn => (Games, true, true, false),
            DefenderExclusions => (Security, true, false, true),
            DefenderRealtimeOff => (Security, false, false, true),
            MemoryIntegrityOff => (Security, false, true, true),
        };
        TweakInfo {
            id: self,
            group,
            recommended,
            reboot,
            security,
        }
    }

    /// The snake_case name used in the protocol.
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub fn from_name(name: &str) -> Option<TweakId> {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
    }
}

/// Why a tweak cannot be changed now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// Tamper Protection is on in Windows Security: Defender ignores the
    /// change until the user turns it off there.
    TamperProtection,
    /// Locked by the firmware (UEFI lock) or by an organisation's policy.
    Locked,
    /// No game folder found to exclude.
    NoGameFolders,
    /// Not supported on this system.
    NotSupported,
}

/// A tweak as the service sees it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TweakState {
    pub id: TweakId,
    /// Every setting of the tweak is in place, whoever set it.
    pub applied: bool,
    /// Canapé applied it (and can put back what was there before).
    #[serde(default)]
    pub by_canape: bool,
    #[serde(default)]
    pub unavailable: Option<Unavailable>,
    /// Changed since the service started and only effective after a
    /// restart.
    #[serde(default)]
    pub reboot_pending: bool,
    /// ASUS services: the ones present. Defender exclusions: the folders
    /// concerned (excluded when applied, to exclude otherwise).
    #[serde(default)]
    pub paths: Vec<String>,
}

/// A preinstalled app the shell offers to remove for the signed-in user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovableApp {
    /// Package family name; the part before `_` is the package name.
    pub family: &'static str,
    /// English product name (the shell translates the localised ones).
    pub name: &'static str,
}

impl RemovableApp {
    /// Package name, as `Get-AppxPackage -Name` takes it.
    pub fn package(&self) -> &'static str {
        self.family.split('_').next().unwrap_or(self.family)
    }

    /// Microsoft Store page to install it again.
    pub fn store_uri(&self) -> String {
        format!("ms-windows-store://pdp/?PFN={}", self.family)
    }

    pub fn find(package: &str) -> Option<&'static RemovableApp> {
        REMOVABLE_APPS
            .iter()
            .find(|a| a.package().eq_ignore_ascii_case(package))
    }
}

const fn app(family: &'static str, name: &'static str) -> RemovableApp {
    RemovableApp { family, name }
}

/// Preinstalled apps of Windows 11 (23H2, 24H2) of no use on a handheld.
/// Only those installed show. Removed for the current user only; each one
/// can be installed again from the Microsoft Store.
pub const REMOVABLE_APPS: &[RemovableApp] = &[
    app("Microsoft.Copilot_8wekyb3d8bbwe", "Copilot"),
    app("Clipchamp.Clipchamp_yxz26nhyzhsrt", "Clipchamp"),
    app("Microsoft.BingNews_8wekyb3d8bbwe", "News"),
    app("Microsoft.BingWeather_8wekyb3d8bbwe", "Weather"),
    app("Microsoft.BingSearch_8wekyb3d8bbwe", "Bing Search"),
    app("Microsoft.GetHelp_8wekyb3d8bbwe", "Get Help"),
    app("Microsoft.Getstarted_8wekyb3d8bbwe", "Tips"),
    app("Microsoft.WindowsFeedbackHub_8wekyb3d8bbwe", "Feedback Hub"),
    app(
        "Microsoft.MicrosoftSolitaireCollection_8wekyb3d8bbwe",
        "Microsoft Solitaire Collection",
    ),
    app("Microsoft.Todos_8wekyb3d8bbwe", "Microsoft To Do"),
    app("MicrosoftTeams_8wekyb3d8bbwe", "Microsoft Teams"),
    app("MSTeams_8wekyb3d8bbwe", "Microsoft Teams"),
    app("Microsoft.OutlookForWindows_8wekyb3d8bbwe", "Outlook"),
    app(
        "microsoft.windowscommunicationsapps_8wekyb3d8bbwe",
        "Mail and Calendar",
    ),
    app("Microsoft.People_8wekyb3d8bbwe", "People"),
    app("Microsoft.WindowsMaps_8wekyb3d8bbwe", "Maps"),
    app(
        "Microsoft.PowerAutomateDesktop_8wekyb3d8bbwe",
        "Power Automate",
    ),
    app(
        "Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe",
        "Sticky Notes",
    ),
    app(
        "Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe",
        "Microsoft 365",
    ),
    app("Microsoft.YourPhone_8wekyb3d8bbwe", "Phone Link"),
    app(
        "MicrosoftCorporationII.MicrosoftFamily_8wekyb3d8bbwe",
        "Family",
    ),
    app(
        "MicrosoftCorporationII.QuickAssist_8wekyb3d8bbwe",
        "Quick Assist",
    ),
    app("Microsoft.Windows.DevHome_8wekyb3d8bbwe", "Dev Home"),
    app("Microsoft.ZuneVideo_8wekyb3d8bbwe", "Movies & TV"),
    app("7EE7776C.LinkedInforWindows_w1wdnht996qgy", "LinkedIn"),
    app("SpotifyAB.SpotifyMusic_zpdnekdrzrea0", "Spotify"),
    app("Disney.37853FC22B2CE_6rarf9sa4v8jt", "Disney+"),
    app(
        "Microsoft.WindowsSoundRecorder_8wekyb3d8bbwe",
        "Sound Recorder",
    ),
    app("Microsoft.WindowsCalculator_8wekyb3d8bbwe", "Calculator"),
    app("Microsoft.Windows.Photos_8wekyb3d8bbwe", "Photos"),
];

/// Packages that must never be removed: the Xbox apps and the Game Bar
/// (Canapé's widget lives there) with what they depend on, the Store, the
/// browser engine and Windows Security. Compared as name prefixes.
pub const PROTECTED_PACKAGES: &[&str] = &[
    "Microsoft.GamingApp",
    "Microsoft.GamingServices",
    "Microsoft.XboxGamingOverlay",
    "Microsoft.XboxGameOverlay",
    "Microsoft.XboxIdentityProvider",
    "Microsoft.XboxSpeechToTextOverlay",
    "Microsoft.Xbox.TCUI",
    "Microsoft.WindowsStore",
    "Microsoft.StorePurchaseApp",
    "Microsoft.DesktopAppInstaller",
    "Microsoft.MicrosoftEdge",
    "Microsoft.Win32WebViewHost",
    "Microsoft.WebView2",
    "Microsoft.SecHealthUI",
    "Microsoft.VCLibs",
    "Microsoft.UI.Xaml",
    "Microsoft.NET.Native",
    "Microsoft.WindowsAppRuntime",
    "Canape",
];

/// True for a package that must stay installed.
pub fn is_protected(package: &str) -> bool {
    PROTECTED_PACKAGES
        .iter()
        .any(|p| package.len() >= p.len() && package[..p.len()].eq_ignore_ascii_case(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_ids_are_unique_and_named() {
        let mut names: Vec<String> = TweakId::ALL.iter().map(|t| t.name()).collect();
        assert!(names.iter().all(|n| !n.is_empty()));
        for id in TweakId::ALL {
            assert_eq!(TweakId::from_name(&id.name()), Some(id));
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TweakId::ALL.len());
        assert_eq!(TweakId::from_name("format_c"), None);
    }

    #[test]
    fn security_tweaks_are_not_recommended_unless_harmless() {
        for id in TweakId::ALL {
            let info = id.info();
            assert_eq!(info.id, id);
            if info.security && info.recommended {
                assert_eq!(id, TweakId::DefenderExclusions);
            }
        }
        assert!(!TweakId::DefenderRealtimeOff.info().recommended);
        assert!(!TweakId::MemoryIntegrityOff.info().recommended);
        assert!(TweakId::GameDvrOff.info().recommended);
        assert!(!TweakId::AsusServicesOff.info().recommended);
    }

    #[test]
    fn removable_apps_never_include_protected_ones() {
        let mut families: Vec<&str> = REMOVABLE_APPS.iter().map(|a| a.family).collect();
        for app in REMOVABLE_APPS {
            assert!(!is_protected(app.package()), "{}", app.family);
            assert!(app.family.contains('_'), "{}", app.family);
            assert_eq!(RemovableApp::find(app.package()), Some(app));
        }
        families.sort();
        families.dedup();
        assert_eq!(families.len(), REMOVABLE_APPS.len());
        assert!(is_protected("Microsoft.XboxGamingOverlay"));
        assert!(is_protected("microsoft.gamingapp"));
        assert!(!is_protected("Microsoft.BingNews"));
    }

    #[test]
    fn store_page_names_the_family() {
        let news = RemovableApp::find("Microsoft.BingNews").unwrap();
        assert_eq!(
            news.store_uri(),
            "ms-windows-store://pdp/?PFN=Microsoft.BingNews_8wekyb3d8bbwe"
        );
    }
}
