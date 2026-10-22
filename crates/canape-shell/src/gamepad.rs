//! Gamepad navigation (Windows): the built-in controller is an XInput pad.
//! It is polled only while the shell has the focus, so a game keeps its
//! input and the shell costs nothing in the background.

use std::thread;
use std::time::{Duration, Instant};

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, SharedString, Weak};
use windows_sys::Win32::UI::Input::XboxController::{
    XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT,
    XINPUT_GAMEPAD_DPAD_RIGHT, XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_LEFT_SHOULDER,
    XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_X, XINPUT_GAMEPAD_Y, XINPUT_STATE,
    XInputGetState,
};

use crate::AppWindow;
use crate::nav::{Navigator, Pad};

// 30 Hz is plenty for menu navigation and halves the idle cost.
const POLL: Duration = Duration::from_millis(33);
const IDLE: Duration = Duration::from_millis(250);
/// Stick deflection that counts as a direction.
const STICK_THRESHOLD: i32 = 20_000;

/// State of the first connected pad.
fn read_pad() -> Option<Pad> {
    (0..4).find_map(|user| {
        let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
        (unsafe { XInputGetState(user, &mut state) } == 0).then(|| {
            let pad = state.Gamepad;
            let pressed = |flag| pad.wButtons & flag != 0;
            let (x, y) = (i32::from(pad.sThumbLX), i32::from(pad.sThumbLY));
            // The stick counts along its main axis only: a slightly
            // diagonal push still moves one way in the library grid.
            let horizontal = x.abs() > y.abs();
            Pad {
                up: pressed(XINPUT_GAMEPAD_DPAD_UP) || (!horizontal && y > STICK_THRESHOLD),
                down: pressed(XINPUT_GAMEPAD_DPAD_DOWN) || (!horizontal && y < -STICK_THRESHOLD),
                left: pressed(XINPUT_GAMEPAD_DPAD_LEFT) || (horizontal && x < -STICK_THRESHOLD),
                right: pressed(XINPUT_GAMEPAD_DPAD_RIGHT) || (horizontal && x > STICK_THRESHOLD),
                confirm: pressed(XINPUT_GAMEPAD_A),
                back: pressed(XINPUT_GAMEPAD_B),
                x: pressed(XINPUT_GAMEPAD_X),
                y: pressed(XINPUT_GAMEPAD_Y),
                page_prev: pressed(XINPUT_GAMEPAD_LEFT_SHOULDER),
                page_next: pressed(XINPUT_GAMEPAD_RIGHT_SHOULDER),
            }
        })
    })
}

pub fn spawn(ui: Weak<AppWindow>) {
    thread::spawn(move || {
        let mut nav = Navigator::default();
        loop {
            if !crate::instance::is_foreground() {
                nav.reset();
                thread::sleep(IDLE);
                continue;
            }
            match read_pad() {
                Some(pad) => {
                    for key in nav.update(pad, Instant::now(), crate::library::arrow_navigation()) {
                        press(&ui, key);
                    }
                    thread::sleep(POLL);
                }
                None => thread::sleep(IDLE),
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
