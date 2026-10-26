//! Processor settings of the active Windows power plan: which settings
//! Canapé writes, the mapping from [`CpuPower`] to their values, and the
//! record that puts back what the plan held before.
//!
//! The plan is the one in use when a setting is first changed; the record
//! names it, so that reverting after the user switched plans still reaches
//! the right one.

use std::collections::BTreeMap;

use canape_core::CpuPower;
use serde::{Deserialize, Serialize};

use super::Tweaks;

/// GUID of the "Processor power management" subgroup.
#[cfg_attr(not(windows), allow(dead_code))]
pub const PROCESSOR_SUBGROUP: &str = "54533251-82be-4824-96c1-47b60b740d00";

/// A setting of that subgroup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSetting {
    /// Processor performance boost mode.
    Boost,
    /// Processor energy performance preference.
    Epp,
    /// Maximum processor frequency, in MHz (0: no limit).
    MaxFrequency,
    /// Minimum percentage of unparked cores.
    MinCores,
}

impl PowerSetting {
    pub const ALL: [PowerSetting; 4] = [
        PowerSetting::Boost,
        PowerSetting::Epp,
        PowerSetting::MaxFrequency,
        PowerSetting::MinCores,
    ];

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn guid(self) -> &'static str {
        match self {
            PowerSetting::Boost => "be337238-0d82-4146-a960-4f3749d470c7",
            PowerSetting::Epp => "36687f9e-e3a5-4dbf-b1dc-15eb381c6863",
            PowerSetting::MaxFrequency => "75b0ae3f-bce0-45a7-8c89-c9611c25e100",
            PowerSetting::MinCores => "0cc5b647-c1df-4637-891a-dec35c318583",
        }
    }

    /// The value this setting takes for `power`, `None` when left alone.
    pub fn wanted(self, power: &CpuPower) -> Option<u32> {
        match self {
            PowerSetting::Boost => power.boost.map(|b| b.mode()),
            PowerSetting::Epp => power.epp,
            PowerSetting::MaxFrequency => power.max_mhz,
            PowerSetting::MinCores => power.min_cores,
        }
    }
}

/// Plugged in, or on battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PowerSource {
    Ac,
    Dc,
}

impl PowerSource {
    pub const BOTH: [PowerSource; 2] = [PowerSource::Ac, PowerSource::Dc];
}

/// The values a setting held before Canapé changed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerPrior {
    pub ac: u32,
    pub dc: u32,
}

/// What Canapé replaced in the power plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerRecord {
    /// GUID of the plan the values belong to.
    pub scheme: Option<String>,
    pub prior: BTreeMap<PowerSetting, PowerPrior>,
}

/// Splits `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` into the fields of a
/// Windows GUID.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_guid(text: &str) -> Option<(u32, u16, u16, [u8; 8])> {
    let groups: Vec<&str> = text.trim_matches(['{', '}']).split('-').collect();
    let lengths: Vec<usize> = groups.iter().map(|g| g.len()).collect();
    if lengths != [8, 4, 4, 4, 12]
        || !text
            .chars()
            .all(|c| c.is_ascii_hexdigit() || "-{}".contains(c))
    {
        return None;
    }
    let data1 = u32::from_str_radix(groups[0], 16).ok()?;
    let data2 = u16::from_str_radix(groups[1], 16).ok()?;
    let data3 = u16::from_str_radix(groups[2], 16).ok()?;
    let tail = format!("{}{}", groups[3], groups[4]);
    let mut data4 = [0u8; 8];
    for (i, byte) in data4.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&tail[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some((data1, data2, data3, data4))
}

impl Tweaks {
    /// What to persist: the values the power plan held before.
    pub fn saved_power(&self) -> PowerRecord {
        self.power.clone()
    }

    /// Takes back the record of a previous run. Nothing is written.
    pub fn restore_power(&mut self, record: PowerRecord) {
        self.power = record;
    }

    /// Brings the plan's processor settings to `want`: each setting given
    /// is written for AC and DC (the first time, after recording what it
    /// held), each one left `None` is put back as it was. An error does
    /// not stop the other settings; the first is returned.
    pub fn set_cpu_power(&mut self, want: &CpuPower) -> Result<(), String> {
        // Nothing asked and nothing to put back: the plan is not even read.
        if want.is_default() && self.power.prior.is_empty() {
            self.power.scheme = None;
            return Ok(());
        }
        let scheme = self.system.power_scheme()?;
        let mut error: Option<String> = None;
        let mut fail = |e: String| {
            error.get_or_insert(e);
        };
        // The user switched plans: leave the old one as it was.
        if self.power.scheme.as_deref().is_some_and(|s| s != scheme) && !self.power.prior.is_empty()
        {
            if let Err(e) = self.put_back(&CpuPower::default()) {
                fail(e);
            }
            if !self.power.prior.is_empty() {
                // Cannot reach it any more; do not keep a record that
                // would be written to the new plan.
                self.power.prior.clear();
            }
        }
        self.power.scheme = Some(scheme.clone());
        let mut changed = false;
        for setting in PowerSetting::ALL {
            let Some(value) = setting.wanted(want) else {
                continue;
            };
            if !self.power.prior.contains_key(&setting) {
                let read = |t: &Self, source| t.system.power_get(&scheme, setting, source);
                match (read(self, PowerSource::Ac), read(self, PowerSource::Dc)) {
                    (Ok(ac), Ok(dc)) => {
                        self.power.prior.insert(setting, PowerPrior { ac, dc });
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        fail(format!("reading {setting:?}: {e}"));
                        continue;
                    }
                }
            }
            for source in PowerSource::BOTH {
                match self.system.power_set(&scheme, setting, source, value) {
                    Ok(()) => changed = true,
                    Err(e) => fail(format!("writing {setting:?}: {e}")),
                }
            }
        }
        match self.put_back(want) {
            Ok(true) => changed = true,
            Ok(false) => {}
            Err(e) => fail(e),
        }
        if changed && let Err(e) = self.system.power_commit(&scheme) {
            fail(format!("applying the power plan: {e}"));
        }
        if self.power.prior.is_empty() {
            self.power.scheme = None;
        }
        error.map_or(Ok(()), Err)
    }

    /// Writes back the settings `want` leaves alone and Canapé had changed;
    /// true when something was written.
    fn put_back(&mut self, want: &CpuPower) -> Result<bool, String> {
        let Some(scheme) = self.power.scheme.clone() else {
            return Ok(false);
        };
        let mut error = None;
        let mut written = false;
        for setting in PowerSetting::ALL {
            if setting.wanted(want).is_some() {
                continue;
            }
            let Some(prior) = self.power.prior.get(&setting).copied() else {
                continue;
            };
            let mut done = true;
            for (source, value) in [(PowerSource::Ac, prior.ac), (PowerSource::Dc, prior.dc)] {
                match self.system.power_set(&scheme, setting, source, value) {
                    Ok(()) => written = true,
                    Err(e) => {
                        done = false;
                        error.get_or_insert(format!("restoring {setting:?}: {e}"));
                    }
                }
            }
            if done {
                self.power.prior.remove(&setting);
            }
        }
        match error {
            Some(e) => Err(e),
            None => Ok(written),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::{SimSystem, System};
    use canape_core::CpuBoost;

    fn engine() -> (Tweaks, SimSystem) {
        let sim = SimSystem::new(true);
        (Tweaks::new(Box::new(sim.clone())), sim)
    }

    fn held(sim: &SimSystem, setting: PowerSetting) -> (u32, u32) {
        let scheme = sim.power_scheme().unwrap();
        (
            sim.power_get(&scheme, setting, PowerSource::Ac).unwrap(),
            sim.power_get(&scheme, setting, PowerSource::Dc).unwrap(),
        )
    }

    #[test]
    fn every_guid_parses() {
        assert_eq!(
            parse_guid(PROCESSOR_SUBGROUP),
            Some((
                0x5453_3251,
                0x82be,
                0x4824,
                [0x96, 0xc1, 0x47, 0xb6, 0x0b, 0x74, 0x0d, 0x00]
            ))
        );
        for setting in PowerSetting::ALL {
            assert!(parse_guid(setting.guid()).is_some(), "{setting:?}");
        }
        assert_eq!(parse_guid("not a guid"), None);
        assert_eq!(parse_guid("be337238-0d82-4146-a960-4f3749d470c"), None);
        assert_eq!(parse_guid("zz337238-0d82-4146-a960-4f3749d470c7"), None);
    }

    #[test]
    fn settings_map_to_the_plan_values() {
        let power = CpuPower {
            boost: Some(CpuBoost::Aggressive),
            epp: Some(70),
            max_mhz: Some(0),
            min_cores: None,
        };
        assert_eq!(PowerSetting::Boost.wanted(&power), Some(2));
        assert_eq!(PowerSetting::Epp.wanted(&power), Some(70));
        assert_eq!(PowerSetting::MaxFrequency.wanted(&power), Some(0));
        assert_eq!(PowerSetting::MinCores.wanted(&power), None);
        assert_eq!(PowerSetting::Boost.wanted(&CpuPower::default()), None);
    }

    #[test]
    fn writes_ac_and_dc_and_puts_back_exactly() {
        let (mut tweaks, sim) = engine();
        let before: Vec<_> = PowerSetting::ALL.iter().map(|&s| held(&sim, s)).collect();
        let power = CpuPower {
            boost: Some(CpuBoost::Disabled),
            epp: Some(90),
            max_mhz: Some(2800),
            min_cores: Some(100),
        };
        assert_eq!(tweaks.set_cpu_power(&power), Ok(()));
        assert_eq!(held(&sim, PowerSetting::Boost), (0, 0));
        assert_eq!(held(&sim, PowerSetting::Epp), (90, 90));
        assert_eq!(held(&sim, PowerSetting::MaxFrequency), (2800, 2800));
        assert_eq!(held(&sim, PowerSetting::MinCores), (100, 100));
        assert_eq!(sim.power_commits(), 1);
        // Changing a value keeps the first record.
        let power = CpuPower {
            epp: Some(10),
            ..power
        };
        assert_eq!(tweaks.set_cpu_power(&power), Ok(()));
        assert_eq!(held(&sim, PowerSetting::Epp), (10, 10));
        // A setting left alone goes back, the others stay.
        let power = CpuPower {
            boost: None,
            ..power
        };
        assert_eq!(tweaks.set_cpu_power(&power), Ok(()));
        assert_eq!(held(&sim, PowerSetting::Boost), before[0]);
        assert_eq!(held(&sim, PowerSetting::Epp), (10, 10));
        assert_eq!(tweaks.saved_power().prior.len(), 3);
        assert_eq!(tweaks.set_cpu_power(&CpuPower::default()), Ok(()));
        let after: Vec<_> = PowerSetting::ALL.iter().map(|&s| held(&sim, s)).collect();
        assert_eq!(after, before);
        assert_eq!(tweaks.saved_power(), PowerRecord::default());
    }

    #[test]
    fn ac_and_dc_are_restored_separately() {
        let (mut tweaks, mut sim) = engine();
        let scheme = sim.power_scheme().unwrap();
        sim.power_set(&scheme, PowerSetting::Epp, PowerSource::Ac, 33)
            .unwrap();
        sim.power_set(&scheme, PowerSetting::Epp, PowerSource::Dc, 66)
            .unwrap();
        let power = CpuPower {
            epp: Some(0),
            ..CpuPower::default()
        };
        tweaks.set_cpu_power(&power).unwrap();
        assert_eq!(held(&sim, PowerSetting::Epp), (0, 0));
        tweaks.set_cpu_power(&CpuPower::default()).unwrap();
        assert_eq!(held(&sim, PowerSetting::Epp), (33, 66));
    }

    #[test]
    fn the_record_survives_a_restart() {
        let (mut tweaks, sim) = engine();
        let before = held(&sim, PowerSetting::Epp);
        let power = CpuPower {
            epp: Some(5),
            ..CpuPower::default()
        };
        tweaks.set_cpu_power(&power).unwrap();
        let json = serde_json::to_string(&tweaks.saved_power()).unwrap();
        let mut again = Tweaks::new(Box::new(sim.clone()));
        again.restore_power(serde_json::from_str(&json).unwrap());
        again.set_cpu_power(&power).unwrap();
        again.set_cpu_power(&CpuPower::default()).unwrap();
        assert_eq!(held(&sim, PowerSetting::Epp), before);
    }

    #[test]
    fn switching_plans_leaves_the_old_one_as_it_was() {
        let (mut tweaks, sim) = engine();
        let old = sim.power_scheme().unwrap();
        let before = held(&sim, PowerSetting::Epp);
        let power = CpuPower {
            epp: Some(5),
            ..CpuPower::default()
        };
        tweaks.set_cpu_power(&power).unwrap();
        sim.switch_scheme("other");
        tweaks.set_cpu_power(&power).unwrap();
        let ac = sim
            .power_get(&old, PowerSetting::Epp, PowerSource::Ac)
            .unwrap();
        assert_eq!(ac, before.0);
        assert_eq!(held(&sim, PowerSetting::Epp), (5, 5));
        assert_eq!(tweaks.saved_power().scheme.as_deref(), Some("other"));
    }

    #[test]
    fn revert_all_puts_the_plan_back() {
        let (mut tweaks, sim) = engine();
        let before = held(&sim, PowerSetting::MaxFrequency);
        tweaks
            .set_cpu_power(&CpuPower {
                max_mhz: Some(2000),
                ..CpuPower::default()
            })
            .unwrap();
        assert_eq!(tweaks.revert_all(), Ok(()));
        assert_eq!(held(&sim, PowerSetting::MaxFrequency), before);
    }
}
