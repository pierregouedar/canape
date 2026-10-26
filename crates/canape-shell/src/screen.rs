//! Refresh rate and resolution of the built-in screen. The rate is the one
//! picked on the Hardware page (shell.json), or the active game profile's
//! while that game runs. The resolution is only ever the profile's, or one
//! picked while a game runs: outside games it stays as it is.
//!
//! The shell only switches on a change of profile, so a mode picked in
//! Windows settings in the meantime is left alone. Leaving a profile goes
//! back to the picked rate, or to the value the profile replaced.

use std::sync::Mutex;

/// What the games and their profiles have done to one screen setting (the
/// refresh rate, the resolution) so far.
#[derive(Debug, PartialEq, Eq)]
pub struct Follow<T> {
    /// Game in the foreground at the last step.
    game: Option<String>,
    /// Value the active profile imposes, if any.
    active: Option<T>,
    /// Value in use before a profile, or a pick during a game, changed it.
    before: Option<T>,
}

impl<T> Default for Follow<T> {
    fn default() -> Self {
        Self {
            game: None,
            active: None,
            before: None,
        }
    }
}

impl<T: Copy + PartialEq> Follow<T> {
    /// The value to switch to, given the game in the foreground, its
    /// profile's value, the one picked on the Hardware page outside games
    /// and the current one.
    pub fn step(
        &mut self,
        game: Option<&str>,
        profile: Option<T>,
        picked: Option<T>,
        current: Option<T>,
    ) -> Option<T> {
        if game == self.game.as_deref() && profile == self.active {
            return None;
        }
        let game_left = self.game.is_some() && game != self.game.as_deref();
        let target = match profile {
            Some(value) => {
                if self.before.is_none() {
                    self.before = current;
                }
                Some(value)
            }
            // Back from a profile's value, or from a game it was changed
            // for: the one picked outside games, else the one before.
            None if self.active.is_some() || game_left => {
                let before = self.before.take();
                picked.or(before)
            }
            None => None,
        };
        self.game = game.map(str::to_owned);
        self.active = profile;
        target.filter(|&value| Some(value) != current)
    }

    /// The value is changed by hand while a game is in front: remember the
    /// one to return to when it leaves.
    pub fn note_game_pick(&mut self, current: Option<T>) {
        if self.before.is_none() {
            self.before = current;
        }
    }
}

static FOLLOW: Mutex<Follow<u32>> = Mutex::new(Follow {
    game: None,
    active: None,
    before: None,
});

static FOLLOW_RESOLUTION: Mutex<Follow<(u32, u32)>> = Mutex::new(Follow {
    game: None,
    active: None,
    before: None,
});

/// Development aid off Windows: `CANAPE_MOCK_SCREEN=60,120` fakes a
/// screen with those rates, starting at the last one, and
/// `CANAPE_MOCK_RESOLUTIONS=1920x1080,1280x720` its resolutions (three
/// 16:9 sizes by default), starting at the first.
#[cfg(not(windows))]
static MOCK: Mutex<Option<u32>> = Mutex::new(None);

#[cfg(not(windows))]
static MOCK_RESOLUTION: Mutex<Option<(u32, u32)>> = Mutex::new(None);

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

/// Applies the rate and the resolution of the foreground game's profile
/// (`None`: no profile, or one without them) when the game or its profile
/// changed since the last call. The resolution goes first: the rates on
/// offer depend on it.
pub fn follow(game: Option<&str>, rate: Option<u32>, resolution: Option<(u32, u32)>) {
    follow_resolution(game, resolution);
    let mut follow = FOLLOW.lock().unwrap();
    if follow.game.as_deref() == game && follow.active == rate {
        return;
    }
    let picked = Some(crate::i18n::ShellSettings::load().refresh_hz).filter(|&hz| hz > 0);
    let current = Some(state().0).filter(|&hz| hz > 0);
    if let Some(hz) = follow.step(game, rate, picked, current) {
        set(hz);
    }
}

fn follow_resolution(game: Option<&str>, profile: Option<(u32, u32)>) {
    let mut follow = FOLLOW_RESOLUTION.lock().unwrap();
    if follow.game.as_deref() == game && follow.active == profile {
        return;
    }
    let current = Some(resolution_state().0).filter(|&(w, h)| w > 0 && h > 0);
    if let Some((width, height)) = follow.step(game, profile, None, current) {
        set_resolution(width, height);
    }
}

/// A rate picked by hand: with a game in front it is only for now (saved in
/// the game's profile with X); otherwise it becomes the rate outside games.
pub fn pick(hz: u32, game_in_front: bool) {
    if game_in_front {
        let current = Some(state().0).filter(|&hz| hz > 0);
        FOLLOW.lock().unwrap().note_game_pick(current);
    } else {
        let mut settings = crate::i18n::ShellSettings::load();
        settings.refresh_hz = hz;
        if let Err(err) = settings.save() {
            crate::log(&format!("saving shell.json: {err}"));
        }
    }
    set(hz);
}

/// The current resolution (`(0, 0)` without a built-in screen) and the
/// ones offered, largest first.
pub fn resolution_state() -> ((u32, u32), Vec<(u32, u32)>) {
    #[cfg(windows)]
    {
        canape_hw::display::Screen::internal().map_or(((0, 0), Vec::new()), |screen| {
            (screen.resolution().unwrap_or((0, 0)), screen.resolutions())
        })
    }
    #[cfg(not(windows))]
    {
        if mock_rates().is_empty() {
            return ((0, 0), Vec::new());
        }
        let sizes = mock_resolutions();
        let current = MOCK_RESOLUTION.lock().unwrap().or(sizes.first().copied());
        (current.unwrap_or((0, 0)), sizes)
    }
}

#[cfg(not(windows))]
fn mock_resolutions() -> Vec<(u32, u32)> {
    let text = std::env::var("CANAPE_MOCK_RESOLUTIONS")
        .unwrap_or_else(|_| "1920x1080,1600x900,1280x720".into());
    text.split(',')
        .filter_map(|size| {
            let (w, h) = size.trim().split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .collect()
}

/// Switches the built-in screen to `width` x `height`, then puts the
/// refresh rate back: changing the size can change the rate with it.
pub fn set_resolution(width: u32, height: u32) {
    #[cfg(windows)]
    {
        let rate = state().0;
        let result = canape_hw::display::Screen::internal()
            .ok_or_else(|| "no built-in screen".to_owned())
            .and_then(|screen| {
                screen
                    .set_resolution(width, height)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                crate::log(&format!("screen: {width}x{height}"));
                if rate > 0 && state().0 != rate {
                    set(rate);
                }
            }
            Err(err) => crate::log(&format!("screen: {width}x{height} refused: {err}")),
        }
    }
    #[cfg(not(windows))]
    {
        *MOCK_RESOLUTION.lock().unwrap() = Some((width, height));
    }
}

/// A resolution picked by hand, while a game runs: only for now (saved in
/// the game's profile with X), back to the previous one when it leaves.
pub fn pick_resolution(width: u32, height: u32) {
    let current = Some(resolution_state().0).filter(|&(w, h)| w > 0 && h > 0);
    FOLLOW_RESOLUTION.lock().unwrap().note_game_pick(current);
    set_resolution(width, height);
}

#[cfg(test)]
mod tests {
    use super::Follow;

    #[test]
    fn resolutions_follow_the_same_way() {
        let mut follow = Follow::<(u32, u32)>::default();
        let game = Some("game.exe");
        assert_eq!(
            follow.step(game, Some((1280, 720)), None, Some((1920, 1080))),
            Some((1280, 720))
        );
        assert_eq!(
            follow.step(game, Some((1280, 720)), None, Some((1280, 720))),
            None
        );
        assert_eq!(
            follow.step(None, None, None, Some((1280, 720))),
            Some((1920, 1080))
        );
        // A game without a resolution leaves the screen alone.
        assert_eq!(follow.step(game, None, None, Some((1920, 1080))), None);
        assert_eq!(follow.step(None, None, None, Some((1920, 1080))), None);
    }

    #[test]
    fn profiles_switch_and_restore() {
        let mut follow = Follow::<u32>::default();
        let game = Some("game.exe");
        // No profile, nothing to do.
        assert_eq!(follow.step(None, None, None, Some(120)), None);
        // A 60 Hz game: switch, remember 120.
        assert_eq!(follow.step(game, Some(60), None, Some(120)), Some(60));
        // Every later status: nothing, even if the user changed it.
        assert_eq!(follow.step(game, Some(60), None, Some(120)), None);
        // Game left with no picked rate: back to what it replaced.
        assert_eq!(follow.step(None, None, None, Some(60)), Some(120));
        // With a picked rate, that one wins.
        assert_eq!(follow.step(game, Some(40), None, Some(120)), Some(40));
        assert_eq!(follow.step(None, None, Some(90), Some(40)), Some(90));
    }

    #[test]
    fn profile_to_profile_keeps_the_first_rate() {
        let mut follow = Follow::<u32>::default();
        assert_eq!(
            follow.step(Some("a.exe"), Some(60), None, Some(120)),
            Some(60)
        );
        assert_eq!(
            follow.step(Some("b.exe"), Some(90), None, Some(60)),
            Some(90)
        );
        assert_eq!(follow.step(None, None, None, Some(90)), Some(120));
        // Already there: no switch.
        assert_eq!(follow.step(Some("c.exe"), Some(120), None, Some(120)), None);
    }

    #[test]
    fn a_rate_picked_during_a_game_ends_with_it() {
        let mut follow = Follow::<u32>::default();
        let game = Some("game.exe");
        assert_eq!(follow.step(game, None, Some(120), Some(120)), None);
        // 60 Hz picked with the game in front, then saved in its profile.
        follow.note_game_pick(Some(120));
        assert_eq!(follow.step(game, Some(60), Some(120), Some(60)), None);
        // The game leaves: back to the rate outside games.
        assert_eq!(follow.step(None, None, Some(120), Some(60)), Some(120));

        // Same without saving a profile, and no rate picked outside games.
        assert_eq!(follow.step(game, None, None, Some(120)), None);
        follow.note_game_pick(Some(120));
        assert_eq!(follow.step(None, None, None, Some(60)), Some(120));
    }
}
