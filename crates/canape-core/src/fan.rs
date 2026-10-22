//! Fan curves: the shape a client may ask for, validated here so that every
//! backend applies the same safety rules before anything reaches the firmware.
//!
//! A curve is 8 points `(temperature °C, duty %)`, the layout of the ASUS
//! firmware (8 temperatures then 8 duties, see `canape_hw::atk`).

use serde::{Deserialize, Serialize};

/// The fans the ASUS firmware knows. The Ally has a CPU and a GPU fan; `mid`
/// only exists on some laptops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fan {
    Cpu,
    Gpu,
    Mid,
}

impl Fan {
    pub const ALL: [Fan; 3] = [Fan::Cpu, Fan::Gpu, Fan::Mid];
}

/// Points per curve.
pub const POINTS: usize = 8;

/// Accepted point temperatures, in °C.
pub const TEMP_RANGE: (u8, u8) = (20, 110);

/// Duty is a percentage, 0–100 in the firmware buffer.
pub const MAX_DUTY: u8 = 100;

/// Safety floor: at or above each temperature, the curve must run the fan
/// at least this fast. The firmware either interpolates between points or
/// steps; with non-decreasing duties, the duty of the last point at or below
/// a temperature is a lower bound in both cases ([`FanCurve::duty_at`]).
///
/// The values sit just under ASUS's own Silent curves (28 % CPU / 35 % GPU
/// from 80 °C,
/// 34 % / 40 % from 90 °C, 41 % / 45 % at 100 °C): a user curve may be as
/// quiet as the factory Silent mode, never quieter when hot. So a curve can
/// never hold 0 % at 90 °C.
pub const SAFETY_FLOOR: [(u8, u8); 3] = [(80, 25), (90, 30), (100, 40)];

/// Eight `(temperature °C, duty %)` points. In JSON, an array of eight
/// `[temp, duty]` pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanCurve(pub [(u8, u8); POINTS]);

impl FanCurve {
    /// Checks the rules every curve must follow before it is written:
    /// temperatures strictly ascending within [`TEMP_RANGE`], duties within
    /// 0–100 and never decreasing, and the [`SAFETY_FLOOR`].
    pub fn validate(&self) -> Result<(), String> {
        let (low, high) = TEMP_RANGE;
        let mut previous: Option<(u8, u8)> = None;
        for &(temp, duty) in &self.0 {
            if !(low..=high).contains(&temp) {
                return Err(format!("fan curve: {temp} °C not in {low}..={high} °C"));
            }
            if duty > MAX_DUTY {
                return Err(format!("fan curve: {duty} % above {MAX_DUTY} %"));
            }
            if let Some((t, d)) = previous {
                if temp <= t {
                    return Err(format!(
                        "fan curve: temperatures must ascend ({t} then {temp} °C)"
                    ));
                }
                if duty < d {
                    return Err(format!(
                        "fan curve: duty must not decrease ({d} then {duty} %)"
                    ));
                }
            }
            previous = Some((temp, duty));
        }
        for (temp, min) in SAFETY_FLOOR {
            let duty = self.duty_at(temp);
            if duty < min {
                return Err(format!(
                    "fan curve: {duty} % at {temp} °C, the minimum there is {min} %"
                ));
            }
        }
        Ok(())
    }

    /// Lowest duty the fan can run at `temp`: the duty of the last point at
    /// or below it, 0 below the first point.
    pub fn duty_at(&self, temp: u8) -> u8 {
        self.0
            .iter()
            .take_while(|&&(t, _)| t <= temp)
            .last()
            .map_or(0, |&(_, duty)| duty)
    }

    /// Firmware layout: 8 temperatures, then 8 duties.
    pub fn to_bytes(&self) -> [u8; 2 * POINTS] {
        let mut bytes = [0; 2 * POINTS];
        for (i, &(temp, duty)) in self.0.iter().enumerate() {
            bytes[i] = temp;
            bytes[i + POINTS] = duty;
        }
        bytes
    }

    /// A curve read from the firmware (its default for a mode). `None` when
    /// the buffer is not 16 bytes, is all zero (no curve) or holds values out
    /// of range.
    ///
    /// Makes temperatures increasing: ASUS defaults sometimes repeat a
    /// temperature, so a temperature not above the previous one is moved to
    /// the previous one + 6, capped at 100.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 2 * POINTS || bytes.iter().all(|&b| b == 0) {
            return None;
        }
        let mut points = [(0, 0); POINTS];
        let mut previous = 0u8;
        for (i, point) in points.iter_mut().enumerate() {
            let mut temp = bytes[i];
            if temp <= previous {
                temp = previous.saturating_add(6).min(100);
            }
            *point = (temp, bytes[i + POINTS]);
            previous = temp;
        }
        let in_range = points
            .iter()
            .all(|&(t, d)| t <= TEMP_RANGE.1 && d <= MAX_DUTY);
        in_range.then_some(Self(points))
    }
}

/// Ready-made curves for UIs, valid by construction (see the tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanPreset {
    /// Fans off while cool, just above the safety floor when hot.
    Silent,
    /// Close to ASUS's own balanced behaviour.
    Balanced,
    /// Runs early and hard: noisy, coolest.
    MaxCooling,
}

impl FanPreset {
    pub const ALL: [FanPreset; 3] = [Self::Silent, Self::Balanced, Self::MaxCooling];

    /// The same curve serves the CPU and the GPU fan.
    pub fn curve(self) -> FanCurve {
        FanCurve(match self {
            Self::Silent => [
                (30, 0),
                (45, 0),
                (55, 10),
                (62, 15),
                (68, 20),
                (75, 26),
                (85, 34),
                (95, 45),
            ],
            Self::Balanced => [
                (40, 10),
                (50, 18),
                (58, 26),
                (65, 34),
                (72, 42),
                (78, 50),
                (85, 62),
                (92, 75),
            ],
            Self::MaxCooling => [
                (30, 35),
                (40, 45),
                (50, 55),
                (60, 65),
                (70, 80),
                (75, 90),
                (80, 100),
                (85, 100),
            ],
        })
    }
}

/// Custom curves per fan; `None` leaves that fan on the firmware's curve
/// for the current performance mode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FanCurves {
    pub cpu: Option<FanCurve>,
    pub gpu: Option<FanCurve>,
    pub mid: Option<FanCurve>,
}

impl FanCurves {
    pub fn get(&self, fan: Fan) -> Option<&FanCurve> {
        match fan {
            Fan::Cpu => self.cpu.as_ref(),
            Fan::Gpu => self.gpu.as_ref(),
            Fan::Mid => self.mid.as_ref(),
        }
    }

    pub fn set(&mut self, fan: Fan, curve: Option<FanCurve>) {
        *match fan {
            Fan::Cpu => &mut self.cpu,
            Fan::Gpu => &mut self.gpu,
            Fan::Mid => &mut self.mid,
        } = curve;
    }

    /// The custom curves, CPU first.
    pub fn iter(&self) -> impl Iterator<Item = (Fan, FanCurve)> + '_ {
        Fan::ALL
            .into_iter()
            .filter_map(|fan| self.get(fan).map(|&curve| (fan, curve)))
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(temps: [u8; 8], duties: [u8; 8]) -> FanCurve {
        let mut points = [(0, 0); 8];
        for i in 0..8 {
            points[i] = (temps[i], duties[i]);
        }
        FanCurve(points)
    }

    /// ASUS's default curves, as bytes.
    const ASUS_DEFAULTS: [[u8; 16]; 6] = [
        // Turbo GPU, CPU.
        [
            0x1E, 0x3F, 0x44, 0x48, 0x4C, 0x50, 0x54, 0x62, 0x16, 0x1F, 0x26, 0x2D, 0x39, 0x47,
            0x55, 0x5F,
        ],
        [
            0x1E, 0x3F, 0x44, 0x48, 0x4C, 0x50, 0x54, 0x62, 0x11, 0x1A, 0x22, 0x29, 0x34, 0x43,
            0x51, 0x5A,
        ],
        // Silent GPU, CPU.
        [
            0x1E, 0x31, 0x3B, 0x42, 0x47, 0x50, 0x5A, 0x64, 0x00, 0x00, 0x04, 0x11, 0x1B, 0x23,
            0x28, 0x2D,
        ],
        [
            0x1E, 0x31, 0x3B, 0x42, 0x47, 0x50, 0x5A, 0x64, 0x00, 0x00, 0x03, 0x0C, 0x14, 0x1C,
            0x22, 0x29,
        ],
        // Balanced GPU, CPU.
        [
            0x3A, 0x3D, 0x40, 0x44, 0x48, 0x4D, 0x51, 0x62, 0x0C, 0x16, 0x1D, 0x1F, 0x26, 0x2D,
            0x34, 0x4A,
        ],
        [
            0x3A, 0x3D, 0x40, 0x44, 0x48, 0x4D, 0x51, 0x62, 0x08, 0x11, 0x16, 0x1A, 0x22, 0x29,
            0x30, 0x45,
        ],
    ];

    #[test]
    fn presets_are_valid() {
        for preset in FanPreset::ALL {
            assert_eq!(preset.curve().validate(), Ok(()), "{preset:?}");
        }
    }

    #[test]
    fn asus_defaults_pass_the_safety_floor() {
        for bytes in ASUS_DEFAULTS {
            let curve = FanCurve::from_bytes(&bytes).unwrap();
            assert_eq!(curve.validate(), Ok(()), "{bytes:02X?}");
            assert_eq!(curve.to_bytes(), bytes);
        }
    }

    #[test]
    fn rejects_zero_duty_when_hot() {
        let silent_at_90 = curve(
            [30, 40, 50, 60, 70, 80, 90, 100],
            [0, 0, 0, 0, 0, 30, 0, 50],
        );
        assert!(silent_at_90.validate().is_err());
        let off = curve([30, 40, 50, 60, 70, 80, 90, 100], [0; 8]);
        let err = off.validate().unwrap_err();
        assert!(err.contains("80 °C"), "{err}");
        // A curve that ends before 80 °C keeps its last duty above it.
        let early = curve(
            [20, 25, 30, 35, 40, 45, 50, 55],
            [0, 0, 0, 0, 10, 20, 30, 40],
        );
        assert_eq!(early.validate(), Ok(()));
        // A curve that starts above 80 °C leaves the fan unknown at 80 °C.
        let late = curve(
            [82, 84, 86, 88, 90, 92, 94, 96],
            [100, 100, 100, 100, 100, 100, 100, 100],
        );
        assert!(late.validate().is_err());
    }

    #[test]
    fn rejects_malformed_curves() {
        let base = FanPreset::Balanced.curve();
        let mut c = base;
        c.0[3].0 = c.0[2].0;
        assert!(c.validate().unwrap_err().contains("ascend"));
        let mut c = base;
        c.0[7].1 = 101;
        assert!(c.validate().unwrap_err().contains("101"));
        let mut c = base;
        c.0[0].0 = 10;
        assert!(c.validate().is_err());
        let mut c = base;
        c.0[7].0 = 111;
        assert!(c.validate().is_err());
        let mut c = base;
        c.0[4].1 = 5;
        assert!(c.validate().unwrap_err().contains("decrease"));
    }

    #[test]
    fn duty_at_is_a_step_lower_bound() {
        let c = FanPreset::Balanced.curve();
        assert_eq!(c.duty_at(39), 0);
        assert_eq!(c.duty_at(40), 10);
        assert_eq!(c.duty_at(84), 50);
        assert_eq!(c.duty_at(110), 75);
    }

    #[test]
    fn firmware_bytes_fix_repeated_temperatures_and_reject_empty() {
        assert_eq!(FanCurve::from_bytes(&[0; 16]), None);
        assert_eq!(FanCurve::from_bytes(&[1; 15]), None);
        let mut bytes = ASUS_DEFAULTS[5];
        bytes[2] = bytes[1]; // 61 °C twice
        let fixed = FanCurve::from_bytes(&bytes).unwrap();
        assert_eq!(fixed.0[1].0, 61);
        assert_eq!(fixed.0[2].0, 67);
        // Garbage (duty above 100) is not a curve.
        let mut bytes = ASUS_DEFAULTS[5];
        bytes[15] = 0xFF;
        assert_eq!(FanCurve::from_bytes(&bytes), None);
    }

    #[test]
    fn curves_serialize_as_pairs() {
        let mut curves = FanCurves::default();
        assert!(curves.is_empty());
        curves.set(Fan::Gpu, Some(FanPreset::Silent.curve()));
        let json = serde_json::to_string(&curves).unwrap();
        assert!(
            json.starts_with(r#"{"cpu":null,"gpu":[[30,0],[45,0],"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<FanCurves>(&json).unwrap(), curves);
        assert_eq!(
            serde_json::from_str::<FanCurves>("{}").unwrap(),
            FanCurves::default()
        );
        assert_eq!(
            curves.iter().map(|(fan, _)| fan).collect::<Vec<_>>(),
            [Fan::Gpu]
        );
    }
}
