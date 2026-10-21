//! A bench in progress: walks the plan's wattages, lets each one settle,
//! then averages what the game delivers.

use canape_core::{BenchPlan, BenchPoint, BenchProgress, BenchReport};

/// Seconds below this frame rate (game paused, alt-tabbed) are not counted.
const MIN_FPS: f32 = 5.0;

/// One second of measurements.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reading {
    pub fps: Option<f32>,
    pub gpu_util: Option<f32>,
    pub cpu_temp: Option<f32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Apply this power limit now.
    SetTdp(u32),
    Done,
}

pub struct BenchRun {
    plan: BenchPlan,
    steps: Vec<u32>,
    index: usize,
    tick_in_step: u32,
    fps: Vec<f32>,
    gpu: Vec<f32>,
    temp_max: Option<f32>,
    report: BenchReport,
}

impl BenchRun {
    /// The run and the first wattage to apply.
    pub fn new(plan: BenchPlan, exe: Option<String>, started_unix: u64) -> (Self, u32) {
        let steps = plan.steps();
        let first = steps[0];
        let run = Self {
            plan,
            steps,
            index: 0,
            tick_in_step: 0,
            fps: Vec::new(),
            gpu: Vec::new(),
            temp_max: None,
            report: BenchReport {
                exe,
                started_unix,
                plan,
                points: Vec::new(),
                completed: false,
            },
        };
        (run, first)
    }

    pub fn tick(&mut self, reading: Reading) -> Option<Step> {
        self.tick_in_step += 1;
        if self.tick_in_step > self.plan.settle_secs {
            if let Some(fps) = reading.fps.filter(|&fps| fps >= MIN_FPS) {
                self.fps.push(fps);
            }
            if let Some(gpu) = reading.gpu_util {
                self.gpu.push(gpu);
            }
            if let Some(temp) = reading.cpu_temp {
                self.temp_max = Some(self.temp_max.map_or(temp, |max| max.max(temp)));
            }
        }
        if self.tick_in_step < self.plan.settle_secs + self.plan.measure_secs {
            return None;
        }
        self.finish_step();
        self.index += 1;
        self.tick_in_step = 0;
        match self.steps.get(self.index) {
            Some(&watts) => Some(Step::SetTdp(watts)),
            None => {
                self.report.completed = true;
                Some(Step::Done)
            }
        }
    }

    fn finish_step(&mut self) {
        let fps = std::mem::take(&mut self.fps);
        let gpu = std::mem::take(&mut self.gpu);
        let temp_max = self.temp_max.take();
        if fps.is_empty() {
            return;
        }
        let average = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        self.report.points.push(BenchPoint {
            watts: self.steps[self.index],
            fps_avg: average(&fps),
            fps_low: fps.iter().copied().fold(f32::INFINITY, f32::min),
            gpu_avg: (!gpu.is_empty()).then(|| average(&gpu)),
            temp_max,
            samples: fps.len() as u32,
        });
    }

    pub fn progress(&self) -> BenchProgress {
        let per_step = self.plan.settle_secs + self.plan.measure_secs;
        let remaining_steps = (self.steps.len() - self.index) as u32;
        BenchProgress {
            step: self.index as u32 + 1,
            steps: self.steps.len() as u32,
            watts: self.steps[self.index.min(self.steps.len() - 1)],
            seconds_left: (remaining_steps * per_step).saturating_sub(self.tick_in_step),
        }
    }

    /// The report so far, points sorted by wattage.
    pub fn into_report(self) -> BenchReport {
        let mut report = self.report;
        report.points.sort_by_key(|p| p.watts);
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> BenchPlan {
        BenchPlan {
            from_watts: 20,
            to_watts: 10,
            step_watts: 5,
            settle_secs: 2,
            measure_secs: 3,
        }
    }

    #[test]
    fn walks_every_step_and_measures_after_settling() {
        let (mut run, first) = BenchRun::new(plan(), Some("game.exe".into()), 0);
        assert_eq!(first, 20);
        let mut steps = Vec::new();
        for t in 0..15 {
            // Settling seconds read 0 FPS: they must not count.
            let fps = if t % 5 < 2 {
                0.0
            } else {
                3.0 * [20.0, 15.0, 10.0][t / 5]
            };
            let reading = Reading {
                fps: Some(fps),
                gpu_util: Some(0.5),
                cpu_temp: Some(60.0 + t as f32),
            };
            if let Some(step) = run.tick(reading) {
                steps.push(step);
            }
        }
        assert_eq!(steps, vec![Step::SetTdp(15), Step::SetTdp(10), Step::Done]);
        let report = run.into_report();
        assert!(report.completed);
        let watts: Vec<_> = report.points.iter().map(|p| p.watts).collect();
        assert_eq!(watts, vec![10, 15, 20]);
        assert_eq!(report.points[2].fps_avg, 60.0);
        assert_eq!(report.points[2].fps_low, 60.0);
        assert_eq!(report.points[2].samples, 3);
        assert_eq!(report.points[0].temp_max, Some(74.0));
    }

    #[test]
    fn progress_counts_down() {
        let (mut run, _) = BenchRun::new(plan(), None, 0);
        assert_eq!(run.progress().seconds_left, 15);
        run.tick(Reading::default());
        let progress = run.progress();
        assert_eq!((progress.step, progress.steps, progress.watts), (1, 3, 20));
        assert_eq!(progress.seconds_left, 14);
    }

    #[test]
    fn steps_without_frames_are_left_out() {
        let (mut run, _) = BenchRun::new(plan(), None, 0);
        for _ in 0..15 {
            run.tick(Reading::default());
        }
        let report = run.into_report();
        assert!(report.completed);
        assert!(report.points.is_empty());
    }
}
