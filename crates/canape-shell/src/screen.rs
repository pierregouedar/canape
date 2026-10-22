//! Refresh rate of the built-in screen: the one picked on the Hardware page
//! (shell.json), or the active game profile's while that game runs.
//!
//! The shell only switches on a change of profile, so a rate picked in
//! Windows settings in the meantime is left alone. Leaving a profile goes
//! back to the picked rate, or to the one the profile replaced.

use std::sync::Mutex;

/// What the profiles have done to the screen so far.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Follow {
    /// Rate the active profile imposes, if any.
    active: Option<u32>,
    /// Rate in use before the first profile changed it.
    before: Option<u32>,
}

impl Follow {
    /// The rate to switch to, given the active profile's rate, the rate
    /// picked on the Hardware page (0: none) and the current one.
    pub fn step(&mut self, profile: Option<u32>, picked: u32, current: Option<u32>) -> Option<u32> {
        if profile == self.active {
            return None;
        }
        let target = match profile {
            Some(hz) => {
                if self.active.is_none() {
                    self.before = current;
                }
                Some(hz)
            }
            None => {
                let before = self.before.take();
                if picked > 0 { Some(picked) } else { before }
            }
        };
        self.active = profile;
        target.filter(|&hz| Some(hz) != current)
    }
}

static FOLLOW: Mutex<Follow> = Mutex::new(Follow {
    active: None,
    before: None,
});

/// Development aid off Windows: `CANAPE_MOCK_SCREEN=60,120` fakes a
/// screen with those rates, starting at the last one.
#[cfg(not(windows))]
static MOCK: Mutex<Option<u32>> = Mutex::new(None);

#[cfg(not(windows))]
fn mock_rates() -> Vec<u32> {
    std::env::var("CANAPE_MOCK_SCREEN")
        .unwrap_or_default()
        .split(',')
        .filter_map(|r| r.trim().parse().ok())
        .collect()
}

/// The current rate and the rates offered, `(0, [])` without a built-in
/// screen.
pub fn state() -> (u32, Vec<u32>) {
    #[cfg(windows)]
    {
        canape_hw::display::Screen::internal().map_or((0, Vec::new()), |screen| {
            (screen.refresh().unwrap_or(0), screen.rates())
        })
    }
    #[cfg(not(windows))]
    {
        let rates = mock_rates();
        let current = MOCK.lock().unwrap().or(rates.last().copied());
        (current.unwrap_or(0), rates)
    }
}

/// Switches the built-in screen to `hz`.
pub fn set(hz: u32) {
    #[cfg(windows)]
    {
        let result = canape_hw::display::Screen::internal()
            .ok_or_else(|| "no built-in screen".to_owned())
            .and_then(|screen| screen.set_refresh(hz).map_err(|e| e.to_string()));
        match result {
            Ok(()) => crate::log(&format!("screen: {hz} Hz")),
            Err(err) => crate::log(&format!("screen: {hz} Hz refused: {err}")),
        }
    }
    #[cfg(not(windows))]
    {
        *MOCK.lock().unwrap() = Some(hz);
    }
}

/// Applies the active profile's rate (`None`: no profile, or one without a
/// rate) when it changed since the last call.
pub fn follow(profile: Option<u32>) {
    let mut follow = FOLLOW.lock().unwrap();
    if follow.active == profile {
        return;
    }
    let picked = crate::i18n::ShellSettings::load().refresh_hz;
    let current = Some(state().0).filter(|&hz| hz > 0);
    if let Some(hz) = follow.step(profile, picked, current) {
        set(hz);
    }
}

#[cfg(test)]
mod tests {
    use super::Follow;

    #[test]
    fn profiles_switch_and_restore() {
        let mut follow = Follow::default();
        // No profile, nothing to do.
        assert_eq!(follow.step(None, 0, Some(120)), None);
        // A 60 Hz game: switch, remember 120.
        assert_eq!(follow.step(Some(60), 0, Some(120)), Some(60));
        // Every later status: nothing, even if the user changed it.
        assert_eq!(follow.step(Some(60), 0, Some(120)), None);
        // Game left with no picked rate: back to what it replaced.
        assert_eq!(follow.step(None, 0, Some(60)), Some(120));
        // With a picked rate, that one wins.
        assert_eq!(follow.step(Some(40), 0, Some(120)), Some(40));
        assert_eq!(follow.step(None, 90, Some(40)), Some(90));
    }

    #[test]
    fn profile_to_profile_keeps_the_first_rate() {
        let mut follow = Follow::default();
        assert_eq!(follow.step(Some(60), 0, Some(120)), Some(60));
        assert_eq!(follow.step(Some(90), 0, Some(60)), Some(90));
        assert_eq!(follow.step(None, 0, Some(90)), Some(120));
        // Already there: no switch.
        assert_eq!(follow.step(Some(120), 0, Some(120)), None);
    }
}
