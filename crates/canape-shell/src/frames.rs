//! Keeps the AMD driver's frame metrics running. A game that comes back
//! from the background after another fullscreen window covered it for a
//! while is no longer reported, and starting the metrics again does nothing
//! while the process that first started them is alive. So the shell never
//! starts them itself: a copy of it run with [`ARM_ARG`] starts them and
//! exits, at startup and whenever a game has no frame rate. Without a frame
//! rate, auto TDP and the bench have nothing to work on.

use std::time::{Duration, Instant};

/// Command-line argument: start the frame metrics and exit.
#[cfg(windows)]
pub const ARM_ARG: &str = "--arm-frame-metrics";

/// How long a game may run without a frame rate before the metrics are
/// started again.
const MISSING_FOR: Duration = Duration::from_secs(6);
/// Wait before trying again for the same game; doubles after each try, so
/// a game that really reports nothing (in a window) is soon left alone.
const RETRY_AFTER: Duration = Duration::from_secs(30);
const RETRY_MAX: Duration = Duration::from_secs(16 * 60);
#[cfg(windows)]
const POLL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Watch {
    game: Option<String>,
    missing_since: Option<Instant>,
    /// Last try for this game, and the wait before the next one.
    last_try: Option<(Instant, Duration)>,
}

impl Watch {
    /// Whether to start the metrics again, given the game in the foreground
    /// and whether the service has a frame rate for it.
    pub fn update(&mut self, game: Option<&str>, has_fps: bool, now: Instant) -> bool {
        if game != self.game.as_deref() || has_fps {
            // Another game, or frames again: start afresh.
            self.game = game.map(str::to_owned);
            self.missing_since = None;
            self.last_try = None;
        }
        if game.is_none() || has_fps {
            return false;
        }
        let since = *self.missing_since.get_or_insert(now);
        if now.duration_since(since) < MISSING_FOR {
            return false;
        }
        let wait = match self.last_try {
            None => RETRY_AFTER,
            Some((at, wait)) if now.duration_since(at) >= wait => (wait * 2).min(RETRY_MAX),
            Some(_) => return false,
        };
        self.last_try = Some((now, wait));
        true
    }
}

/// Starts the metrics in this process: run with [`ARM_ARG`], then exit.
#[cfg(windows)]
pub fn arm() {
    match canape_hw::FrameMetrics::arm() {
        Ok(metrics) => crate::log(&metrics.describe()),
        Err(err) => crate::log(&format!("adl: {err}")),
    }
}

/// Runs a copy of the shell that starts the metrics and exits.
#[cfg(windows)]
fn arm_from_new_process() {
    let started = std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).arg(ARM_ARG).spawn());
    match started {
        Ok(mut child) => {
            let _ = child.wait();
        }
        Err(err) => crate::log(&format!("adl: {ARM_ARG}: {err}")),
    }
}

/// Starts the metrics, then watches the service's status from a thread.
#[cfg(windows)]
pub fn spawn(link: crate::link::Link) {
    std::thread::spawn(move || {
        arm_from_new_process();
        let mut watch = Watch::default();
        loop {
            std::thread::sleep(POLL);
            let status = link.status();
            let game = status.foreground.as_deref();
            if watch.update(game, status.fps.is_some(), Instant::now()) {
                crate::log("adl: no frame rate for the game: starting the metrics again");
                arm_from_new_process();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_after_a_while_without_frames_then_waits_longer() {
        let mut watch = Watch::default();
        let game = Some("game.exe");
        let t = Instant::now();
        assert!(!watch.update(game, false, t));
        assert!(!watch.update(game, false, t + Duration::from_secs(4)));
        let first = t + MISSING_FOR;
        assert!(watch.update(game, false, first));
        assert!(!watch.update(game, false, first + Duration::from_secs(10)));
        let second = first + RETRY_AFTER;
        assert!(watch.update(game, false, second));
        // The wait doubled: not again after another RETRY_AFTER.
        assert!(!watch.update(game, false, second + RETRY_AFTER));
        assert!(watch.update(game, false, second + 2 * RETRY_AFTER));
    }

    #[test]
    fn frames_or_another_game_start_afresh() {
        let mut watch = Watch::default();
        let t = Instant::now();
        assert!(!watch.update(Some("a.exe"), false, t));
        assert!(watch.update(Some("a.exe"), false, t + MISSING_FOR));
        // Frames came back, then went again: the short wait applies.
        assert!(!watch.update(Some("a.exe"), true, t + MISSING_FOR * 2));
        let again = t + MISSING_FOR * 3;
        assert!(!watch.update(Some("a.exe"), false, again));
        assert!(watch.update(Some("a.exe"), false, again + MISSING_FOR));
        // Another game: its own count.
        let other = again + MISSING_FOR + Duration::from_secs(1);
        assert!(!watch.update(Some("b.exe"), false, other));
        assert!(watch.update(Some("b.exe"), false, other + MISSING_FOR));
    }

    #[test]
    fn nothing_to_do_with_frames_or_without_a_game() {
        let mut watch = Watch::default();
        let t = Instant::now();
        for s in 0..60 {
            let now = t + Duration::from_secs(s);
            assert!(!watch.update(Some("a.exe"), true, now));
            assert!(!watch.update(None, false, now));
        }
    }
}
