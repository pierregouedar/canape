//! ASUS ATK ACPI interface: device IDs and buffer layouts.
//!
//! reimplemented here; no code is copied. The I/O itself lives in
//! `windows.rs`; everything in this module is plain data, tested on any OS.

use canape_core::{Fan, FanCurve};

/// `\\.\ATKACPI`, opened read/write, shared, `OPEN_EXISTING`.
pub const DEVICE_PATH: &str = r"\\.\ATKACPI";
pub const IOCTL: u32 = 0x0022_240C;

/// Method IDs: little-endian ASCII.
pub const DSTS: u32 = 0x5354_5344; // "DSTS", read
pub const DEVS: u32 = 0x5356_4544; // "DEVS", write

/// Size of the output buffer for plain reads and writes.
pub const OUTPUT_LEN: usize = 16;
/// Bit set by DSTS when the device ID exists.
pub const PRESENT: u32 = 0x0001_0000;

pub const PERFORMANCE_MODE: u32 = 0x0012_0075;
/// SPL / PL1, sustained limit.
pub const PPT_SPL: u32 = 0x0012_00A3;
/// sPPT / PL2, slow boost limit.
pub const PPT_SPPT: u32 = 0x0012_00A0;
/// fPPT, fast boost limit.
pub const PPT_FPPT: u32 = 0x0012_00C1;
pub const BATTERY_LIMIT: u32 = 0x0012_0057;
pub const TEMP_CPU: u32 = 0x0012_0094;

pub const FAN_CPU: u32 = 0x0011_0013;
pub const FAN_GPU: u32 = 0x0011_0014;
pub const FAN_MID: u32 = 0x0011_0031;

pub const FAN_CURVE_CPU: u32 = 0x0011_0024;
pub const FAN_CURVE_GPU: u32 = 0x0011_0025;
pub const FAN_CURVE_MID: u32 = 0x0011_0032;

pub const FAN_CURVE_DELAY_MS: u64 = 100;

/// Firmware values for [`PERFORMANCE_MODE`].
pub mod mode {
    pub const BALANCED: u32 = 0;
    pub const TURBO: u32 = 1;
    pub const SILENT: u32 = 2;
    pub const MANUAL: u32 = 4;
}

/// Mode argument of a fan curve read (DSTS): the firmware swaps silent and
pub fn fan_curve_mode(performance_mode: u32) -> u32 {
    match performance_mode {
        mode::TURBO => 2,
        mode::SILENT => 1,
        _ => 0,
    }
}

pub fn fan_curve_id(fan: Fan) -> u32 {
    match fan {
        Fan::Cpu => FAN_CURVE_CPU,
        Fan::Gpu => FAN_CURVE_GPU,
        Fan::Mid => FAN_CURVE_MID,
    }
}

pub fn fan_speed_id(fan: Fan) -> u32 {
    match fan {
        Fan::Cpu => FAN_CPU,
        Fan::Gpu => FAN_GPU,
        Fan::Mid => FAN_MID,
    }
}

/// Battery charge limit accepted by the firmware, in percent.
pub const BATTERY_LIMIT_RANGE: (u8, u8) = (40, 100);

/// `[method][args length][args…]`, all `u32` little-endian.
pub fn call_buffer(method: u32, args: &[u32]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(8 + 4 * args.len());
    buffer.extend_from_slice(&method.to_le_bytes());
    buffer.extend_from_slice(&((4 * args.len()) as u32).to_le_bytes());
    for arg in args {
        buffer.extend_from_slice(&arg.to_le_bytes());
    }
    buffer
}

pub fn set_buffer(device_id: u32, value: u32) -> Vec<u8> {
    call_buffer(DEVS, &[device_id, value])
}

pub fn get_buffer(device_id: u32) -> Vec<u8> {
    call_buffer(DSTS, &[device_id, 0])
}

/// DEVS with a byte payload: `[DEVS][4 + len][device id][payload]`, as
/// whose length field is 20.
pub fn set_bytes_buffer(device_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(12 + payload.len());
    buffer.extend_from_slice(&DEVS.to_le_bytes());
    buffer.extend_from_slice(&((4 + payload.len()) as u32).to_le_bytes());
    buffer.extend_from_slice(&device_id.to_le_bytes());
    buffer.extend_from_slice(payload);
    buffer
}

/// Writes a fan curve: 8 temperatures then 8 duties.
pub fn fan_curve_buffer(fan: Fan, curve: &FanCurve) -> Vec<u8> {
    set_bytes_buffer(fan_curve_id(fan), &curve.to_bytes())
}

/// Reads the firmware's curve for a performance mode: DSTS with the mode as
pub fn fan_curve_read_buffer(fan: Fan, performance_mode: u32) -> Vec<u8> {
    call_buffer(DSTS, &[fan_curve_id(fan), fan_curve_mode(performance_mode)])
}

/// the low 16 bits; above 120, or 0 without the presence bit, there is no
/// reading. The unit is 100 RPM (app/HardwareControl.cs:859-860).
pub fn fan_rpm(output: &[u8; OUTPUT_LEN]) -> Option<u32> {
    let raw = (result(output) as i32).wrapping_sub(PRESENT as i32);
    let speed = raw & 0xFFFF;
    if speed > 120 || (speed == 0 && raw < 0) {
        return None;
    }
    Some(speed as u32 * 100)
}

/// First `u32` of the output buffer.
pub fn result(output: &[u8; OUTPUT_LEN]) -> u32 {
    u32::from_le_bytes([output[0], output[1], output[2], output[3]])
}

/// A DEVS write succeeded when the firmware returns 1.
pub fn set_succeeded(output: &[u8; OUTPUT_LEN]) -> bool {
    result(output) == 1
}

/// A DSTS value, or `None` when the device ID does not exist.
pub fn get_value(output: &[u8; OUTPUT_LEN]) -> Option<u32> {
    (result(output) as i32 - PRESENT as i32).try_into().ok()
}

/// Ally models, from `SystemProductName` (for example "RC71L").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// RC71L, Z1 / Z1 Extreme.
    Ally,
    /// RC72LA, Z1 Extreme.
    AllyX,
    /// RC73YA, Z2 A.
    XboxAlly,
    /// RC73XA, Z2 Extreme.
    XboxAllyX,
}

impl Model {
    pub fn detect(product_name: &str) -> Option<Self> {
        let name = product_name.to_ascii_uppercase();
        [
            ("RC73XA", Self::XboxAllyX),
            ("RC73YA", Self::XboxAlly),
            ("RC72", Self::AllyX),
            ("RC71", Self::Ally),
        ]
        .into_iter()
        .find_map(|(code, model)| name.contains(code).then_some(model))
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ally => "ROG Ally",
            Self::AllyX => "ROG Ally X",
            Self::XboxAlly => "ROG Xbox Ally",
            Self::XboxAllyX => "ROG Xbox Ally X",
        }
    }

    /// against an ASUS spec yet.
    pub fn tdp_range(self) -> (u32, u32) {
        match self {
            Self::Ally | Self::AllyX => (5, 30),
            Self::XboxAlly | Self::XboxAllyX => (5, 35),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_core::FanPreset;

    #[test]
    fn method_ids_spell_their_names() {
        assert_eq!(&DSTS.to_le_bytes(), b"DSTS");
        assert_eq!(&DEVS.to_le_bytes(), b"DEVS");
    }

    #[test]
    fn set_buffer_layout() {
        let buffer = set_buffer(PPT_SPL, 15);
        assert_eq!(buffer.len(), 16);
        assert_eq!(&buffer[0..4], b"DEVS");
        assert_eq!(&buffer[4..8], &8u32.to_le_bytes());
        assert_eq!(&buffer[8..12], &0x0012_00A3u32.to_le_bytes());
        assert_eq!(&buffer[12..16], &15u32.to_le_bytes());
    }

    #[test]
    fn get_value_strips_the_presence_bit() {
        let mut output = [0u8; OUTPUT_LEN];
        output[..4].copy_from_slice(&(PRESENT + 58).to_le_bytes());
        assert_eq!(get_value(&output), Some(58));
        output[..4].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(get_value(&output), None);
        output[..4].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        assert_eq!(get_value(&output), None);
    }

    #[test]
    fn fan_curve_buffer_layout() {
        let curve = FanPreset::Balanced.curve();
        let buffer = fan_curve_buffer(Fan::Gpu, &curve);
        assert_eq!(buffer.len(), 28);
        assert_eq!(&buffer[0..4], b"DEVS");
        assert_eq!(&buffer[4..8], &20u32.to_le_bytes());
        assert_eq!(&buffer[8..12], &0x0011_0025u32.to_le_bytes());
        assert_eq!(&buffer[12..20], &[40, 50, 58, 65, 72, 78, 85, 92]);
        assert_eq!(&buffer[20..28], &[10, 18, 26, 34, 42, 50, 62, 75]);
    }

    #[test]
    fn fan_curve_read_swaps_silent_and_turbo() {
        let read = fan_curve_read_buffer(Fan::Cpu, mode::SILENT);
        assert_eq!(read.len(), 16);
        assert_eq!(&read[0..4], b"DSTS");
        assert_eq!(&read[8..12], &0x0011_0024u32.to_le_bytes());
        assert_eq!(&read[12..16], &1u32.to_le_bytes());
        assert_eq!(fan_curve_mode(mode::TURBO), 2);
        assert_eq!(fan_curve_mode(mode::BALANCED), 0);
        assert_eq!(fan_curve_mode(mode::MANUAL), 0);
        assert_eq!(fan_curve_id(Fan::Mid), 0x0011_0032);
    }

    #[test]
    fn fan_rpm_follows_g_helper_validity() {
        let output = |value: u32| {
            let mut out = [0u8; OUTPUT_LEN];
            out[..4].copy_from_slice(&value.to_le_bytes());
            out
        };
        assert_eq!(fan_rpm(&output(PRESENT + 32)), Some(3200));
        assert_eq!(fan_rpm(&output(PRESENT)), Some(0), "fan stopped");
        assert_eq!(fan_rpm(&output(PRESENT + 121)), None);
        assert_eq!(fan_rpm(&output(0)), None, "absent");
        assert_eq!(fan_rpm(&output(0xFFFF_FFFE)), None);
        assert_eq!(fan_speed_id(Fan::Gpu), 0x0011_0014);
    }

    #[test]
    fn detects_models() {
        assert_eq!(Model::detect("ROG Ally RC71L_RC71L"), Some(Model::Ally));
        assert_eq!(Model::detect("RC72LA"), Some(Model::AllyX));
        assert_eq!(Model::detect("RC73YA"), Some(Model::XboxAlly));
        assert_eq!(Model::detect("rc73xa"), Some(Model::XboxAllyX));
        assert_eq!(Model::detect("G614JV"), None);
    }
}
