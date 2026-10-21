//! The last minute of power draw, power limit and GPU activity, drawn as
//! small graphs: one sample per status push (about once per second), kept
//! in a ring buffer and turned into Slint `Path` commands.
//!
//! Paths use a viewbox of `(LEN - 1) x 100` stretched over the graph, so
//! x is the sample index and y the share of the scale, top down.

use std::collections::VecDeque;
use std::fmt::Write;
use std::time::{Duration, Instant};

use canape_core::Status;

use crate::GraphView;

/// Samples shown: one minute at one push per second.
pub const LEN: usize = 60;
/// Pushes closer than this (the service also pushes after every change)
/// replace the last sample instead of adding one.
const MIN_GAP: Duration = Duration::from_millis(700);
const HEIGHT: f32 = 100.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Sample {
    /// Watts drawn.
    power: Option<f32>,
    /// Power limit in watts.
    limit: Option<f32>,
    /// GPU activity in `[0, 1]`.
    gpu: Option<f32>,
}

#[derive(Debug, Default)]
pub struct History {
    samples: VecDeque<Sample>,
    last: Option<Instant>,
}

impl History {
    /// Records `status` (`None`: no service, a gap in the graphs).
    pub fn push(&mut self, status: Option<&Status>, now: Instant) {
        let sample = status.map_or(Sample::default(), |s| Sample {
            power: s.power_watts.filter(|w| w.is_finite() && *w >= 0.0),
            limit: s.tdp_watts.map(|w| w as f32),
            gpu: s
                .gpu_util
                .filter(|g| g.is_finite())
                .map(|g| g.clamp(0.0, 1.0)),
        });
        let close = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < MIN_GAP);
        if close && !self.samples.is_empty() {
            *self.samples.back_mut().unwrap() = sample;
            return;
        }
        if self.samples.len() == LEN {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        self.last = Some(now);
    }

    /// Top of the power graph in watts: room above the highest value of the
    /// minute, in steps of 5 W.
    fn power_scale(&self) -> u32 {
        let top = self
            .samples
            .iter()
            .flat_map(|s| [s.power, s.limit])
            .flatten()
            .fold(0.0f32, f32::max);
        (((top * 1.1) / 5.0).ceil() as u32 * 5).max(10)
    }

    /// x of sample `i`: the newest sample sits on the right edge.
    fn x(&self, i: usize) -> f32 {
        (LEN - self.samples.len() + i) as f32
    }

    /// Runs of consecutive known values, as (x, y) points.
    fn runs(&self, value: impl Fn(&Sample) -> Option<f32>, scale: f32) -> Vec<Vec<(f32, f32)>> {
        let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
        let mut open = false;
        for (i, sample) in self.samples.iter().enumerate() {
            match value(sample) {
                Some(v) => {
                    let y = HEIGHT - (v / scale * HEIGHT).clamp(0.0, HEIGHT);
                    if !open {
                        runs.push(Vec::new());
                        open = true;
                    }
                    runs.last_mut().unwrap().push((self.x(i), y));
                }
                None => open = false,
            }
        }
        runs
    }

    pub fn view(&self) -> GraphView {
        let scale = self.power_scale();
        let power = self.runs(|s| s.power, scale as f32);
        let limit = self.runs(|s| s.limit, scale as f32);
        let gpu = self.runs(|s| s.gpu, 1.0);
        GraphView {
            power_scale: scale as i32,
            power_line: line(&power).into(),
            power_area: area(&power).into(),
            limit_line: steps(&limit).into(),
            gpu_line: line(&gpu).into(),
            gpu_area: area(&gpu).into(),
        }
    }
}

/// Polylines through each run; a lone point becomes a short dash so it
/// still shows.
fn line(runs: &[Vec<(f32, f32)>]) -> String {
    let mut out = String::new();
    for run in runs {
        let (x, y) = run[0];
        let _ = write!(out, "M {x} {y} ");
        if run.len() == 1 {
            let _ = write!(out, "L {} {y} ", x - 0.5);
        }
        for (x, y) in &run[1..] {
            let _ = write!(out, "L {x} {y} ");
        }
    }
    out.trim_end().to_owned()
}

/// Closed shapes between each run and the baseline.
fn area(runs: &[Vec<(f32, f32)>]) -> String {
    let mut out = String::new();
    for run in runs.iter().filter(|run| run.len() > 1) {
        let (first, last) = (run[0].0, run[run.len() - 1].0);
        let _ = write!(out, "M {first} {HEIGHT} ");
        for (x, y) in run {
            let _ = write!(out, "L {x} {y} ");
        }
        let _ = write!(out, "L {last} {HEIGHT} Z ");
    }
    out.trim_end().to_owned()
}

/// A limit holds until it changes: horizontal steps rather than slopes.
fn steps(runs: &[Vec<(f32, f32)>]) -> String {
    let mut out = String::new();
    for run in runs {
        let (x, y) = run[0];
        let _ = write!(out, "M {x} {y} ");
        if run.len() == 1 {
            let _ = write!(out, "L {} {y} ", x - 0.5);
        }
        let mut level = y;
        for &(x, y) in &run[1..] {
            let _ = write!(out, "L {x} {level} ");
            if y != level {
                let _ = write!(out, "L {x} {y} ");
                level = y;
            }
        }
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(power: f32, tdp: u32, gpu: f32) -> Status {
        Status {
            power_watts: Some(power),
            tdp_watts: Some(tdp),
            gpu_util: Some(gpu),
            ..Status::default()
        }
    }

    #[test]
    fn keeps_one_minute_one_sample_per_second() {
        let mut history = History::default();
        let t = Instant::now();
        for i in 0..100 {
            history.push(Some(&status(10.0, 12, 0.5)), t + Duration::from_secs(i));
        }
        assert_eq!(history.samples.len(), LEN);
        // An extra push right after a change replaces the last sample.
        history.push(
            Some(&status(20.0, 22, 0.5)),
            t + Duration::from_millis(99_200),
        );
        assert_eq!(history.samples.len(), LEN);
        assert_eq!(history.samples.back().unwrap().limit, Some(22.0));
    }

    #[test]
    fn scale_leaves_room_above_the_limit() {
        let mut history = History::default();
        let t = Instant::now();
        history.push(Some(&status(4.0, 7, 0.1)), t);
        assert_eq!(history.power_scale(), 10);
        history.push(Some(&status(22.5, 25, 0.9)), t + Duration::from_secs(1));
        assert_eq!(history.power_scale(), 30);
    }

    #[test]
    fn paths_start_on_the_right_and_break_on_gaps() {
        let mut history = History::default();
        let t = Instant::now();
        history.push(Some(&status(9.0, 10, 0.5)), t);
        history.push(Some(&status(9.0, 10, 0.25)), t + Duration::from_secs(1));
        history.push(None, t + Duration::from_secs(2));
        history.push(Some(&status(9.0, 10, 1.0)), t + Duration::from_secs(3));
        let view = history.view();
        assert_eq!(view.power_scale, 15);
        // Four samples: x from 56 to 59; the gap splits the GPU line.
        assert_eq!(view.gpu_line, "M 56 50 L 57 75 M 59 0 L 58.5 0");
        assert_eq!(view.gpu_area, "M 56 100 L 56 50 L 57 75 L 57 100 Z");
        assert_eq!(view.limit_line.matches('M').count(), 2);
    }

    #[test]
    fn limit_draws_as_steps() {
        let runs = vec![vec![(0.0, 50.0), (1.0, 50.0), (2.0, 20.0)]];
        assert_eq!(steps(&runs), "M 0 50 L 1 50 L 2 50 L 2 20");
    }

    #[test]
    fn empty_history_draws_nothing() {
        let view = History::default().view();
        assert_eq!(view.power_line, "");
        assert_eq!(view.limit_line, "");
        assert_eq!(view.power_scale, 10);
    }
}
