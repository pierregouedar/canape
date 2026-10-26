//! "Full power in Canapé": the shell draws on the CPU, so while it is the
//! window in front (and nothing is being played) the performance mode's
//! power limits are written, then the user's own state is put back.
//!
//! The decision is pure; the handler owns the clock and the writes.

use std::time::Duration;

use canape_core::PerformanceMode;

/// How long the shell must be out of the foreground before the user's state
/// is put back: a short trip to another window does not flap the firmware.
pub const RELEASE_AFTER: Duration = Duration::from_millis(1500);

/// A manual (or auto) power limit under this many watts is below the
/// performance mode's. It is only compared with, never written: the mode
/// applies the model's own limits.
pub const UI_LEVEL_WATTS: u32 = 15;

/// What the decision looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    /// The setting is on.
    pub enabled: bool,
    /// The shell reported being in front, recently enough.
    pub shell_front: bool,
    /// A game is running (in front, or behind the shell).
    pub game_running: bool,
    /// A bench is running or waiting for its game.
    pub bench: bool,
    /// The user's mode and power limit (what the firmware would hold
    /// without the interface level).
    pub mode: Option<PerformanceMode>,
    pub watts: Option<u32>,
    /// The last attempt to write the level failed: not tried again until
    /// the shell has left the foreground.
    pub failed: bool,
}

/// What to do about the firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Write the interface level.
    Boost,
    /// Put the user's state back.
    Release,
    /// Nothing to write.
    Keep,
}

/// True when the user's state is lower than the performance mode's, and
/// known well enough to be put back exactly. A mode that is not known (the
/// firmware cannot report it, and nothing was set yet) is left alone.
pub fn below_ui_level(mode: Option<PerformanceMode>, watts: Option<u32>) -> bool {
    match mode {
        Some(PerformanceMode::Silent) => true,
        Some(PerformanceMode::Manual) => watts.is_some_and(|w| w < UI_LEVEL_WATTS),
        Some(PerformanceMode::Performance | PerformanceMode::Turbo) | None => false,
    }
}

/// `active`: the interface level is written now. `out_for`: how long the
/// shell has been out of the foreground while it was active.
pub fn decide(input: Input, active: bool, out_for: Option<Duration>) -> Step {
    let blocked = !input.enabled
        || input.game_running
        || input.bench
        || !below_ui_level(input.mode, input.watts);
    if blocked {
        return if active { Step::Release } else { Step::Keep };
    }
    if input.shell_front && !input.failed {
        return if active { Step::Keep } else { Step::Boost };
    }
    match out_for {
        Some(out) if active && out >= RELEASE_AFTER => Step::Release,
        _ => Step::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Input {
        Input {
            enabled: true,
            shell_front: true,
            game_running: false,
            bench: false,
            mode: Some(PerformanceMode::Silent),
            watts: None,
            failed: false,
        }
    }

    #[test]
    fn boosts_a_low_level_while_the_shell_is_in_front() {
        assert_eq!(decide(input(), false, None), Step::Boost);
        assert_eq!(decide(input(), true, None), Step::Keep);
        let manual = |watts| Input {
            mode: Some(PerformanceMode::Manual),
            watts,
            ..input()
        };
        assert_eq!(decide(manual(Some(9)), false, None), Step::Boost);
        assert_eq!(
            decide(manual(Some(UI_LEVEL_WATTS)), false, None),
            Step::Keep
        );
        assert_eq!(decide(manual(Some(25)), false, None), Step::Keep);
        assert_eq!(decide(manual(None), false, None), Step::Keep);
    }

    #[test]
    fn leaves_levels_at_or_above_the_ui_level_and_unknown_modes() {
        for mode in [
            Some(PerformanceMode::Performance),
            Some(PerformanceMode::Turbo),
            None,
        ] {
            let input = Input { mode, ..input() };
            assert_eq!(decide(input, false, None), Step::Keep);
        }
    }

    #[test]
    fn a_game_a_bench_or_the_setting_off_release_at_once() {
        for blocked in [
            Input {
                game_running: true,
                ..input()
            },
            Input {
                bench: true,
                ..input()
            },
            Input {
                enabled: false,
                ..input()
            },
        ] {
            assert_eq!(decide(blocked, false, None), Step::Keep);
            assert_eq!(decide(blocked, true, None), Step::Release);
        }
    }

    #[test]
    fn leaving_the_foreground_releases_after_the_hold() {
        let away = Input {
            shell_front: false,
            ..input()
        };
        assert_eq!(decide(away, true, Some(Duration::ZERO)), Step::Keep);
        assert_eq!(decide(away, true, Some(RELEASE_AFTER)), Step::Release);
        assert_eq!(decide(away, false, Some(RELEASE_AFTER)), Step::Keep);
        // Back in front before the hold ends: nothing happens.
        assert_eq!(
            decide(input(), true, Some(Duration::from_secs(1))),
            Step::Keep
        );
    }

    #[test]
    fn a_failed_write_is_not_retried_while_the_shell_stays() {
        let failed = Input {
            failed: true,
            ..input()
        };
        assert_eq!(decide(failed, false, None), Step::Keep);
    }
}
