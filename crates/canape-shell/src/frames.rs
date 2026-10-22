//! Keeps the AMD driver's frame metrics running. They are started at
//! startup; a game that comes back from the background after another
//! fullscreen window covered it for a while is no longer reported until
//! they are started again. Without a frame rate, auto TDP and the bench
//! have nothing to work on.

use std::time::{Duration, Instant};

/// How long a game may run without a frame rate before the metrics are
/// started again.
const MISSING_FOR: Duration = Duration::from_secs(6);
/// At most one restart in this time, in case the game really draws nothing.
const RETRY_AFTER: Duration = Duration::from_secs(30);
#[cfg(windows)]
const POLL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Watch {
    missing_since: Option<Instant>,
    last_rearm: Option<Instant>,
}

impl Watch {
    /// Whether to start the metrics again, given whether a game is in the
    /// foreground and whether the service has a frame rate for it.
    pub fn update(&mut self, game: bool, has_fps: bool, now: Instant) -> bool {
        if !game || has_fps {
            self.missing_since = None;
            return false;
        }
        let since = *self.missing_since.get_or_insert(now);
        let waited = now.duration_since(since) >= MISSING_FOR;
        let allowed = self
            .last_rearm
            .is_none_or(|t| now.duration_since(t) >= RETRY_AFTER);
        if waited && allowed {
            self.last_rearm = Some(now);
            self.missing_since = None;
            return true;
        }
        false
    }
}

/// Starts the metrics, then watches the service's status from a thread.
#[cfg(windows)]
pub fn spawn(link: crate::link::Link) {
    std::thread::spawn(move || {
        let mut metrics = match canape_hw::FrameMetrics::arm() {
            Ok(metrics) => {
                crate::log(&metrics.describe());
                metrics
            }
            Err(err) => {
                crate::log(&format!("adl: {err}"));
                return;
            }
        };
        let mut watch = Watch::default();
        loop {
            std::thread::sleep(POLL);
            let status = link.status();
            if watch.update(
                status.foreground.is_some(),
                status.fps.is_some(),
                Instant::now(),
            ) {
                match metrics.rearm() {
                    Ok(()) => crate::log("adl: frame metrics started again (no frame rate)"),
                    Err(err) => crate::log(&format!("adl: restart: {err}")),
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_after_a_while_without_frames_then_waits() {
        let mut watch = Watch::default();
        let t = Instant::now();
        assert!(!watch.update(true, false, t));
        assert!(!watch.update(true, false, t + Duration::from_secs(4)));
        assert!(watch.update(true, false, t + MISSING_FOR));
        // Still nothing: not again before RETRY_AFTER.
        let later = t + MISSING_FOR + Duration::from_secs(10);
        assert!(!watch.update(true, false, later));
        assert!(watch.update(true, false, t + MISSING_FOR + RETRY_AFTER));
    }

    #[test]
    fn nothing_to_do_with_frames_or_without_a_game() {
        let mut watch = Watch::default();
        let t = Instant::now();
        for s in 0..60 {
            let now = t + Duration::from_secs(s);
            assert!(!watch.update(true, true, now));
            assert!(!watch.update(false, false, now));
        }
    }
}
