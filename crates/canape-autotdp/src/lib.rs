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
            cooldown_up_ticks: 1,
            cooldown_down_ticks: 3,
            fps_smoothing: 0.4,
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
}

impl Controller {
    pub fn new(config: Config, initial_watts: u32) -> Self {
        Self {
            watts: initial_watts.clamp(config.min_watts, config.max_watts),
            config,
            smoothed_fps: None,
            ticks_since_change: 0,
        }
    }

    pub fn watts(&self) -> u32 {
        self.watts
    }

    pub fn smoothed_fps(&self) -> Option<f32> {
        self.smoothed_fps
    }

    /// Feeds one sample; returns the new wattage when it changed.
    pub fn update(&mut self, sample: Sample) -> Option<u32> {
        let c = &self.config;
        let fps = match self.smoothed_fps {
            Some(prev) => prev + c.fps_smoothing * (sample.fps - prev),
            None => sample.fps,
        };
        self.smoothed_fps = Some(fps);
        self.ticks_since_change = self.ticks_since_change.saturating_add(1);

        let low = c.target_fps * (1.0 - c.tolerance);
        let high = c.target_fps * (1.0 + c.headroom);
        // A frame-capped game sits at its target, so FPS alone never shows the
        // spare power: GPU utilization does.
        let gpu_has_slack = sample
            .gpu_util
            .is_some_and(|util| util < c.gpu_idle_threshold);

        let next = if fps < low && self.ticks_since_change >= c.cooldown_up_ticks {
            // Recover faster from a big deficit.
            let step = if fps < c.target_fps * 0.8 {
                c.step_up_watts * 2
            } else {
                c.step_up_watts
            };
            self.watts.saturating_add(step).min(c.max_watts)
        } else if (fps > high || (fps >= low && gpu_has_slack))
            && self.ticks_since_change >= c.cooldown_down_ticks
        {
            self.watts
                .saturating_sub(c.step_down_watts)
                .max(c.min_watts)
        } else {
            self.watts
        };

        if next == self.watts {
            return None;
        }
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
        assert_eq!(ctl.update(sample(90.0)), None);
        assert_eq!(ctl.update(sample(90.0)), Some(14));
    }

    #[test]
    fn frame_capped_game_with_idle_gpu_loses_power() {
        let mut ctl = Controller::new(Config::default(), 15);
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
}
