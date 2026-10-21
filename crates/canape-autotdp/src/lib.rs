//! Auto TDP: keep the game at its target frame rate with as few watts as possible.
//!
//! The controller is a pure state machine fed once per tick (about one second)
//! with a [`Sample`]. It never touches the hardware: the service applies the
//! wattage it returns.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub target_fps: f32,
    pub min_watts: u32,
    pub max_watts: u32,
    pub step_up_watts: u32,
    pub step_down_watts: u32,
    /// Below `target * (1 - tolerance)` the game is starving: raise power.
    pub tolerance: f32,
    /// Above `target * (1 + headroom)` there is spare power: lower it.
    pub headroom: f32,
    /// GPU utilization under which a game sitting at its frame cap has spare power.
    pub gpu_idle_threshold: f32,
    /// Ticks to wait after a change before raising / lowering again.
    pub cooldown_up_ticks: u32,
    pub cooldown_down_ticks: u32,
    /// Exponential smoothing factor applied to FPS, in `(0, 1]`.
    pub fps_smoothing: f32,
    /// After a raise, the new wattage is a floor for this many ticks; the
    /// hold doubles at each raise, up to `floor_hold_max_ticks`.
    pub floor_hold_ticks: u32,
    pub floor_hold_max_ticks: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_fps: 60.0,
            min_watts: 7,
            max_watts: 25,
            step_up_watts: 2,
            step_down_watts: 1,
            tolerance: 0.05,
            headroom: 0.10,
            gpu_idle_threshold: 0.80,
            // Two ticks let the smoothed FPS catch up, avoiding overshoot.
            cooldown_up_ticks: 2,
            cooldown_down_ticks: 3,
            fps_smoothing: 0.4,
            floor_hold_ticks: 30,
            floor_hold_max_ticks: 480,
        }
    }
}

/// One measurement of the foreground game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub fps: f32,
    /// GPU busy ratio in `[0, 1]`, when known.
    pub gpu_util: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct Controller {
    config: Config,
    watts: u32,
    smoothed_fps: Option<f32>,
    ticks_since_change: u32,
    /// Wattage that recently proved necessary, and ticks left to honor it.
    floor: Option<(u32, u32)>,
    floor_hold: u32,
    /// The last change lowered power: a deficit now means the probe failed.
    probing: bool,
}

impl Controller {
    pub fn new(config: Config, initial_watts: u32) -> Self {
        Self {
            watts: initial_watts.clamp(config.min_watts, config.max_watts),
            smoothed_fps: None,
            // The first sample may raise power at once.
            ticks_since_change: config.cooldown_up_ticks.saturating_sub(1),
            floor: None,
            floor_hold: config.floor_hold_ticks,
            probing: false,
            config,
        }
    }

    pub fn watts(&self) -> u32 {
        self.watts
    }

    pub fn smoothed_fps(&self) -> Option<f32> {
        self.smoothed_fps
    }

    /// Feeds one sample; returns the new wattage when it changed.
    ///
    /// Raises power as soon as the game falls below its target. Lowers it
    /// only when there is clear spare power: frame rate well above target, or
    /// sitting at the target (frame cap) with an idle GPU. GPU activity alone
    /// is not trusted below the target: on the Ally it stays around 50 % even
    /// when the frame rate depends on power (docs/m4-report.md). A wattage
    /// that had to be raised to becomes a floor for a while, with a hold that
    /// grows each time, so probing for a lower wattage gets rarer instead of
    /// sawing up and down.
    pub fn update(&mut self, sample: Sample) -> Option<u32> {
        let c = self.config;
        let fps = match self.smoothed_fps {
            Some(prev) => prev + c.fps_smoothing * (sample.fps - prev),
            None => sample.fps,
        };
        self.smoothed_fps = Some(fps);
        self.ticks_since_change = self.ticks_since_change.saturating_add(1);
        self.floor = self
            .floor
            .and_then(|(watts, left)| (left > 1).then(|| (watts, left - 1)));

        let low = c.target_fps * (1.0 - c.tolerance);
        let high = c.target_fps * (1.0 + c.headroom);
        let at_target = fps >= c.target_fps * 0.98;
        let gpu_has_slack = sample
            .gpu_util
            .is_some_and(|util| util < c.gpu_idle_threshold);
        let floor = self.floor.map_or(c.min_watts, |(watts, _)| watts);

        let next = if fps < low && self.ticks_since_change >= c.cooldown_up_ticks {
            // Recover faster from a big deficit.
            let step = if fps < c.target_fps * 0.8 {
                c.step_up_watts * 2
            } else {
                c.step_up_watts
            };
            let raised = self.watts.saturating_add(step).min(c.max_watts);
            if raised != self.watts {
                // Only a failed probe makes the next one rarer; a plain ramp
                // up (game start, heavier scene) does not.
                if self.probing {
                    self.floor_hold = (self.floor_hold * 2).min(c.floor_hold_max_ticks);
                }
                self.floor = Some((raised, self.floor_hold));
            }
            raised
        } else if (fps > high || (at_target && gpu_has_slack))
            && self.ticks_since_change >= c.cooldown_down_ticks
        {
            self.watts
                .saturating_sub(c.step_down_watts)
                .max(c.min_watts)
                .max(floor.min(self.watts))
        } else {
            self.watts
        };

        if next == self.watts {
            return None;
        }
        self.probing = next < self.watts;
        self.watts = next;
        self.ticks_since_change = 0;
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(fps: f32) -> Sample {
        Sample {
            fps,
            gpu_util: None,
        }
    }

    #[test]
    fn raises_power_when_below_target() {
        let mut ctl = Controller::new(Config::default(), 10);
        assert_eq!(ctl.update(sample(55.0)), Some(12));
    }

    #[test]
    fn big_deficit_uses_a_double_step() {
        let mut ctl = Controller::new(Config::default(), 10);
        assert_eq!(ctl.update(sample(30.0)), Some(14));
    }

    #[test]
    fn lowers_power_after_cooldown_when_over_target() {
        let mut ctl = Controller::new(Config::default(), 15);
        assert_eq!(ctl.update(sample(90.0)), None);
        assert_eq!(ctl.update(sample(90.0)), Some(14));
        assert_eq!(ctl.update(sample(90.0)), None);
    }

    #[test]
    fn frame_capped_game_with_idle_gpu_loses_power() {
        let mut ctl = Controller::new(Config::default(), 15);
        // At its frame cap with an idle GPU: lower, every cooldown.
        let capped = Sample {
            fps: 60.0,
            gpu_util: Some(0.5),
        };
        let changes: Vec<_> = (0..6).filter_map(|_| ctl.update(capped)).collect();
        assert_eq!(changes, vec![14, 13]);
    }

    #[test]
    fn holds_steady_on_target_with_busy_gpu() {
        let mut ctl = Controller::new(Config::default(), 15);
        let steady = Sample {
            fps: 60.0,
            gpu_util: Some(0.95),
        };
        assert!((0..10).all(|_| ctl.update(steady).is_none()));
        assert_eq!(ctl.watts(), 15);
    }

    #[test]
    fn stays_within_bounds() {
        let config = Config::default();
        let mut ctl = Controller::new(config, 24);
        for _ in 0..10 {
            ctl.update(sample(20.0));
        }
        assert_eq!(ctl.watts(), config.max_watts);

        let mut ctl = Controller::new(config, 8);
        for _ in 0..20 {
            ctl.update(sample(200.0));
        }
        assert_eq!(ctl.watts(), config.min_watts);
    }

    #[test]
    fn initial_watts_are_clamped() {
        assert_eq!(Controller::new(Config::default(), 100).watts(), 25);
    }

    /// A game whose frame rate follows power (≈ 3.2 FPS per watt, so the
    /// 60 FPS target needs ≈ 19 W), with ±2 FPS of jitter and a GPU that
    /// always reads 50 %: the M4 case that used to oscillate 16–24 W.
    fn simulate(ticks: usize) -> (Controller, Vec<u32>, Vec<f32>) {
        let mut ctl = Controller::new(Config::default(), 7);
        let (mut watts, mut fps_seen) = (Vec::new(), Vec::new());
        for t in 0..ticks {
            let jitter = 2.0 * ((t as f32) * 0.7).sin();
            let fps = (3.2 * ctl.watts() as f32).min(120.0) + jitter;
            ctl.update(Sample {
                fps,
                gpu_util: Some(0.5),
            });
            watts.push(ctl.watts());
            fps_seen.push(fps);
        }
        (ctl, watts, fps_seen)
    }

    #[test]
    fn power_limited_game_settles_without_sawtooth() {
        let (_, watts, fps) = simulate(900);
        let settled = &watts[120..];
        let changes = settled.windows(2).filter(|w| w[0] != w[1]).count();
        let (lo, hi) = (settled.iter().min().unwrap(), settled.iter().max().unwrap());
        assert!(changes <= 12, "{changes} changes after settling");
        assert!(hi - lo <= 3, "watts spread {lo}..={hi}");
        let mean = fps[120..].iter().sum::<f32>() / (fps.len() - 120) as f32;
        assert!(mean >= 57.0, "mean fps {mean}");
    }

    #[test]
    fn raising_sets_a_floor_that_blocks_lowering() {
        let mut ctl = Controller::new(Config::default(), 10);
        assert_eq!(ctl.update(sample(50.0)), Some(12));
        // Way over target right after: no lowering below the floor yet.
        for _ in 0..10 {
            assert_eq!(ctl.update(sample(200.0)), None);
        }
        assert_eq!(ctl.watts(), 12);
    }
}
