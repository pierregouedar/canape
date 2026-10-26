//! Background services of the ASUS software, stopped and disabled by the
//! "ASUS services" tweak. Only the ones installed are touched, and the way
//! each one started is recorded to put it back.
//!
//! The list is a set of well-known service names, not yet checked on an
//! Ally. The ACPI driver Canapé talks to (`ATKACPI`, the ATK package) is a
//! kernel driver and not part of it; [`is_kept`] guards against ever adding
//! it.

use serde::{Deserialize, Serialize};

/// Start type value of a disabled service.
pub const START_DISABLED: u32 = 4;

/// ASUS services that may be disabled.
pub const ASUS_SERVICES: &[&str] = &[
    "ArmouryCrateService",
    "AsusAppService",
    "ASUSOptimization",
    "ASUSSoftwareManager",
    "ASUSSwitch",
    "ASUSSystemAnalysis",
    "ASUSSystemDiagnosis",
    "asus_framework",
    "LightingService",
    "AsusCertService",
];

/// Services and drivers Canapé relies on: never touched.
pub fn is_kept(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("atk") || lower.contains("acpi")
}

/// A service as the system reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceState {
    /// `SERVICE_*_START` value: 2 automatic, 3 manual, 4 disabled.
    pub start: u32,
    /// Automatic start, delayed.
    pub delayed: bool,
    pub running: bool,
}

/// How a service was before Canapé disabled it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedService {
    pub name: String,
    pub start: u32,
    #[serde(default)]
    pub delayed: bool,
    #[serde(default)]
    pub running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_never_includes_what_canape_needs() {
        for name in ASUS_SERVICES {
            assert!(!is_kept(name), "{name}");
        }
        assert!(is_kept("ATKACPI"));
        assert!(is_kept("AsusATKPackage"));
        assert!(!is_kept("ArmouryCrateService"));
        let mut names: Vec<String> = ASUS_SERVICES.iter().map(|s| s.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ASUS_SERVICES.len());
    }
}
