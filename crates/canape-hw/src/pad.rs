//! The Ally's built-in controller: HID facts and feature reports. The I/O
//! lives in `hid.rs`; this module is plain data, tested on any OS.

use canape_core::ControllerMode;

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
    pad(vec![REPORT_ID, 0xD1, 0x01, 0x01, mode_value(mode)], len)
}

/// Firmware value of each controller mode: 1 gamepad, 2 WASD, 3 mouse.
pub fn mode_value(mode: ControllerMode) -> u8 {
    match mode {
        ControllerMode::Gamepad => 1,
        ControllerMode::Wasd => 2,
        ControllerMode::Mouse => 3,
    }
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
