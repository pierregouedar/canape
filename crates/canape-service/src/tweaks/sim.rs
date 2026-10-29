//! A Windows that only exists in memory: registry values, the power plan,
//! ASUS services, Tamper Protection, Defender exclusions and real-time
//! protection. Used by the tests and by the service off Windows (or with
//! `CANAPE_MOCK_TWEAKS`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{PowerSetting, PowerSource, RegData, Scope, ServiceState, System, same_folder};

type Key = (Scope, String, String);

struct Inner {
    registry: BTreeMap<Key, RegData>,
    tamper: bool,
    exclusions: Vec<String>,
    realtime_disabled: bool,
    scheme: String,
    /// Processor settings by plan: AC and DC values.
    power: BTreeMap<(String, PowerSetting), (u32, u32)>,
    commits: u32,
    services: BTreeMap<String, ServiceState>,
    /// Values Windows refuses to write (key, name).
    denied: Vec<(String, String)>,
}

impl Default for Inner {
    fn default() -> Self {
        let service = |start, delayed, running| ServiceState {
            start,
            delayed,
            running,
        };
        Self {
            registry: BTreeMap::new(),
            tamper: false,
            exclusions: Vec::new(),
            realtime_disabled: false,
            scheme: "sim-balanced".into(),
            power: BTreeMap::new(),
            commits: 0,
            denied: Vec::new(),
            // An Ally with the ASUS software installed.
            services: BTreeMap::from([
                ("ArmouryCrateService".into(), service(2, false, true)),
                ("AsusAppService".into(), service(2, true, true)),
                ("ASUSOptimization".into(), service(3, false, false)),
                ("LightingService".into(), service(2, false, true)),
            ]),
        }
    }
}

/// What a plan holds before anyone changes it: AC, DC.
fn power_default(setting: PowerSetting) -> (u32, u32) {
    match setting {
        PowerSetting::Boost => (2, 1),
        PowerSetting::Epp => (33, 66),
        PowerSetting::MaxFrequency => (0, 0),
        PowerSetting::MinCores => (100, 50),
    }
}

/// Cloning shares the same state.
#[derive(Clone, Default)]
pub struct SimSystem(Arc<Mutex<Inner>>);

impl SimSystem {
    pub fn new(tamper_protected: bool) -> Self {
        let sim = Self::default();
        sim.set_tamper(tamper_protected);
        sim
    }

    /// Windows refuses to write this value, as it does for a protected policy.
    #[cfg(test)]
    pub fn deny(&self, key: &str, name: &str) {
        self.0
            .lock()
            .unwrap()
            .denied
            .push((key.into(), name.into()));
    }

    /// Another power plan becomes the active one.
    #[cfg(test)]
    pub fn switch_scheme(&self, scheme: &str) {
        self.0.lock().unwrap().scheme = scheme.into();
    }

    /// How many times Windows was told to apply the plan.
    #[cfg(test)]
    pub fn power_commits(&self) -> u32 {
        self.0.lock().unwrap().commits
    }

    #[cfg(test)]
    pub fn remove_service(&self, name: &str) {
        self.0.lock().unwrap().services.remove(name);
    }

    #[cfg(test)]
    pub fn services(&self) -> BTreeMap<String, ServiceState> {
        self.0.lock().unwrap().services.clone()
    }

    /// Tamper Protection on, as Windows ships, unless
    /// `CANAPE_MOCK_TAMPER=0`.
    pub fn from_env() -> Self {
        Self::new(std::env::var("CANAPE_MOCK_TAMPER").map_or(true, |v| v != "0"))
    }

    pub fn set_tamper(&self, on: bool) {
        self.0.lock().unwrap().tamper = on;
    }

    #[cfg(test)]
    pub fn put(&self, scope: Scope, key: &str, name: &str, data: RegData) {
        self.0
            .lock()
            .unwrap()
            .registry
            .insert(Self::key(scope, key, name), data);
    }

    #[cfg(test)]
    pub fn registry(&self) -> BTreeMap<(Scope, String, String), RegData> {
        self.0.lock().unwrap().registry.clone()
    }

    fn key(scope: Scope, key: &str, name: &str) -> Key {
        (scope, key.to_lowercase(), name.to_lowercase())
    }
}

impl System for SimSystem {
    fn read(&self, scope: Scope, key: &str, name: &str) -> Result<Option<RegData>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .registry
            .get(&Self::key(scope, key, name))
            .cloned())
    }

    fn write(
        &mut self,
        scope: Scope,
        key: &str,
        name: &str,
        data: Option<&RegData>,
    ) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        if data.is_some()
            && inner
                .denied
                .iter()
                .any(|(k, n)| k.eq_ignore_ascii_case(key) && n.eq_ignore_ascii_case(name))
        {
            return Err("error 5".into());
        }
        let key = Self::key(scope, key, name);
        match data {
            Some(data) => inner.registry.insert(key, data.clone()),
            None => inner.registry.remove(&key),
        };
        Ok(())
    }

    fn tamper_protected(&self) -> Option<bool> {
        Some(self.0.lock().unwrap().tamper)
    }

    fn game_folders(&self) -> Vec<PathBuf> {
        [
            r"D:\SteamLibrary\steamapps\common",
            r"C:\Program Files\Epic Games",
            r"D:\Games",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }

    fn defender_exclusions(&self) -> Vec<String> {
        self.0.lock().unwrap().exclusions.clone()
    }

    fn add_defender_exclusions(&mut self, paths: &[String]) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        for path in paths {
            if !inner.exclusions.iter().any(|e| same_folder(e, path)) {
                inner.exclusions.push(path.clone());
            }
        }
        Ok(())
    }

    fn remove_defender_exclusions(&mut self, paths: &[String]) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        inner
            .exclusions
            .retain(|e| !paths.iter().any(|p| same_folder(e, p)));
        Ok(())
    }

    fn realtime_disabled(&self) -> bool {
        self.0.lock().unwrap().realtime_disabled
    }

    fn set_realtime_disabled(&mut self, disabled: bool) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        // Like Windows: silently ignored while Tamper Protection is on.
        if !inner.tamper {
            inner.realtime_disabled = disabled;
        }
        Ok(())
    }

    fn power_scheme(&self) -> Result<String, String> {
        Ok(self.0.lock().unwrap().scheme.clone())
    }

    fn power_get(
        &self,
        scheme: &str,
        setting: PowerSetting,
        source: PowerSource,
    ) -> Result<u32, String> {
        let (ac, dc) = self
            .0
            .lock()
            .unwrap()
            .power
            .get(&(scheme.to_owned(), setting))
            .copied()
            .unwrap_or_else(|| power_default(setting));
        Ok(if source == PowerSource::Ac { ac } else { dc })
    }

    fn power_set(
        &mut self,
        scheme: &str,
        setting: PowerSetting,
        source: PowerSource,
        value: u32,
    ) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        let entry = inner
            .power
            .entry((scheme.to_owned(), setting))
            .or_insert_with(|| power_default(setting));
        match source {
            PowerSource::Ac => entry.0 = value,
            PowerSource::Dc => entry.1 = value,
        }
        Ok(())
    }

    fn power_commit(&mut self, _scheme: &str) -> Result<(), String> {
        self.0.lock().unwrap().commits += 1;
        Ok(())
    }

    fn service(&self, name: &str) -> Option<ServiceState> {
        self.0.lock().unwrap().services.get(name).copied()
    }

    fn set_service_start(&mut self, name: &str, start: u32, delayed: bool) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        let state = inner.services.get_mut(name).ok_or("no such service")?;
        state.start = start;
        state.delayed = delayed;
        Ok(())
    }

    fn stop_service(&mut self, name: &str) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        inner
            .services
            .get_mut(name)
            .ok_or("no such service")?
            .running = false;
        Ok(())
    }

    fn start_service(&mut self, name: &str) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        let state = inner.services.get_mut(name).ok_or("no such service")?;
        if state.start == super::START_DISABLED {
            return Err("service is disabled".into());
        }
        state.running = true;
        Ok(())
    }
}
