//! The motion sensor: the gyroscope in the screen block, read from the
//! user session (the shell), which turns its readings into mouse movement.
//! It is not part of [`crate::Device`]: that one lives in the service, in
//! session 0, where input cannot be injected into the user's desktop.

use crate::Result;

/// Angular velocity in degrees per second, in the screen's frame for the
/// device's native (landscape) orientation: X to the right along the
/// screen, Y up the screen, Z out of the screen towards the player.
/// Positive values turn counter-clockwise around the axis, seen from its tip.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AngularVelocity {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub trait Gyro: Send {
    /// One line for the log: which sensor, how often it reports.
    fn describe(&self) -> String;
    /// Readings are needed often (`true`) or not at all (`false`, the
    /// sensor's default rate, which saves power).
    fn set_active(&mut self, _active: bool) {}
    /// The latest reading, `None` before the first one.
    fn read(&mut self) -> Result<Option<AngularVelocity>>;
}

/// A gyroscope turning at a constant rate, for development and tests.
#[derive(Debug, Clone, Default)]
pub struct MockGyro {
    pub rate: AngularVelocity,
    pub reads: u32,
}

impl MockGyro {
    /// `CANAPE_MOCK_GYRO=x,y,z` (degrees per second) fakes the sensor.
    pub fn from_env() -> Option<Self> {
        parse_rate(&std::env::var("CANAPE_MOCK_GYRO").ok()?).map(|rate| Self { rate, reads: 0 })
    }
}

impl Gyro for MockGyro {
    fn describe(&self) -> String {
        let r = self.rate;
        format!("gyro: simulated, {} {} {} deg/s", r.x, r.y, r.z)
    }

    fn read(&mut self) -> Result<Option<AngularVelocity>> {
        self.reads += 1;
        Ok(Some(self.rate))
    }
}

/// `"x,y,z"` in degrees per second.
fn parse_rate(text: &str) -> Option<AngularVelocity> {
    let mut parts = text.split(',').map(|p| p.trim().parse::<f32>().ok());
    let rate = AngularVelocity {
        x: parts.next()??,
        y: parts.next()??,
        z: parts.next()??,
    };
    parts.next().is_none().then_some(rate)
}

/// Opens the device's gyroscope: the simulator when `CANAPE_MOCK_GYRO` is
/// set, the Windows sensor stack otherwise.
pub fn open() -> Result<Box<dyn Gyro>> {
    if let Some(mock) = MockGyro::from_env() {
        return Ok(Box::new(mock));
    }
    #[cfg(windows)]
    {
        crate::gyro::WindowsGyro::open().map(|gyro| Box::new(gyro) as Box<dyn Gyro>)
    }
    #[cfg(not(windows))]
    {
        Err(crate::HwError::Unsupported("gyroscope"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_rates() {
        assert_eq!(
            parse_rate("1.5, -2,0"),
            Some(AngularVelocity {
                x: 1.5,
                y: -2.0,
                z: 0.0
            })
        );
        assert_eq!(parse_rate("1,2"), None);
        assert_eq!(parse_rate("1,2,3,4"), None);
        assert_eq!(parse_rate("a,2,3"), None);
        let mut mock = MockGyro {
            rate: AngularVelocity {
                y: 30.0,
                ..Default::default()
            },
            reads: 0,
        };
        assert_eq!(mock.read().unwrap().unwrap().y, 30.0);
        assert_eq!(mock.reads, 1);
    }
}
