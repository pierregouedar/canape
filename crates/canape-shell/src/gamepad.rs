//! Gamepad navigation: the built-in controller drives the UI through key
//! events (D-pad / left stick → Tab / Shift+Tab, A → Return), only while the
//! shell has the focus. Windows only: the Ally controller is an XInput pad.

use std::thread;
use std::time::Duration;

use gilrs::{Axis, Button, EventType, Gilrs};
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, SharedString, Weak};

use crate::AppWindow;

/// Stick deflection that counts as one step.
const STICK_THRESHOLD: f32 = 0.6;

pub fn spawn(ui: Weak<AppWindow>) {
    thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(gilrs) => gilrs,
            Err(err) => {
                crate::log(&format!("gamepad unavailable: {err}"));
                return;
            }
        };
        let mut stick_engaged = false;
        loop {
            let Some(event) = gilrs.next_event_blocking(Some(Duration::from_millis(500))) else {
                continue;
            };
            let key = match event.event {
                EventType::ButtonPressed(button, _) => match button {
                    Button::DPadDown | Button::DPadRight => Some(Key::Tab),
                    Button::DPadUp | Button::DPadLeft => Some(Key::Backtab),
                    Button::South => Some(Key::Return),
                    Button::East => Some(Key::Escape),
                    _ => None,
                },
                EventType::AxisChanged(Axis::LeftStickY | Axis::LeftStickX, value, _) => {
                    if value.abs() < STICK_THRESHOLD / 2.0 {
                        stick_engaged = false;
                        None
                    } else if !stick_engaged && value.abs() >= STICK_THRESHOLD {
                        stick_engaged = true;
                        // Stick Y is positive upward.
                        let forward = match event.event {
                            EventType::AxisChanged(Axis::LeftStickY, ..) => value < 0.0,
                            _ => value > 0.0,
                        };
                        Some(if forward { Key::Tab } else { Key::Backtab })
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(key) = key.filter(|_| crate::instance::is_foreground()) {
                press(&ui, key);
            }
        }
    });
}

fn press(ui: &Weak<AppWindow>, key: Key) {
    let text = SharedString::from(key);
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let window = ui.window();
        window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        window.dispatch_event(WindowEvent::KeyReleased { text });
    });
}
