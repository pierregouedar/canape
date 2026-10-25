//! A Windows that only exists in memory: registry values, Tamper
//! Protection, Defender exclusions and real-time protection. Used by the
//! tests and by the service off Windows (or with `CANAPE_MOCK_TWEAKS`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{RegData, Scope, System, same_folder};

type Key = (Scope, String, String);

#[derive(Default)]
struct Inner {
    registry: BTreeMap<Key, RegData>,
    tamper: bool,
    exclusions: Vec<String>,
    realtime_disabled: bool,
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
}
