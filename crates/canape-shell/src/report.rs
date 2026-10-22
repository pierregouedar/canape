//! Bench report → what the bench page shows and what its "apply" actions
//! send. Pure logic; the strings are formatted and translated in Slint.

use canape_core::{AutoTdpSettings, BenchReport, Profile};

/// Frame-rate targets offered in the UI, ascending.
pub const TARGETS: [u32; 6] = [30, 40, 45, 60, 90, 120];

/// One recommended wattage and what it delivered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recommendation {
    pub watts: u32,
    pub fps: f32,
}

/// The three recommendations of a report, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Recommendations {
    /// Lowest wattage within 5 % of the best frame rate.
    pub sweet_spot: Option<Recommendation>,
    /// Lowest wattage that holds the auto TDP target.
    pub for_target: Option<Recommendation>,
    /// Best frames per watt.
    pub most_efficient: Option<Recommendation>,
}

fn at(report: &BenchReport, watts: Option<u32>) -> Option<Recommendation> {
    let watts = watts?;
    let point = report.points.iter().find(|p| p.watts == watts)?;
    Some(Recommendation {
        watts,
        fps: point.fps_avg,
    })
}

pub fn recommendations(report: &BenchReport, target_fps: u32) -> Recommendations {
    Recommendations {
        sweet_spot: at(report, report.sweet_spot()),
        for_target: at(report, report.watts_for(target_fps as f32)),
        most_efficient: at(report, report.most_efficient()),
    }
}

/// Highest offered target a frame rate holds, or the frame rate itself
/// (rounded down) below the lowest one.
pub fn snap_target(fps: f32) -> u32 {
    let floor = fps.max(1.0).floor() as u32;
    TARGETS
        .iter()
        .copied()
        .rev()
        .find(|&t| t <= floor)
        .unwrap_or(floor)
}

/// Game profile for a recommendation: auto TDP aims at what `watts`
/// delivered and never goes above it; the battery and temperature limits
/// stay as they are.
pub fn profile_for(
    report: &BenchReport,
    rec: Recommendation,
    exe: &str,
    current: AutoTdpSettings,
) -> Profile {
    let lowest = report
        .points
        .iter()
        .map(|p| p.watts)
        .min()
        .unwrap_or(rec.watts);
    Profile {
        exe: exe.to_owned(),
        auto_tdp: AutoTdpSettings {
            enabled: true,
            target_fps: snap_target(rec.fps),
            min_watts: current.min_watts.min(lowest).min(rec.watts),
            max_watts: rec.watts,
            ..current
        },
        learned_watts: Some(rec.watts),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_core::{BenchPlan, BenchPoint};

    fn report() -> BenchReport {
        let point = |watts, fps_avg| BenchPoint {
            watts,
            fps_avg,
            fps_low: fps_avg - 6.0,
            gpu_avg: Some(0.9),
            temp_max: Some(70.0),
            samples: 15,
        };
        BenchReport {
            exe: Some("game.exe".into()),
            started_unix: 0,
            plan: BenchPlan::default(),
            points: vec![
                point(7, 31.0),
                point(10, 44.0),
                point(13, 55.0),
                point(16, 61.0),
                point(19, 64.0),
                point(22, 65.0),
                point(25, 65.5),
            ],
            completed: true,
        }
    }

    #[test]
    fn three_recommendations() {
        let recs = recommendations(&report(), 60);
        assert_eq!(
            recs.sweet_spot,
            Some(Recommendation {
                watts: 19,
                fps: 64.0
            })
        );
        assert_eq!(
            recs.for_target,
            Some(Recommendation {
                watts: 16,
                fps: 61.0
            })
        );
        assert_eq!(
            recs.most_efficient,
            Some(Recommendation {
                watts: 7,
                fps: 31.0
            })
        );
        assert_eq!(recommendations(&report(), 90).for_target, None);
    }

    #[test]
    fn targets_snap_down_to_offered_values() {
        assert_eq!(snap_target(64.0), 60);
        assert_eq!(snap_target(44.9), 40);
        assert_eq!(snap_target(130.0), 120);
        assert_eq!(snap_target(31.0), 30);
        assert_eq!(snap_target(24.7), 24);
        assert_eq!(snap_target(0.0), 1);
    }

    #[test]
    fn profile_caps_power_at_the_recommendation() {
        let current = AutoTdpSettings {
            min_watts: 9,
            battery_max_watts: Some(15),
            max_temp_c: Some(85),
            ..AutoTdpSettings::default()
        };
        let report = report();
        let rec = recommendations(&report, 60).sweet_spot.unwrap();
        let profile = profile_for(&report, rec, "game.exe", current);
        assert_eq!(profile.exe, "game.exe");
        assert_eq!(profile.learned_watts, Some(19));
        assert_eq!(
            profile.auto_tdp,
            AutoTdpSettings {
                enabled: true,
                target_fps: 60,
                min_watts: 7,
                max_watts: 19,
                battery_max_watts: Some(15),
                max_temp_c: Some(85),
            }
        );
    }
}
