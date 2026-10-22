//! Bench runs: the service sweeps power limits on the running game and
//! records what each wattage delivers, to pick the right TDP for it.

use serde::{Deserialize, Serialize};

/// Fraction of the best frame rate that counts as "as good as the maximum".
const SWEET_SPOT_SHARE: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchPlan {
    pub from_watts: u32,
    pub to_watts: u32,
    pub step_watts: u32,
    /// Ignored seconds after each change, while the frame rate settles.
    pub settle_secs: u32,
    pub measure_secs: u32,
}

impl Default for BenchPlan {
    fn default() -> Self {
        Self {
            from_watts: 25,
            to_watts: 7,
            step_watts: 3,
            settle_secs: 5,
            measure_secs: 15,
        }
    }
}

impl BenchPlan {
    /// Wattages to visit, in order, from `from_watts` towards `to_watts`.
    pub fn steps(&self) -> Vec<u32> {
        let step = self.step_watts.max(1);
        let (lo, hi) = (
            self.from_watts.min(self.to_watts),
            self.from_watts.max(self.to_watts),
        );
        let mut watts: Vec<u32> = (lo..=hi).step_by(step as usize).collect();
        if watts.last() != Some(&hi) {
            watts.push(hi);
        }
        if self.from_watts > self.to_watts {
            watts.reverse();
        }
        watts
    }

    pub fn total_secs(&self) -> u32 {
        self.steps().len() as u32 * (self.settle_secs + self.measure_secs)
    }
}

/// What one wattage delivered over its measuring window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchPoint {
    pub watts: u32,
    pub fps_avg: f32,
    /// Lowest one-second frame rate seen: stutters show up here.
    pub fps_low: f32,
    pub gpu_avg: Option<f32>,
    pub temp_max: Option<f32>,
    pub samples: u32,
}

impl BenchPoint {
    pub fn fps_per_watt(&self) -> f32 {
        self.fps_avg / self.watts.max(1) as f32
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    /// Foreground game when the run started.
    pub exe: Option<String>,
    pub started_unix: u64,
    pub plan: BenchPlan,
    /// Sorted by wattage, ascending.
    pub points: Vec<BenchPoint>,
    /// False when cancelled or interrupted.
    pub completed: bool,
}

impl BenchReport {
    fn best_fps(&self) -> Option<f32> {
        self.points.iter().map(|p| p.fps_avg).reduce(f32::max)
    }

    /// Lowest wattage within 5 % of the best frame rate: more power buys
    /// almost nothing past it.
    pub fn sweet_spot(&self) -> Option<u32> {
        let best = self.best_fps()?;
        self.points
            .iter()
            .filter(|p| p.fps_avg >= best * SWEET_SPOT_SHARE)
            .map(|p| p.watts)
            .min()
    }

    /// Lowest wattage that holds `fps` on average. An incomplete run (it
    /// goes from high to low) only answers when a lower wattage it measured
    /// missed: otherwise the answer may lie below what it reached.
    pub fn watts_for(&self, fps: f32) -> Option<u32> {
        let watts = self
            .points
            .iter()
            .filter(|p| p.fps_avg >= fps)
            .map(|p| p.watts)
            .min()?;
        let lowest = self.points.iter().map(|p| p.watts).min()?;
        (self.completed || watts > lowest).then_some(watts)
    }

    /// Best frames per watt: the most battery-friendly setting.
    pub fn most_efficient(&self) -> Option<u32> {
        self.points
            .iter()
            .max_by(|a, b| a.fps_per_watt().total_cmp(&b.fps_per_watt()))
            .map(|p| p.watts)
    }
}

/// Where a running bench stands, for the status.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchProgress {
    /// 1-based index of the current step.
    pub step: u32,
    pub steps: u32,
    pub watts: u32,
    pub seconds_left: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(watts: u32, fps: f32) -> BenchPoint {
        BenchPoint {
            watts,
            fps_avg: fps,
            fps_low: fps - 5.0,
            gpu_avg: None,
            temp_max: None,
            samples: 15,
        }
    }

    fn report() -> BenchReport {
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
    fn an_incomplete_run_does_not_guess_below_what_it_measured() {
        let mut report = BenchReport {
            exe: None,
            started_unix: 0,
            plan: BenchPlan::default(),
            points: vec![point(25, 82.0), point(22, 79.0), point(19, 75.0)],
            completed: false,
        };
        // Everything measured holds 40 FPS: lower might too.
        assert_eq!(report.watts_for(40.0), None);
        // 19 W misses 78 FPS: 22 W is the answer.
        assert_eq!(report.watts_for(78.0), Some(22));
        report.completed = true;
        assert_eq!(report.watts_for(40.0), Some(19));
    }

    #[test]
    fn steps_go_from_high_to_low_and_include_both_ends() {
        assert_eq!(
            BenchPlan::default().steps(),
            vec![25, 22, 19, 16, 13, 10, 7]
        );
        let plan = BenchPlan {
            from_watts: 8,
            to_watts: 15,
            step_watts: 3,
            ..BenchPlan::default()
        };
        assert_eq!(plan.steps(), vec![8, 11, 14, 15]);
        assert_eq!(plan.total_secs(), 4 * 20);
    }

    #[test]
    fn recommendations() {
        let report = report();
        assert_eq!(report.sweet_spot(), Some(19));
        assert_eq!(report.watts_for(60.0), Some(16));
        assert_eq!(report.watts_for(90.0), None);
        assert_eq!(report.most_efficient(), Some(7));
    }

    #[test]
    fn empty_report_has_no_recommendation() {
        let report = BenchReport {
            points: vec![],
            ..report()
        };
        assert_eq!(report.sweet_spot(), None);
        assert_eq!(report.most_efficient(), None);
    }
}
