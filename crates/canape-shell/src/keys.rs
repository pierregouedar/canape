//! Keyboard input, as the window receives it from the system. Programs
//! that turn the gamepad into keys send Enter for A and Escape for B: the
//! same rules as for the pad apply, so a press meant for a game, or one made
//! while the shell is still opening, does nothing.

use std::collections::HashSet;
use std::hash::Hash;
use std::time::{Duration, Instant};

use slint::winit_030::winit::event::{ElementState, WindowEvent};
use slint::winit_030::winit::keyboard::{Key, NamedKey};
use slint::winit_030::{EventResult, WinitWindowAccessor};

/// Keys ignored when the shell first gets the focus, e.g. at boot while
/// buttons are being pressed to get through the sign-in. On the Ally the
/// window shows about 1.5 s after it first gets the focus: a press made as
/// it appears must still be ignored.
pub const FIRST_QUIET: Duration = Duration::from_secs(2);

/// Which key presses reach the window.
pub struct KeyGate<K> {
    focused_once: bool,
    quiet_until: Option<Instant>,
    /// Keys pressed while the window had the focus.
    down: HashSet<K>,
    /// Keys swallowed, with their repeats, until released.
    swallowed: HashSet<K>,
}

impl<K> Default for KeyGate<K> {
    fn default() -> Self {
        Self {
            focused_once: false,
            quiet_until: None,
            down: HashSet::new(),
            swallowed: HashSet::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> KeyGate<K> {
    pub fn focus(&mut self, focused: bool, now: Instant) {
        self.down.clear();
        if focused && !self.focused_once {
            self.focused_once = true;
            self.quiet_until = Some(now + FIRST_QUIET);
        }
    }

    /// Time since the window first got the focus, for the log.
    pub fn since_first_focus(&self, now: Instant) -> Option<Duration> {
        self.quiet_until.map(|until| now + FIRST_QUIET - until)
    }

    /// Whether this key event reaches the window. A repeat of a key not
    /// seen going down was held before the window got the focus.
    pub fn pass(&mut self, key: K, pressed: bool, repeat: bool, now: Instant) -> bool {
        if !pressed {
            self.down.remove(&key);
            return !self.swallowed.remove(&key);
        }
        let quiet = self.quiet_until.is_some_and(|until| now <= until);
        let held_before = repeat && !self.down.contains(&key);
        if self.swallowed.contains(&key) || quiet || held_before {
            self.swallowed.insert(key);
            return false;
        }
        self.down.insert(key);
        true
    }
}

/// Filters the window's keyboard input through a [`KeyGate`], and logs
/// Enter and Escape with where they come from.
pub fn install(window: &slint::Window) {
    let mut gate = KeyGate::default();
    window.on_winit_window_event(move |_, event| {
        let now = Instant::now();
        match event {
            WindowEvent::Focused(focused) => gate.focus(*focused, now),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                let pressed = event.state == ElementState::Pressed;
                let pass = gate.pass(event.physical_key, pressed, event.repeat, now);
                if pressed
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Enter | NamedKey::Escape)
                    )
                {
                    crate::log(&format!(
                        "keys: keyboard {:?} {} ms after the first focus{}{}",
                        event.logical_key,
                        gate.since_first_focus(now).map_or(0, |d| d.as_millis()),
                        if event.repeat { " (repeat)" } else { "" },
                        if pass { "" } else { ", ignored" },
                    ));
                }
                if !pass {
                    return EventResult::PreventDefault;
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_at_first_focus_until_released() {
        let mut gate = KeyGate::default();
        let t = Instant::now();
        gate.focus(true, t);
        assert!(!gate.pass('a', true, false, t));
        let later = t + FIRST_QUIET * 2;
        // Still held after the quiet time: its repeats stay swallowed.
        assert!(!gate.pass('a', true, true, later));
        assert!(!gate.pass('a', false, false, later));
        assert!(gate.pass('a', true, false, later));
        assert!(gate.pass('a', false, false, later));
    }

    #[test]
    fn key_held_from_before_the_focus_does_nothing() {
        let mut gate = KeyGate::default();
        let t = Instant::now();
        gate.focus(true, t);
        let later = t + FIRST_QUIET * 2;
        gate.focus(false, later);
        gate.focus(true, later);
        // Only repeats arrive for a key held while another window had the focus.
        assert!(!gate.pass('a', true, true, later));
        assert!(!gate.pass('a', false, false, later));
        assert!(gate.pass('a', true, false, later));
        // A key pressed here repeats as usual.
        assert!(gate.pass('a', true, true, later));
    }

    #[test]
    fn later_focus_is_not_quiet() {
        let mut gate = KeyGate::default();
        let t = Instant::now();
        gate.focus(true, t);
        let later = t + FIRST_QUIET * 2;
        gate.focus(false, later);
        gate.focus(true, later);
        assert!(gate.pass('a', true, false, later));
    }
}
