//! Gamepad state → navigation keys, with auto-repeat while a direction is
//! held. Pure logic; the XInput polling lives in `gamepad.rs`.

use std::time::{Duration, Instant};

use slint::platform::Key;

const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(120);

/// What the pad is doing at one instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    pub forward: bool,
    pub backward: bool,
    pub confirm: bool,
    pub back: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Forward,
    Backward,
}

#[derive(Default)]
pub struct Navigator {
    previous: Pad,
    held: Option<(Direction, Instant, Instant)>,
}

impl Navigator {
    /// Forget held buttons, e.g. when the window loses the focus.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Keys to send for the pad state observed at `now`.
    pub fn update(&mut self, pad: Pad, now: Instant) -> Vec<Key> {
        let mut keys = Vec::new();
        if pad.confirm && !self.previous.confirm {
            keys.push(Key::Return);
        }
        if pad.back && !self.previous.back {
            keys.push(Key::Escape);
        }

        let direction = match (pad.forward, pad.backward) {
            (true, false) => Some(Direction::Forward),
            (false, true) => Some(Direction::Backward),
            _ => None,
        };
        let fire = match (direction, self.held) {
            (None, _) => {
                self.held = None;
                None
            }
            (Some(dir), Some((held, since, last))) if held == dir => {
                let repeat = now - since >= REPEAT_DELAY && now - last >= REPEAT_EVERY;
                if repeat {
                    self.held = Some((dir, since, now));
                }
                repeat.then_some(dir)
            }
            (Some(dir), _) => {
                self.held = Some((dir, now, now));
                Some(dir)
            }
        };
        if let Some(dir) = fire {
            keys.push(match dir {
                Direction::Forward => Key::Tab,
                Direction::Backward => Key::Backtab,
            });
        }
        self.previous = pad;
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORWARD: Pad = Pad {
        forward: true,
        backward: false,
        confirm: false,
        back: false,
    };

    #[test]
    fn confirm_fires_once_per_press() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        let pressed = Pad {
            confirm: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(pressed, t), vec![Key::Return]);
        assert!(nav.update(pressed, t + REPEAT_DELAY * 2).is_empty());
        assert!(nav.update(Pad::default(), t).is_empty());
        assert_eq!(nav.update(pressed, t), vec![Key::Return]);
    }

    #[test]
    fn held_direction_repeats_after_a_delay() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        assert_eq!(nav.update(FORWARD, t), vec![Key::Tab]);
        assert!(
            nav.update(FORWARD, t + Duration::from_millis(200))
                .is_empty()
        );
        assert_eq!(nav.update(FORWARD, t + REPEAT_DELAY), vec![Key::Tab]);
        assert!(
            nav.update(FORWARD, t + REPEAT_DELAY + Duration::from_millis(50))
                .is_empty()
        );
        assert_eq!(
            nav.update(FORWARD, t + REPEAT_DELAY + REPEAT_EVERY),
            vec![Key::Tab]
        );
    }

    #[test]
    fn release_then_press_moves_again_at_once() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        nav.update(FORWARD, t);
        nav.update(Pad::default(), t);
        let backward = Pad {
            backward: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(backward, t), vec![Key::Backtab]);
    }

    #[test]
    fn reset_forgets_held_buttons() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        nav.update(FORWARD, t);
        nav.reset();
        assert_eq!(nav.update(FORWARD, t), vec![Key::Tab]);
    }
}
