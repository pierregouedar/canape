//! The Ally's built-in controller: HID facts and feature reports. The I/O
//! lives in `hid.rs`; this module is plain data, tested on any OS.

use canape_core::{BackButton, BackButtons, ButtonTarget, ControllerMode};

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

/// Command group of the controller settings (`0xD1`) and its button
/// mapping command (`0x02`).
const SETTINGS: u8 = 0xD1;
const SET_MAPPING: u8 = 0x02;
/// Button pairs of the mapping command: the back buttons are pair 8, M2
/// first, M1 second.
const PAIR_BACK_BUTTONS: u8 = 0x08;
/// A pair's payload: four 11-byte bindings, the first button then its
/// secondary binding (with the left button under the screen held), the
/// second button then its secondary binding.
const BINDING_LEN: usize = 11;
const PAIR_LEN: u8 = 4 * BINDING_LEN as u8;

/// Binding groups: the first byte of a binding says where its code goes.
const GROUP_PAD: u8 = 0x01;
const GROUP_KEY: u8 = 0x02;
const GROUP_MOUSE: u8 = 0x03;
const GROUP_MEDIA: u8 = 0x05;

/// Key codes of the back buttons' own keys.
const KEY_M1: u8 = 0x8F;
const KEY_M2: u8 = 0x8E;

/// The 11 bytes that make a button send `target`; `button` gives what
/// [`ButtonTarget::Default`] means. Gamepad codes go in byte 1, keyboard
/// codes in byte 2, media codes in byte 3, mouse codes in byte 4.
pub fn binding(button: BackButton, target: ButtonTarget) -> [u8; BINDING_LEN] {
    use ButtonTarget as T;
    let (group, code) = match target {
        T::Default => (
            GROUP_KEY,
            match button {
                BackButton::M1 => KEY_M1,
                BackButton::M2 => KEY_M2,
            },
        ),
        T::Disabled => return [0; BINDING_LEN],
        T::PadA => (GROUP_PAD, 0x01),
        T::PadB => (GROUP_PAD, 0x02),
        T::PadX => (GROUP_PAD, 0x03),
        T::PadY => (GROUP_PAD, 0x04),
        T::PadLb => (GROUP_PAD, 0x05),
        T::PadRb => (GROUP_PAD, 0x06),
        T::PadLs => (GROUP_PAD, 0x07),
        T::PadRs => (GROUP_PAD, 0x08),
        T::DpadUp => (GROUP_PAD, 0x09),
        T::DpadDown => (GROUP_PAD, 0x0A),
        T::DpadLeft => (GROUP_PAD, 0x0B),
        T::DpadRight => (GROUP_PAD, 0x0C),
        T::PadLt => (GROUP_PAD, 0x0D),
        T::PadRt => (GROUP_PAD, 0x0E),
        T::PadView => (GROUP_PAD, 0x11),
        T::PadMenu => (GROUP_PAD, 0x12),
        T::PadXbox => (GROUP_PAD, 0x13),
        T::KeyEscape => (GROUP_KEY, 0x76),
        T::KeyEnter => (GROUP_KEY, 0x5A),
        T::KeySpace => (GROUP_KEY, 0x29),
        T::KeyTab => (GROUP_KEY, 0x0D),
        T::KeyBackspace => (GROUP_KEY, 0x66),
        T::KeyUp => (GROUP_KEY, 0x98),
        T::KeyDown => (GROUP_KEY, 0x99),
        T::KeyLeft => (GROUP_KEY, 0x9A),
        T::KeyRight => (GROUP_KEY, 0x9B),
        T::KeyPageUp => (GROUP_KEY, 0x96),
        T::KeyPageDown => (GROUP_KEY, 0x97),
        T::KeyShift => (GROUP_KEY, 0x88),
        T::KeyCtrl => (GROUP_KEY, 0x8C),
        T::KeyWindows => (GROUP_KEY, 0x82),
        T::KeyF11 => (GROUP_KEY, 0x78),
        T::KeyF12 => (GROUP_KEY, 0x07),
        T::MouseLeft => (GROUP_MOUSE, 0x01),
        T::MouseRight => (GROUP_MOUSE, 0x02),
        T::MouseMiddle => (GROUP_MOUSE, 0x03),
        T::WheelUp => (GROUP_MOUSE, 0x04),
        T::WheelDown => (GROUP_MOUSE, 0x05),
        T::MuteMic => (GROUP_MEDIA, 0x01),
        T::VolumeDown => (GROUP_MEDIA, 0x02),
        T::VolumeUp => (GROUP_MEDIA, 0x03),
        T::ShowKeyboard => (GROUP_MEDIA, 0x19),
    };
    let mut bytes = [0; BINDING_LEN];
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

/// `[0x5A, 0xD1, 0x02, 0x08, 0x2C]` then M2, M2 held with the left
/// screen button, M1, M1 held. The secondary bindings keep the buttons'
/// own keys.
pub fn back_buttons_report(buttons: &BackButtons, len: usize) -> Vec<u8> {
    let mut report = vec![
        REPORT_ID,
        SETTINGS,
        SET_MAPPING,
        PAIR_BACK_BUTTONS,
        PAIR_LEN,
    ];
    for button in [BackButton::M2, BackButton::M1] {
        report.extend_from_slice(&binding(button, buttons.get(button)));
        report.extend_from_slice(&binding(button, ButtonTarget::Default));
    }
    pad(report, len)
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

    #[test]
    fn factory_back_buttons() {
        let report = back_buttons_report(&BackButtons::default(), FEATURE_REPORT_LEN);
        assert_eq!(report.len(), 64);
        assert_eq!(&report[..5], &[0x5A, 0xD1, 0x02, 0x08, 0x2C]);
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
        let report = back_buttons_report(&buttons, FEATURE_REPORT_LEN);
        // M2: nothing, M2 held: its key, M1: A, M1 held: its key.
        assert!(report[5..16].iter().all(|&b| b == 0));
        assert_eq!(&report[16..19], &[0x02, 0x00, 0x8E]);
        assert_eq!(&report[27..29], &[0x01, 0x01]);
        assert!(report[29..38].iter().all(|&b| b == 0));
        assert_eq!(&report[38..41], &[0x02, 0x00, 0x8F]);
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
