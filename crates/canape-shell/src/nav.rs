//! Gamepad state → navigation keys, with auto-repeat while a direction is
//! held. Pure logic; the XInput polling lives in `gamepad.rs`.
//!
//! A → Return, B → Escape, X → F2, Y → F3, and the bumpers LB / RB → PageUp / PageDown,
//! which the window turns into page changes. The D-pad and the stick send
//! Tab / Shift+Tab (down or right / up or left), which every page
//! understands; while the library grid has the focus they send the arrow
//! keys instead, to move in two dimensions.

use std::time::{Duration, Instant};

use slint::platform::Key;

const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(120);

/// What the pad is doing at one instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub back: bool,
    /// X: the page's secondary action.
    pub x: bool,
    /// Y: settings of the selected item.
    pub y: bool,
    /// LB: previous page.
    pub page_prev: bool,
    /// RB: next page.
    pub page_next: bool,
}

/// The key a held direction sends, if any. Opposite directions cancel
/// out; with arrows, vertical wins over horizontal on a diagonal.
fn direction(pad: Pad, arrows: bool) -> Option<Key> {
    let axis = |minus: bool, plus: bool, keys: (Key, Key)| match (minus, plus) {
        (true, false) => Some(keys.0),
        (false, true) => Some(keys.1),
        _ => None,
    };
    if arrows {
        axis(pad.up, pad.down, (Key::UpArrow, Key::DownArrow))
            .or_else(|| axis(pad.left, pad.right, (Key::LeftArrow, Key::RightArrow)))
    } else {
        axis(
            pad.up || pad.left,
            pad.down || pad.right,
            (Key::Backtab, Key::Tab),
        )
    }
}

#[derive(Default)]
pub struct Navigator {
    previous: Pad,
    /// Key being repeated, since when, last sent when.
    held: Option<(Key, Instant, Instant)>,
}

impl Navigator {
    /// Forget held buttons, e.g. when the window loses the focus.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Keys to send for the pad state observed at `now`; `arrows` when the
    /// focused control moves with the arrow keys.
    pub fn update(&mut self, pad: Pad, now: Instant, arrows: bool) -> Vec<Key> {
        let mut keys = Vec::new();
        if pad.confirm && !self.previous.confirm {
            keys.push(Key::Return);
        }
        if pad.back && !self.previous.back {
            keys.push(Key::Escape);
        }
        if pad.x && !self.previous.x {
            keys.push(Key::F2);
        }
        if pad.y && !self.previous.y {
            keys.push(Key::F3);
        }
        if pad.page_prev && !self.previous.page_prev {
            keys.push(Key::PageUp);
        }
        if pad.page_next && !self.previous.page_next {
            keys.push(Key::PageDown);
        }

        let fire = match (direction(pad, arrows), self.held) {
            (None, _) => {
                self.held = None;
                None
            }
            (Some(key), Some((held, since, last))) if held == key => {
                let repeat = now - since >= REPEAT_DELAY && now - last >= REPEAT_EVERY;
                if repeat {
                    self.held = Some((key, since, now));
                }
                repeat.then_some(key)
            }
            // A new direction, or the same one now meaning another key.
            (Some(key), _) => {
                self.held = Some((key, now, now));
                Some(key)
            }
        };
        keys.extend(fire);
        self.previous = pad;
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWN: Pad = Pad {
        up: false,
        down: true,
        left: false,
        right: false,
        confirm: false,
        back: false,
        x: false,
        y: false,
        page_prev: false,
        page_next: false,
    };

    #[test]
    fn confirm_fires_once_per_press() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        let pressed = Pad {
            confirm: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(pressed, t, false), vec![Key::Return]);
        assert!(nav.update(pressed, t + REPEAT_DELAY * 2, false).is_empty());
        assert!(nav.update(Pad::default(), t, false).is_empty());
        assert_eq!(nav.update(pressed, t, true), vec![Key::Return]);
    }

    #[test]
    fn face_buttons_send_function_keys() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        let pad = Pad {
            x: true,
            y: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(pad, t, false), vec![Key::F2, Key::F3]);
        assert!(nav.update(pad, t, false).is_empty());
    }

    #[test]
    fn bumpers_turn_pages_once_per_press() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        let rb = Pad {
            page_next: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(rb, t, false), vec![Key::PageDown]);
        assert!(nav.update(rb, t + REPEAT_DELAY * 2, false).is_empty());
        let both = Pad {
            page_prev: true,
            ..rb
        };
        assert_eq!(nav.update(both, t, false), vec![Key::PageUp]);
        nav.update(Pad::default(), t, false);
        let b_and_lb = Pad {
            back: true,
            page_prev: true,
            ..Pad::default()
        };
        assert_eq!(
            nav.update(b_and_lb, t, false),
            vec![Key::Escape, Key::PageUp]
        );
    }

    #[test]
    fn held_direction_repeats_after_a_delay() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        assert_eq!(nav.update(DOWN, t, false), vec![Key::Tab]);
        assert!(
            nav.update(DOWN, t + Duration::from_millis(200), false)
                .is_empty()
        );
        assert_eq!(nav.update(DOWN, t + REPEAT_DELAY, false), vec![Key::Tab]);
        assert!(
            nav.update(DOWN, t + REPEAT_DELAY + Duration::from_millis(50), false)
                .is_empty()
        );
        assert_eq!(
            nav.update(DOWN, t + REPEAT_DELAY + REPEAT_EVERY, false),
            vec![Key::Tab]
        );
    }

    #[test]
    fn release_then_press_moves_again_at_once() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        nav.update(DOWN, t, false);
        nav.update(Pad::default(), t, false);
        let up = Pad {
            up: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(up, t, false), vec![Key::Backtab]);
    }

    #[test]
    fn reset_forgets_held_buttons() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        nav.update(DOWN, t, false);
        nav.reset();
        assert_eq!(nav.update(DOWN, t, false), vec![Key::Tab]);
    }

    #[test]
    fn tab_mode_folds_four_directions_into_two() {
        let t = Instant::now();
        let key = |pad: Pad| Navigator::default().update(pad, t, false);
        let right = Pad {
            right: true,
            ..Pad::default()
        };
        let left = Pad {
            left: true,
            ..Pad::default()
        };
        assert_eq!(key(right), vec![Key::Tab]);
        assert_eq!(key(left), vec![Key::Backtab]);
        // Down and left at once: forward and backward cancel out.
        assert!(key(Pad { left: true, ..DOWN }).is_empty());
    }

    #[test]
    fn arrow_mode_sends_each_direction() {
        let t = Instant::now();
        let key = |pad: Pad| Navigator::default().update(pad, t, true);
        let pad = |up, down, left, right| Pad {
            up,
            down,
            left,
            right,
            ..Pad::default()
        };
        assert_eq!(key(pad(true, false, false, false)), vec![Key::UpArrow]);
        assert_eq!(key(pad(false, true, false, false)), vec![Key::DownArrow]);
        assert_eq!(key(pad(false, false, true, false)), vec![Key::LeftArrow]);
        assert_eq!(key(pad(false, false, false, true)), vec![Key::RightArrow]);
        // Diagonal: vertical wins; up and down cancel, leaving horizontal.
        assert_eq!(key(pad(false, true, false, true)), vec![Key::DownArrow]);
        assert_eq!(key(pad(true, true, true, false)), vec![Key::LeftArrow]);
        assert!(key(pad(true, true, true, true)).is_empty());
    }

    #[test]
    fn arrows_repeat_and_mode_switch_is_a_new_press() {
        let mut nav = Navigator::default();
        let t = Instant::now();
        let right = Pad {
            right: true,
            ..Pad::default()
        };
        assert_eq!(nav.update(right, t, true), vec![Key::RightArrow]);
        assert_eq!(
            nav.update(right, t + REPEAT_DELAY, true),
            vec![Key::RightArrow]
        );
        // The focus left the grid while the direction is held: Tab at once.
        assert_eq!(
            nav.update(right, t + REPEAT_DELAY + Duration::from_millis(10), false),
            vec![Key::Tab]
        );
    }
}
