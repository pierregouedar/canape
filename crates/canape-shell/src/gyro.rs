//! Gyro aiming: the device's rotation moves the mouse while a game is in
//! front. The settings come from the service in every status push
//! ([`follow`]); the back paddles arrive from the button listener
//! ([`paddle`]). A thread reads the gyroscope through `canape_hw::motion`
//! every 10 ms while aiming, and sleeps otherwise.
//!
//! The conversion ([`Aim`]) and the activation rules ([`Trigger`]) are pure
//! and tested on any OS; the thread and `SendInput` are Windows only.

use canape_core::{GyroActivation, GyroAxis, GyroSettings};
use canape_hw::motion::AngularVelocity;

/// Rate subtracted from every reading, in degrees per second: absorbs the
/// sensor's noise at rest without cutting off slow movements.
const DEADZONE_DPS: f32 = 0.75;
/// Longest time one reading stands for, in seconds: a late wake-up must not
/// turn into a jump.
const MAX_STEP_SECS: f32 = 0.05;

/// Turns angular velocity into whole mouse counts, keeping the fractions
/// from one reading to the next so that slow movements still add up.
#[derive(Debug, Default)]
pub struct Aim {
    remainder: (f32, f32),
}

impl Aim {
    /// Mouse movement `(dx, dy)` for `rate` held during `secs` seconds;
    /// screen coordinates (right and down are positive).
    pub fn step(
        &mut self,
        settings: &GyroSettings,
        rate: AngularVelocity,
        secs: f32,
    ) -> (i32, i32) {
        let secs = secs.clamp(0.0, MAX_STEP_SECS);
        let scale = settings.sensitivity as f32 * secs;
        let horizontal = match settings.horizontal {
            GyroAxis::Yaw => rate.y,
            GyroAxis::Roll => rate.z,
        };
        // Turning left (positive around Y or Z) moves left; tilting the top
        // away (positive around X) moves up.
        let mut dy = -soften(rate.x) * scale;
        if settings.invert_y {
            dy = -dy;
        }
        let dx = -soften(horizontal) * scale + self.remainder.0;
        let dy = dy + self.remainder.1;
        let (x, y) = (dx.trunc(), dy.trunc());
        self.remainder = (dx - x, dy - y);
        (x as i32, y as i32)
    }

    /// Forgets the fractions, when aiming starts again.
    pub fn reset(&mut self) {
        self.remainder = (0.0, 0.0);
    }
}

fn soften(rate: f32) -> f32 {
    rate.signum() * (rate.abs() - DEADZONE_DPS).max(0.0)
}

/// Whether aiming is on, from the activation setting and the back paddles.
#[derive(Debug, Default)]
pub struct Trigger {
    held: bool,
    toggled: bool,
}

impl Trigger {
    /// A back paddle went down (`true`) or the paddles were released.
    pub fn paddle(&mut self, down: bool) {
        if down && !self.held {
            self.toggled = !self.toggled;
        }
        self.held = down;
    }

    /// The game left: a toggle starts off with the next one.
    pub fn reset(&mut self) {
        self.toggled = false;
    }

    pub fn active(&self, activation: GyroActivation) -> bool {
        match activation {
            GyroActivation::Off => false,
            GyroActivation::Hold => self.held,
            GyroActivation::Toggle => self.toggled,
            GyroActivation::Always => true,
        }
    }
}

#[cfg(windows)]
pub use engine::{follow, paddle, spawn};

#[cfg(windows)]
mod engine {
    use std::sync::{Condvar, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use canape_core::{GyroActivation, GyroSettings};
    use canape_hw::motion::{self, Gyro};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
    };

    use super::{Aim, Trigger};

    /// Time between two readings while aiming.
    pub const TICK: Duration = Duration::from_millis(10);
    /// Check again this often while the shell is in front.
    const SHELL_IN_FRONT: Duration = Duration::from_millis(250);
    /// Wait before looking for a sensor again after a failure.
    const RETRY: Duration = Duration::from_secs(60);

    struct State {
        settings: GyroSettings,
        /// A game is being played (the service's `foreground`).
        game: bool,
        trigger: Trigger,
    }

    impl State {
        fn wanted(&self) -> bool {
            self.game && self.trigger.active(self.settings.activation)
        }
    }

    static STATE: Mutex<State> = Mutex::new(State {
        settings: GyroSettings {
            activation: GyroActivation::Off,
            sensitivity: 10,
            horizontal: canape_core::GyroAxis::Yaw,
            invert_y: false,
        },
        game: false,
        trigger: Trigger {
            held: false,
            toggled: false,
        },
    });
    static CHANGED: Condvar = Condvar::new();

    /// The settings in effect and whether a game is in front, from a
    /// status push.
    pub fn follow(settings: GyroSettings, game: bool) {
        let mut state = STATE.lock().unwrap();
        if state.settings == settings && state.game == game {
            return;
        }
        if !game {
            state.trigger.reset();
        }
        state.settings = settings;
        state.game = game;
        CHANGED.notify_all();
    }

    /// A back paddle went down or the paddles were released.
    pub fn paddle(down: bool) {
        let mut state = STATE.lock().unwrap();
        state.trigger.paddle(down);
        CHANGED.notify_all();
    }

    pub fn spawn() {
        thread::spawn(|| {
            let mut sensor: Option<Box<dyn Gyro>> = None;
            loop {
                wait_until_wanted();
                if sensor.is_none() {
                    match motion::open() {
                        Ok(gyro) => {
                            crate::log(&gyro.describe());
                            sensor = Some(gyro);
                        }
                        Err(err) => {
                            crate::log(&format!("gyro: {err}"));
                            thread::sleep(RETRY);
                            continue;
                        }
                    }
                }
                let gyro = sensor.as_mut().expect("opened above");
                if let Err(err) = aim(gyro.as_mut()) {
                    crate::log(&format!("gyro: {err}"));
                    sensor = None;
                    thread::sleep(Duration::from_secs(5));
                }
            }
        });
    }

    fn wait_until_wanted() {
        let mut state = STATE.lock().unwrap();
        while !state.wanted() {
            state = CHANGED.wait(state).unwrap();
        }
    }

    /// Moves the mouse until aiming is no longer wanted.
    fn aim(gyro: &mut dyn Gyro) -> canape_hw::Result<()> {
        gyro.set_active(true);
        let mut aim = Aim::default();
        let mut last = Instant::now();
        let result = loop {
            let Some(settings) = ({
                let state = STATE.lock().unwrap();
                state.wanted().then_some(state.settings)
            }) else {
                break Ok(());
            };
            if crate::instance::is_foreground() {
                aim.reset();
                thread::sleep(SHELL_IN_FRONT);
                last = Instant::now();
                continue;
            }
            let rate = match gyro.read() {
                Ok(rate) => rate,
                Err(err) => break Err(err),
            };
            let now = Instant::now();
            if let Some(rate) = rate {
                let (dx, dy) = aim.step(&settings, rate, (now - last).as_secs_f32());
                if dx != 0 || dy != 0 {
                    move_mouse(dx, dy);
                }
            }
            last = now;
            thread::sleep(TICK);
        };
        gyro.set_active(false);
        result
    }

    /// Relative movement, as a mouse would send it.
    fn move_mouse(dx: i32, dy: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(sensitivity: u32) -> GyroSettings {
        GyroSettings {
            activation: GyroActivation::Hold,
            sensitivity,
            ..GyroSettings::default()
        }
    }

    fn rate(x: f32, y: f32, z: f32) -> AngularVelocity {
        AngularVelocity { x, y, z }
    }

    /// 1/64 s: with 16 counts per degree, a quarter count per degree per
    /// second, exactly.
    const DT: f32 = 1.0 / 64.0;

    #[test]
    fn turning_left_moves_left_and_tilting_back_moves_up() {
        let mut aim = Aim::default();
        // 16 deg/s once the dead zone is out: 4 counts.
        assert_eq!(aim.step(&settings(16), rate(0.0, 16.75, 0.0), DT), (-4, 0));
        assert_eq!(aim.step(&settings(16), rate(16.75, 0.0, 0.0), DT), (0, -4));
        let inverted = GyroSettings {
            invert_y: true,
            ..settings(16)
        };
        assert_eq!(aim.step(&inverted, rate(16.75, 0.0, 0.0), DT), (0, 4));
        // Roll is ignored with yaw, and drives with roll.
        assert_eq!(aim.step(&settings(16), rate(0.0, 0.0, -16.75), DT), (0, 0));
        let roll = GyroSettings {
            horizontal: GyroAxis::Roll,
            ..settings(16)
        };
        assert_eq!(aim.step(&roll, rate(0.0, 0.0, -16.75), DT), (4, 0));
    }

    #[test]
    fn slow_movements_add_up_and_noise_does_not() {
        let mut aim = Aim::default();
        // A quarter count per reading: one count every four readings.
        let moved: i32 = (0..10)
            .map(|_| aim.step(&settings(16), rate(0.0, -1.75, 0.0), DT).0)
            .sum();
        assert_eq!(moved, 2);
        aim.reset();
        let still: i32 = (0..1000)
            .map(|_| aim.step(&settings(50), rate(0.5, -0.7, 0.6), 0.01).0)
            .sum();
        assert_eq!(still, 0);
    }

    #[test]
    fn a_late_reading_does_not_jump() {
        let mut aim = Aim::default();
        // Two seconds count as 50 ms: 100 deg/s at 10 counts per degree.
        let (dx, _) = aim.step(&settings(10), rate(0.0, 100.75, 0.0), 2.0);
        assert!((dx + 50).abs() <= 1, "{dx}");
        aim.reset();
        assert_eq!(
            aim.step(&settings(10), rate(0.0, 100.75, 0.0), -1.0),
            (0, 0)
        );
    }

    #[test]
    fn paddles_hold_toggle_or_nothing() {
        let mut trigger = Trigger::default();
        assert!(!trigger.active(GyroActivation::Hold));
        assert!(trigger.active(GyroActivation::Always));
        trigger.paddle(true);
        assert!(trigger.active(GyroActivation::Hold));
        assert!(trigger.active(GyroActivation::Toggle));
        assert!(!trigger.active(GyroActivation::Off));
        // A repeated "down" is the same press.
        trigger.paddle(true);
        trigger.paddle(false);
        assert!(!trigger.active(GyroActivation::Hold));
        assert!(trigger.active(GyroActivation::Toggle));
        trigger.paddle(true);
        trigger.paddle(false);
        assert!(!trigger.active(GyroActivation::Toggle));
        trigger.paddle(true);
        trigger.reset();
        assert!(!trigger.active(GyroActivation::Toggle));
    }
}
