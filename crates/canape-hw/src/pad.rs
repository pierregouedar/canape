//! The Ally's built-in controller: HID facts and feature reports. The I/O
//! lives in `hid.rs`; this module is plain data, tested on any OS.

use canape_core::{
    BackButton, BackButtons, ButtonTarget, ControllerMode, LightingEffect, LightingSpeed,
};

/// ASUS.
pub const VENDOR_ID: u16 = 0x0B05;
/// ROG Ally and Ally X. The RC73 Xbox Ally PIDs are not known yet: no
/// controller there for now.
pub const PRODUCT_IDS: [u16; 2] = [0x1ABE, 0x1B4C];
/// Report ID of the input-configuration feature report. The right HID
/// interface is the one whose descriptor declares a feature report with
/// this ID.
pub const REPORT_ID: u8 = 0x5A;
/// Default feature report length. The real length comes from the
/// interface (`HidP_GetCaps`).
pub const FEATURE_REPORT_LEN: usize = 64;

/// Wakes the controller's configuration interface: the report ID followed
/// by `ASUS Tech.Inc.` in ASCII, sent once before the first mode report.
pub fn init_report(len: usize) -> Vec<u8> {
    let mut report = vec![REPORT_ID];
    report.extend_from_slice(b"ASUS Tech.Inc.");
    pad(report, len)
}

/// `[0x5A, 0xD1, 0x01, 0x01, mode]`.
pub fn mode_report(mode: ControllerMode, len: usize) -> Vec<u8> {
    pad(vec![REPORT_ID, SETTINGS, 0x01, 0x01, mode_value(mode)], len)
}

/// Firmware value of each controller mode: 1 gamepad, 2 WASD, 3 mouse.
pub fn mode_value(mode: ControllerMode) -> u8 {
    match mode {
        ControllerMode::Gamepad => 1,
        ControllerMode::Wasd => 2,
        ControllerMode::Mouse => 3,
    }
}

/// Command group of the controller settings (`0xD1`), its button mapping
/// command (`0x02`) and its turbo command (`0x0F`, 32 bytes of payload).
const SETTINGS: u8 = 0xD1;
const SET_MAPPING: u8 = 0x02;
const SET_TURBO: u8 = 0x0F;
const TURBO_LEN: u8 = 0x20;
/// The mapping command takes the buttons two by two: nine pairs, numbered
/// from 1, in this order. Each pair lists its first button then its
/// second one.
const PAIRS: u8 = 9;
/// D-pad up / down, D-pad left / right, stick clicks, bumpers, A / B,
/// X / Y, View / Menu, the back buttons (M2, the left one, first; M1, the
/// right one, second), triggers.
const PAIR_DPAD_UP_DOWN: u8 = 1;
const PAIR_DPAD_LEFT_RIGHT: u8 = 2;
const PAIR_STICK_CLICKS: u8 = 3;
const PAIR_BUMPERS: u8 = 4;
const PAIR_AB: u8 = 5;
const PAIR_XY: u8 = 6;
const PAIR_VIEW_MENU: u8 = 7;
const PAIR_BACK_BUTTONS: u8 = 8;
const PAIR_TRIGGERS: u8 = 9;
/// A pair's payload: four 11-byte bindings, the first button then its
/// secondary binding (with the left button under the screen held), the
/// second button then its secondary binding.
const BINDING_LEN: usize = 11;
const PAIR_LEN: u8 = 4 * BINDING_LEN as u8;

type Binding = [u8; BINDING_LEN];

/// Binding groups: the first byte of a binding says where its code goes.
const GROUP_PAD: u8 = 0x01;
const GROUP_KEY: u8 = 0x02;
const GROUP_MOUSE: u8 = 0x03;
const GROUP_COMBO: u8 = 0x04;
const GROUP_MEDIA: u8 = 0x05;

/// Nothing: the button sends nothing.
const NONE: Binding = [0; BINDING_LEN];

/// Gamepad codes go in byte 1, keyboard codes in byte 2, media codes in
/// byte 3, mouse codes in byte 4.
const fn code(group: u8, code: u8) -> Binding {
    let mut bytes = NONE;
    bytes[0] = group;
    let at = match group {
        GROUP_PAD => 1,
        GROUP_KEY => 2,
        GROUP_MEDIA => 3,
        _ => 4,
    };
    bytes[at] = code;
    bytes
}

const fn pad_button(c: u8) -> Binding {
    code(GROUP_PAD, c)
}

const fn key(c: u8) -> Binding {
    code(GROUP_KEY, c)
}

const fn mouse(c: u8) -> Binding {
    code(GROUP_MOUSE, c)
}

const fn media(c: u8) -> Binding {
    code(GROUP_MEDIA, c)
}

/// Keys pressed together: their count in byte 5, then their codes.
const fn combo(keys: &[u8]) -> Binding {
    let mut bytes = NONE;
    bytes[0] = GROUP_COMBO;
    bytes[5] = keys.len() as u8;
    let mut i = 0;
    while i < keys.len() {
        bytes[6 + i] = keys[i];
        i += 1;
    }
    bytes
}

/// Key codes used by the factory bindings.
const KEY_M1: u8 = 0x8F;
const KEY_M2: u8 = 0x8E;
const KEY_CTRL: u8 = 0x8C;
const KEY_SHIFT: u8 = 0x88;
const KEY_WINDOWS: u8 = 0x82;
const KEY_ESCAPE: u8 = 0x76;
const KEY_TAB: u8 = 0x0D;

/// The 11 bytes that make a button send `target`; `button` gives what
/// [`ButtonTarget::Default`] means.
pub fn binding(button: BackButton, target: ButtonTarget) -> Binding {
    use ButtonTarget as T;
    match target {
        T::Default => key(match button {
            BackButton::M1 => KEY_M1,
            BackButton::M2 => KEY_M2,
        }),
        T::Disabled => NONE,
        T::PadA => pad_button(0x01),
        T::PadB => pad_button(0x02),
        T::PadX => pad_button(0x03),
        T::PadY => pad_button(0x04),
        T::PadLb => pad_button(0x05),
        T::PadRb => pad_button(0x06),
        T::PadLs => pad_button(0x07),
        T::PadRs => pad_button(0x08),
        T::DpadUp => pad_button(0x09),
        T::DpadDown => pad_button(0x0A),
        T::DpadLeft => pad_button(0x0B),
        T::DpadRight => pad_button(0x0C),
        T::PadLt => pad_button(0x0D),
        T::PadRt => pad_button(0x0E),
        T::PadView => pad_button(0x11),
        T::PadMenu => pad_button(0x12),
        T::PadXbox => pad_button(0x13),
        T::KeyEscape => key(KEY_ESCAPE),
        T::KeyEnter => key(0x5A),
        T::KeySpace => key(0x29),
        T::KeyTab => key(KEY_TAB),
        T::KeyBackspace => key(0x66),
        T::KeyUp => key(0x98),
        T::KeyDown => key(0x99),
        T::KeyLeft => key(0x9A),
        T::KeyRight => key(0x9B),
        T::KeyPageUp => key(0x96),
        T::KeyPageDown => key(0x97),
        T::KeyShift => key(KEY_SHIFT),
        T::KeyCtrl => key(KEY_CTRL),
        T::KeyWindows => key(KEY_WINDOWS),
        T::KeyF11 => key(0x78),
        T::KeyF12 => key(0x07),
        T::MouseLeft => mouse(0x01),
        T::MouseRight => mouse(0x02),
        T::MouseMiddle => mouse(0x03),
        T::WheelUp => mouse(0x04),
        T::WheelDown => mouse(0x05),
        T::MuteMic => media(0x01),
        T::VolumeDown => media(0x02),
        T::VolumeUp => media(0x03),
        T::ShowKeyboard => media(0x19),
    }
}

/// The factory bindings of `pair` (other than the back buttons) in
/// `mode`, `None` when they are not known for that mode. The secondary
/// bindings, kept the same in every mode: D-pad up the on-screen
/// keyboard (media `0x19`), down Ctrl Shift Esc, left Win D, right
/// Win Tab, A a screenshot (media `0x16`), B Win N, X Win P, Y recording
/// (media `0x1E`).
fn factory_pair(pair: u8, mode: ControllerMode) -> Option<[Binding; 4]> {
    let mouse_mode = match mode {
        ControllerMode::Gamepad => false,
        ControllerMode::Mouse => true,
        ControllerMode::Wasd => return None,
    };
    let (first, second) = match (pair, mouse_mode) {
        (PAIR_DPAD_UP_DOWN, false) => (pad_button(0x09), pad_button(0x0A)),
        (PAIR_DPAD_UP_DOWN, true) => (key(0x98), key(0x99)),
        (PAIR_DPAD_LEFT_RIGHT, false) => (pad_button(0x0B), pad_button(0x0C)),
        (PAIR_DPAD_LEFT_RIGHT, true) => (key(0x9A), key(0x9B)),
        (PAIR_STICK_CLICKS, false) => (pad_button(0x07), pad_button(0x08)),
        (PAIR_STICK_CLICKS, true) => (key(KEY_SHIFT), mouse(0x01)),
        (PAIR_BUMPERS, false) => (pad_button(0x05), pad_button(0x06)),
        (PAIR_BUMPERS, true) => (key(KEY_TAB), mouse(0x01)),
        (PAIR_AB, false) => (pad_button(0x01), pad_button(0x02)),
        (PAIR_AB, true) => (key(0x5A), key(KEY_ESCAPE)),
        (PAIR_XY, false) => (pad_button(0x03), pad_button(0x04)),
        (PAIR_XY, true) => (key(0x97), key(0x96)),
        (PAIR_VIEW_MENU, _) => (pad_button(0x11), pad_button(0x12)),
        (PAIR_TRIGGERS, false) => (pad_button(0x0D), pad_button(0x0E)),
        (PAIR_TRIGGERS, true) => (combo(&[KEY_SHIFT, KEY_TAB]), mouse(0x02)),
        _ => return None,
    };
    let (first_held, second_held) = match pair {
        PAIR_DPAD_UP_DOWN => (media(0x19), combo(&[KEY_CTRL, KEY_SHIFT, KEY_ESCAPE])),
        PAIR_DPAD_LEFT_RIGHT => (combo(&[KEY_WINDOWS, 0x23]), combo(&[KEY_WINDOWS, KEY_TAB])),
        PAIR_AB => (media(0x16), combo(&[KEY_WINDOWS, 0x31])),
        PAIR_XY => (combo(&[KEY_WINDOWS, 0x4D]), media(0x1E)),
        _ => (NONE, NONE),
    };
    Some([first, first_held, second, second_held])
}

/// `[0x5A, 0xD1, 0x02, pair, 0x2C]` then the pair's four bindings.
fn mapping_report(pair: u8, bindings: &[Binding; 4], len: usize) -> Vec<u8> {
    let mut report = vec![REPORT_ID, SETTINGS, SET_MAPPING, pair, PAIR_LEN];
    for b in bindings {
        report.extend_from_slice(b);
    }
    pad(report, len)
}

/// The back button pair: M2, M2 held with the left screen button, M1,
/// M1 held. The secondary bindings keep the buttons' own keys.
fn back_buttons_pair(buttons: &BackButtons) -> [Binding; 4] {
    let m2 = BackButton::M2;
    let m1 = BackButton::M1;
    [
        binding(m2, buttons.m2),
        binding(m2, ButtonTarget::Default),
        binding(m1, buttons.m1),
        binding(m1, ButtonTarget::Default),
    ]
}

/// Every report that sets the back buttons, in order: the mode, the nine
/// pairs (the factory bindings of `mode` for all but the back buttons),
/// then the turbo settings, all off: the controller is given its whole
/// mapping table each time, never a pair on its own. `None` in WASD mode,
/// whose factory bindings are not known.
pub fn back_buttons_reports(
    buttons: &BackButtons,
    mode: ControllerMode,
    len: usize,
) -> Option<Vec<Vec<u8>>> {
    let mut reports = vec![mode_report(mode, len)];
    for pair in 1..=PAIRS {
        let bindings = if pair == PAIR_BACK_BUTTONS {
            back_buttons_pair(buttons)
        } else {
            factory_pair(pair, mode)?
        };
        reports.push(mapping_report(pair, &bindings, len));
    }
    reports.push(turbo_off_report(len));
    Some(reports)
}

/// `[0x5A, 0xD1, 0x0F, 0x20]` and zeros: no button repeats on its own.
pub fn turbo_off_report(len: usize) -> Vec<u8> {
    pad(vec![REPORT_ID, SETTINGS, SET_TURBO, TURBO_LEN], len)
}

/// Brightness of the stick rings, 0 (off) to 3:
/// `[0x5A, 0xBA, 0xC5, 0xC4, level]`.
pub fn brightness_report(level: u8, len: usize) -> Vec<u8> {
    pad(vec![REPORT_ID, 0xBA, 0xC5, 0xC4, level], len)
}

/// Colour of the stick rings, set directly: `[0x5A, 0xD1, 0x08, 0x0C]`
/// then red, green and blue for each of the four zones (two per ring),
/// the same colour in all four.
pub fn ring_color_report(rgb: [u8; 3], len: usize) -> Vec<u8> {
    let mut report = vec![REPORT_ID, SETTINGS, 0x08, 0x0C];
    for _ in 0..4 {
        report.extend_from_slice(&rgb);
    }
    pad(report, len)
}

/// The controller's own lighting effect for the stick rings, all four
/// zones: `[0x5A, 0xB3, 0x00, effect, r, g, b, speed, 0x00, breathing,
/// r2, g2, b2]`. Effects: `0x00` steady, `0x01` breathing, `0x02` colour
/// cycle, `0x03` rainbow, `0x0A` strobe. Speed: `0xE1` slow, `0xEB`
/// medium, `0xF5` fast, `0x00` when steady. Breathing sets the breathing
/// byte and fades to black (second colour zero); the rainbow ignores the
/// colour, sent as zeros. The controller keeps the effect after
/// [`effect_set_report`] and [`effect_apply_report`].
pub fn effect_report(
    effect: LightingEffect,
    rgb: [u8; 3],
    speed: LightingSpeed,
    len: usize,
) -> Vec<u8> {
    let mode = match effect {
        LightingEffect::Steady => 0x00,
        LightingEffect::Breathing => 0x01,
        LightingEffect::ColorCycle => 0x02,
        LightingEffect::Rainbow => 0x03,
        LightingEffect::Strobe => 0x0A,
    };
    let speed = match (effect.moves(), speed) {
        (false, _) => 0x00,
        (true, LightingSpeed::Slow) => 0xE1,
        (true, LightingSpeed::Medium) => 0xEB,
        (true, LightingSpeed::Fast) => 0xF5,
    };
    let rgb = if effect == LightingEffect::Rainbow {
        [0; 3]
    } else {
        rgb
    };
    let breathing = u8::from(effect == LightingEffect::Breathing);
    let mut report = vec![REPORT_ID, EFFECT, 0x00, mode];
    report.extend_from_slice(&rgb);
    report.extend_from_slice(&[speed, 0x00, breathing, 0, 0, 0]);
    pad(report, len)
}

/// Makes the controller keep the effect just written: `[0x5A, 0xB5]`.
pub fn effect_set_report(len: usize) -> Vec<u8> {
    pad(vec![REPORT_ID, EFFECT_SET], len)
}

/// Puts the effect kept into use: `[0x5A, 0xB4]`.
pub fn effect_apply_report(len: usize) -> Vec<u8> {
    pad(vec![REPORT_ID, EFFECT_APPLY], len)
}

/// Lighting commands: the effect, keeping it, putting it in use.
const EFFECT: u8 = 0xB3;
const EFFECT_APPLY: u8 = 0xB4;
const EFFECT_SET: u8 = 0xB5;

/// A press on one of the Ally's own buttons, read from the input reports
/// of the same interface (report ID [`REPORT_ID`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The left button under the screen, below View.
    CommandCenter,
    /// The right button under the screen, below Menu (the ROG logo).
    Armoury,
    /// Either back paddle went down (the two are not told apart).
    PaddleDown,
    /// The back paddles were released.
    PaddleUp,
    /// Any other key code, for the log.
    Other(u8),
}

/// The button an input report stands for, if any. Code 236 is sent
/// alongside other keys and means nothing on its own.
pub fn parse_input(report: &[u8]) -> Option<Button> {
    match report {
        [REPORT_ID, code, ..] => match code {
            166 => Some(Button::CommandCenter),
            56 | 147 => Some(Button::Armoury),
            165 => Some(Button::PaddleDown),
            0 => Some(Button::PaddleUp),
            236 => None,
            &other => Some(Button::Other(other)),
        },
        _ => None,
    }
}

/// Zero padding up to the feature report length, report ID included.
fn pad(mut report: Vec<u8>, len: usize) -> Vec<u8> {
    if report.len() < len {
        report.resize(len, 0);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_report_spells_asus() {
        let report = init_report(FEATURE_REPORT_LEN);
        assert_eq!(report.len(), 64);
        assert_eq!(report[0], 0x5A);
        assert_eq!(&report[1..15], b"ASUS Tech.Inc.");
        assert!(report[15..].iter().all(|&b| b == 0));
    }

    #[test]
    fn button_codes() {
        assert_eq!(parse_input(&[0x5A, 166, 0, 0]), Some(Button::CommandCenter));
        assert_eq!(parse_input(&[0x5A, 56]), Some(Button::Armoury));
        assert_eq!(parse_input(&[0x5A, 147]), Some(Button::Armoury));
        assert_eq!(parse_input(&[0x5A, 165]), Some(Button::PaddleDown));
        assert_eq!(parse_input(&[0x5A, 0]), Some(Button::PaddleUp));
        assert_eq!(parse_input(&[0x5A, 162]), Some(Button::Other(162)));
        assert_eq!(parse_input(&[0x5A, 236]), None);
        assert_eq!(parse_input(&[0x5D, 166]), None);
        assert_eq!(parse_input(&[0x5A]), None);
    }

    /// The pair 8 report of a sequence, checked for its place and header.
    fn back_pair(reports: &[Vec<u8>]) -> &[u8] {
        let report = &reports[8];
        assert_eq!(&report[..5], &[0x5A, 0xD1, 0x02, 0x08, 0x2C]);
        report
    }

    #[test]
    fn factory_back_buttons() {
        let reports =
            back_buttons_reports(&BackButtons::default(), ControllerMode::Gamepad, 64).unwrap();
        let report = back_pair(&reports);
        assert_eq!(report.len(), 64);
        let mut expected = [0u8; 44];
        for (i, key) in [0x8E, 0x8E, 0x8F, 0x8F].into_iter().enumerate() {
            expected[i * 11] = 0x02;
            expected[i * 11 + 2] = key;
        }
        assert_eq!(&report[5..49], &expected);
        assert!(report[49..].iter().all(|&b| b == 0));
    }

    #[test]
    fn remapped_back_buttons() {
        let buttons = BackButtons {
            m1: ButtonTarget::PadA,
            m2: ButtonTarget::Disabled,
        };
        let reports = back_buttons_reports(&buttons, ControllerMode::Gamepad, 64).unwrap();
        let report = back_pair(&reports);
        // M2: nothing, M2 held: its key, M1: A, M1 held: its key.
        assert!(report[5..16].iter().all(|&b| b == 0));
        assert_eq!(&report[16..19], &[0x02, 0x00, 0x8E]);
        assert_eq!(&report[27..29], &[0x01, 0x01]);
        assert!(report[29..38].iter().all(|&b| b == 0));
        assert_eq!(&report[38..41], &[0x02, 0x00, 0x8F]);
    }

    #[test]
    fn back_buttons_sequence() {
        let reports =
            back_buttons_reports(&BackButtons::default(), ControllerMode::Gamepad, 64).unwrap();
        // The mode, the nine pairs in order, then turbo off.
        assert_eq!(reports.len(), 11);
        assert!(reports.iter().all(|r| r.len() == 64));
        assert_eq!(reports[0], mode_report(ControllerMode::Gamepad, 64));
        for (i, report) in reports[1..10].iter().enumerate() {
            assert_eq!(&report[..5], &[0x5A, 0xD1, 0x02, i as u8 + 1, 0x2C]);
            assert!(report[49..].iter().all(|&b| b == 0));
        }
        assert_eq!(&reports[10][..4], &[0x5A, 0xD1, 0x0F, 0x20]);
        assert!(reports[10][4..].iter().all(|&b| b == 0));
        // D-pad up / down: up, the on-screen keyboard, down, Ctrl Shift Esc.
        let dpad = &reports[1][5..49];
        assert_eq!(&dpad[..2], &[0x01, 0x09]);
        assert_eq!(&dpad[11..15], &[0x05, 0x00, 0x00, 0x19]);
        assert_eq!(&dpad[22..24], &[0x01, 0x0A]);
        assert_eq!(&dpad[33..42], &[0x04, 0, 0, 0, 0, 0x03, 0x8C, 0x88, 0x76]);
        // A / B: A, a screenshot, B, Win N.
        let ab = &reports[5][5..49];
        assert_eq!(&ab[..2], &[0x01, 0x01]);
        assert_eq!(&ab[11..15], &[0x05, 0x00, 0x00, 0x16]);
        assert_eq!(&ab[22..24], &[0x01, 0x02]);
        assert_eq!(&ab[33..41], &[0x04, 0, 0, 0, 0, 0x02, 0x82, 0x31]);
        // Triggers: LT and RT, nothing held.
        let triggers = &reports[9][5..49];
        assert_eq!(&triggers[..2], &[0x01, 0x0D]);
        assert_eq!(&triggers[22..24], &[0x01, 0x0E]);
        assert!(triggers[11..22].iter().all(|&b| b == 0));
    }

    #[test]
    fn back_buttons_in_other_modes() {
        let buttons = BackButtons::default();
        let mouse = back_buttons_reports(&buttons, ControllerMode::Mouse, 64).unwrap();
        assert_eq!(mouse[0][4], 3);
        // A / B send Enter and Esc, the triggers Shift Tab and a right click.
        assert_eq!(&mouse[5][5..8], &[0x02, 0x00, 0x5A]);
        assert_eq!(&mouse[5][27..30], &[0x02, 0x00, 0x76]);
        assert_eq!(&mouse[9][5..13], &[0x04, 0, 0, 0, 0, 0x02, 0x88, 0x0D]);
        assert_eq!(&mouse[9][27..32], &[0x03, 0, 0, 0, 0x02]);
        assert_eq!(
            back_pair(&mouse),
            back_pair(&back_buttons_reports(&buttons, ControllerMode::Gamepad, 64).unwrap())
        );
        assert_eq!(
            back_buttons_reports(&buttons, ControllerMode::Wasd, 64),
            None
        );
    }

    #[test]
    fn binding_groups() {
        let m1 = BackButton::M1;
        assert_eq!(binding(m1, ButtonTarget::PadRt)[..2], [0x01, 0x0E]);
        assert_eq!(binding(m1, ButtonTarget::KeyF12)[..3], [0x02, 0x00, 0x07]);
        assert_eq!(binding(m1, ButtonTarget::VolumeUp)[..4], [0x05, 0, 0, 0x03]);
        assert_eq!(
            binding(m1, ButtonTarget::MouseLeft)[..5],
            [0x03, 0, 0, 0, 0x01]
        );
        // One non-zero code per binding, and only `Disabled` is all zero.
        for target in ButtonTarget::ALL {
            let bytes = binding(m1, target);
            let codes = bytes[1..].iter().filter(|&&b| b != 0).count();
            assert_eq!(
                codes,
                usize::from(target != ButtonTarget::Disabled),
                "{target:?}"
            );
        }
    }

    #[test]
    fn lighting_reports() {
        let bright = brightness_report(2, FEATURE_REPORT_LEN);
        assert_eq!(bright.len(), 64);
        assert_eq!(&bright[..5], &[0x5A, 0xBA, 0xC5, 0xC4, 0x02]);
        assert!(bright[5..].iter().all(|&b| b == 0));
        let color = ring_color_report([0xFF, 0x3D, 0x5E], FEATURE_REPORT_LEN);
        assert_eq!(color.len(), 64);
        assert_eq!(&color[..4], &[0x5A, 0xD1, 0x08, 0x0C]);
        assert_eq!(color[4..16], [0xFF, 0x3D, 0x5E].repeat(4)[..]);
        assert!(color[16..].iter().all(|&b| b == 0));
    }

    #[test]
    fn effect_reports() {
        let red = [0xFF, 0x00, 0x00];
        let breathing = effect_report(LightingEffect::Breathing, red, LightingSpeed::Slow, 64);
        assert_eq!(breathing.len(), 64);
        assert_eq!(
            &breathing[..13],
            &[
                0x5A, 0xB3, 0x00, 0x01, 0xFF, 0x00, 0x00, 0xE1, 0x00, 0x01, 0, 0, 0
            ]
        );
        assert!(breathing[13..].iter().all(|&b| b == 0));
        let steady = effect_report(LightingEffect::Steady, red, LightingSpeed::Fast, 64);
        assert_eq!(
            &steady[..10],
            &[0x5A, 0xB3, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
        let cycle = effect_report(LightingEffect::ColorCycle, red, LightingSpeed::Medium, 64);
        assert_eq!(&cycle[3..8], &[0x02, 0xFF, 0x00, 0x00, 0xEB]);
        let rainbow = effect_report(LightingEffect::Rainbow, red, LightingSpeed::Fast, 64);
        assert_eq!(&rainbow[3..8], &[0x03, 0x00, 0x00, 0x00, 0xF5]);
        let strobe = effect_report(LightingEffect::Strobe, red, LightingSpeed::Fast, 64);
        assert_eq!(&strobe[3..10], &[0x0A, 0xFF, 0x00, 0x00, 0xF5, 0x00, 0x00]);
        assert_eq!(&effect_set_report(64)[..3], &[0x5A, 0xB5, 0x00]);
        assert_eq!(&effect_apply_report(64)[..3], &[0x5A, 0xB4, 0x00]);
    }

    #[test]
    fn mode_reports() {
        let report = mode_report(ControllerMode::Mouse, FEATURE_REPORT_LEN);
        assert_eq!(report.len(), 64);
        assert_eq!(&report[..5], &[0x5A, 0xD1, 0x01, 0x01, 0x03]);
        assert!(report[5..].iter().all(|&b| b == 0));
        assert_eq!(mode_report(ControllerMode::Gamepad, 64)[4], 1);
        assert_eq!(mode_report(ControllerMode::Wasd, 64)[4], 2);
        // A shorter interface never truncates the command.
        assert_eq!(mode_report(ControllerMode::Gamepad, 2).len(), 5);
    }
}
