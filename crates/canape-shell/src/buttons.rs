//! The Ally's own buttons under the screen (Windows only). The Armoury
//! button (right) brings Canapé to the front, or back to the library when
//! it already is; the Command Center button (left) opens the Xbox Game
//! Bar, where the Canapé widget lives. Off in the settings page when
//! another tool should get them. The back paddles go to gyro aiming.

use std::thread;
use std::time::Duration;

use canape_hw::pad::Button;
use slint::Weak;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_G,
    VK_LWIN, VK_MENU,
};

use crate::AppWindow;

/// Wait before looking for the buttons again: after sleep the device comes
/// back on its own, a missing one never does.
const RETRY: Duration = Duration::from_secs(5);
const ABSENT_RETRY: Duration = Duration::from_secs(60);

pub fn spawn(ui: Weak<AppWindow>) {
    thread::spawn(move || {
        // The same failure is logged once, not at every retry.
        let mut last = String::new();
        loop {
            let result = canape_hw::listen_buttons(|button| pressed(&ui, button));
            let absent = matches!(result, Err(canape_hw::HwError::Unsupported(_)));
            if let Err(err) = result {
                let err = err.to_string();
                if err != last {
                    crate::log(&format!("buttons: {err}"));
                    last = err;
                }
            }
            thread::sleep(if absent { ABSENT_RETRY } else { RETRY });
        }
    });
}

fn pressed(ui: &Weak<AppWindow>, button: Button) {
    // The paddles drive gyro aiming, which has its own off setting.
    match button {
        Button::PaddleDown => return crate::gyro::paddle(true),
        Button::PaddleUp => return crate::gyro::paddle(false),
        _ => {}
    }
    if !crate::i18n::ShellSettings::load().ally_buttons {
        return;
    }
    match button {
        Button::Armoury => {
            let _ = ui.upgrade_in_event_loop(|ui| {
                if crate::instance::is_foreground() {
                    ui.set_page(0);
                } else {
                    crate::instance::bring_to_front();
                }
            });
        }
        Button::CommandCenter => send_keys(&[VK_LWIN, VK_G]),
        Button::Other(code) => crate::log(&format!("buttons: unknown code {code}")),
        Button::PaddleDown | Button::PaddleUp => {}
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Presses `keys` in order, then releases them in reverse order.
pub fn send_keys(keys: &[VIRTUAL_KEY]) {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&vk| key(vk, false))
        .chain(keys.iter().rev().map(|&vk| key(vk, true)))
        .collect();
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
}

/// A synthetic Alt tap: Windows lets the process that sent the last input
/// take the foreground.
pub fn tap_alt() {
    send_keys(&[VK_MENU]);
}
