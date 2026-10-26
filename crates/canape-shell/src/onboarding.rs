//! First-run tour: shown once, over the whole window, until finished or put
//! off (`onboarded` in shell.json). The step reached is saved as it changes
//! (`onboarding_step`), so a restart comes back to it. Settings can show
//! it again, and so can `CANAPE_ONBOARDING=1` (development).
//!
//! The steps only call what the settings page already uses (language,
//! accent, sign-ins, games folder, the Ally's buttons); this module adds
//! the keys typed or pasted on the stores step and the touch keyboard.

use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

use crate::i18n::ShellSettings;
use crate::{App, AppWindow, Onboarding, Tweaks};

/// Steps, as `Onboarding.step` numbers them (see onboarding.slint).
const CONFLICTS_STEP: u32 = 3;
const STORES_STEP: u32 = 4;
const TWEAKS_STEP: u32 = 7;
const LAST_STEP: u32 = 8;

/// The Windows optimisations step (7): applies the recommended set through
/// the tweaks page's own callback, and shows its outcome.
const WINDOWS_TWEAKS: bool = true;

/// How often the stores step reads what the library found.
const REFRESH: Duration = Duration::from_secs(1);

/// What a key entry is for, as the page names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    SteamKey,
    IgdbId,
    IgdbSecret,
}

impl Entry {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "steam" => Some(Self::SteamKey),
            "igdb-id" => Some(Self::IgdbId),
            "igdb-secret" => Some(Self::IgdbSecret),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SteamKey => "steam",
            Self::IgdbId => "igdb-id",
            Self::IgdbSecret => "igdb-secret",
        }
    }

    /// The value to save from `text` (typed, or the clipboard's), if it
    /// looks like one.
    fn read(self, text: &str) -> Option<String> {
        match self {
            Self::SteamKey => steam_key(text),
            Self::IgdbId | Self::IgdbSecret => twitch_value(text),
        }
    }

    fn slot(self, settings: &mut ShellSettings) -> &mut String {
        match self {
            Self::SteamKey => &mut settings.steam_api_key,
            Self::IgdbId => &mut settings.igdb_client_id,
            Self::IgdbSecret => &mut settings.igdb_client_secret,
        }
    }
}

/// A Steam Web API key in `text`: 32 hexadecimal digits, alone or among
/// other words (the key's page reads "Key: …").
fn steam_key(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|word| word.len() == 32 && word.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_uppercase)
}

/// A Twitch application's client id or secret: one word of letters and
/// digits (30 of them today).
fn twitch_value(text: &str) -> Option<String> {
    let text = text.trim();
    ((20..=64).contains(&text.len()) && text.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| text.to_owned())
}

/// A saved secret as the page shows it: its last four characters only.
fn masked(secret: &str) -> String {
    let secret = secret.trim();
    let count = secret.chars().count();
    if count == 0 {
        String::new()
    } else if count < 8 {
        "••••".into()
    } else {
        let tail: String = secret.chars().skip(count - 4).collect();
        format!("•••• {tail}")
    }
}

/// Step to show after a restart: the saved one, unless it only shows under
/// a condition not known yet at startup.
fn resume_step(saved: u32, tweaks: bool) -> u32 {
    match saved.min(LAST_STEP) {
        CONFLICTS_STEP => STORES_STEP,
        TWEAKS_STEP if !tweaks => LAST_STEP,
        step => step,
    }
}

fn save(settings: &ShellSettings) {
    if let Err(err) = settings.save() {
        crate::log(&format!("saving shell.json: {err}"));
    }
}

/// Shows the saved keys, masked.
fn show_keys(ui: &AppWindow, settings: &ShellSettings) {
    let tour = ui.global::<Onboarding>();
    tour.set_steam_key(masked(&settings.steam_api_key).into());
    tour.set_igdb_id(masked(&settings.igdb_client_id).into());
    tour.set_igdb_secret(masked(&settings.igdb_client_secret).into());
}

fn show_steam(ui: &AppWindow) {
    let (found, games) = crate::library::steam_summary();
    let tour = ui.global::<Onboarding>();
    tour.set_steam_found(found);
    tour.set_steam_games(i32::try_from(games).unwrap_or(i32::MAX));
}

/// Saves `entry` from `text`; `pasted` says where the text came from, for
/// the reason shown when it is refused. An empty entry removes the value.
fn save_entry(ui: &AppWindow, entry: Entry, text: Option<String>) {
    let tour = ui.global::<Onboarding>();
    let pasted = text.is_none();
    let text = text.or_else(crate::clipboard::text).unwrap_or_default();
    let value = if text.trim().is_empty() && !pasted {
        Some(String::new())
    } else {
        entry.read(&text)
    };
    let Some(value) = value else {
        tour.set_refused(entry.name().into());
        // 1: what was typed is not a key; 2: the clipboard holds none.
        tour.set_refused_why(if pasted { 2 } else { 1 });
        return;
    };
    tour.set_refused("".into());
    let mut settings = ShellSettings::load();
    *entry.slot(&mut settings) = value;
    save(&settings);
    // Which value changed, never the value.
    crate::log(&format!("onboarding: {} saved", entry.name()));
    show_keys(ui, &settings);
    if entry == Entry::SteamKey {
        crate::library::steam_key_changed();
    }
}

/// A step came into view: the Windows step asks the service for the
/// tweaks, as the tweaks page does when it opens.
fn entered(ui: &AppWindow, step: u32) {
    if WINDOWS_TWEAKS && step == TWEAKS_STEP {
        ui.global::<Tweaks>().invoke_open();
    }
}

/// Shows the tour from `step`, refreshing what the stores step shows.
fn open(ui: &AppWindow, refresh: &Timer, step: u32) {
    let tour = ui.global::<Onboarding>();
    tour.set_step(step as i32);
    entered(ui, step);
    tour.set_refused("".into());
    show_keys(ui, &ShellSettings::load());
    show_steam(ui);
    tour.set_active(true);
    let weak = ui.as_weak();
    refresh.start(TimerMode::Repeated, REFRESH, move || {
        if let Some(ui) = weak.upgrade() {
            show_steam(&ui);
        }
    });
}

/// Wires the tour and shows it on a first run.
pub fn start(ui: &AppWindow) {
    let tour = ui.global::<Onboarding>();
    tour.set_tweaks_available(WINDOWS_TWEAKS);
    #[cfg(debug_assertions)]
    if let Ok(names) = std::env::var("CANAPE_MOCK_CONFLICTS") {
        // Development: off Windows the service never reports any.
        tour.set_mock_conflicts(names.into());
    }
    let refresh = Rc::new(Timer::default());

    tour.on_step_changed({
        let weak = ui.as_weak();
        move |step| {
            let step = u32::try_from(step).unwrap_or(0);
            let mut settings = ShellSettings::load();
            settings.onboarding_step = step;
            save(&settings);
            if let Some(ui) = weak.upgrade() {
                entered(&ui, step);
            }
        }
    });
    tour.on_finish({
        let (weak, refresh) = (ui.as_weak(), refresh.clone());
        move || {
            refresh.stop();
            let mut settings = ShellSettings::load();
            settings.onboarded = true;
            settings.onboarding_step = 0;
            save(&settings);
            crate::log("onboarding: done");
            let Some(ui) = weak.upgrade() else { return };
            ui.global::<Onboarding>().set_active(false);
            ui.set_page(0);
            // The library tells the gamepad itself once it has the focus.
            crate::library::set_page_arrows(false);
        }
    });
    tour.on_replay({
        let (weak, refresh) = (ui.as_weak(), refresh.clone());
        move || {
            let mut settings = ShellSettings::load();
            settings.onboarded = false;
            settings.onboarding_step = 0;
            save(&settings);
            crate::log("onboarding: shown again from Settings");
            if let Some(ui) = weak.upgrade() {
                open(&ui, &refresh, 0);
            }
        }
    });
    tour.on_save_entry({
        let weak = ui.as_weak();
        move |name, text| {
            if let (Some(ui), Some(entry)) = (weak.upgrade(), Entry::from_name(&name)) {
                save_entry(&ui, entry, Some(text.into()));
            }
        }
    });
    tour.on_paste_entry({
        let weak = ui.as_weak();
        move |name| {
            if let (Some(ui), Some(entry)) = (weak.upgrade(), Entry::from_name(&name)) {
                save_entry(&ui, entry, None);
            }
        }
    });
    tour.on_show_keyboard(crate::touchkb::show);
    tour.on_apply_tweaks({
        let weak = ui.as_weak();
        move || {
            crate::log("onboarding: applying the recommended Windows optimisations");
            if let Some(ui) = weak.upgrade() {
                ui.global::<Tweaks>().invoke_apply_recommended();
            }
        }
    });

    let settings = ShellSettings::load();
    let forced = std::env::var_os("CANAPE_ONBOARDING").is_some();
    if !settings.onboarded || forced {
        let step = if settings.onboarded {
            0
        } else {
            resume_step(settings.onboarding_step, WINDOWS_TWEAKS)
        };
        crate::log(&format!("onboarding: shown at step {step}"));
        open(ui, &refresh, step);
        // The tour moves with the arrow keys.
        ui.global::<App>().invoke_arrow_focus(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_keys_are_found_in_pasted_text() {
        let key = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(
            steam_key(key).as_deref(),
            Some("0123456789ABCDEF0123456789ABCDEF")
        );
        assert_eq!(
            steam_key(&format!("Key: {key}\r\nDomain Name: localhost")).as_deref(),
            Some("0123456789ABCDEF0123456789ABCDEF")
        );
        assert_eq!(steam_key("0123456789abcdef"), None);
        assert_eq!(steam_key("0123456789abcdefg123456789abcdef"), None);
        assert_eq!(steam_key(""), None);
    }

    #[test]
    fn twitch_values_are_one_word() {
        let id = "abcdefghij0123456789klmnopqrst";
        assert_eq!(twitch_value(&format!("  {id}\n")).as_deref(), Some(id));
        assert_eq!(twitch_value("short"), None);
        assert_eq!(twitch_value("abcdefghij 0123456789 klmnopqrst"), None);
    }

    #[test]
    fn secrets_show_their_end_only() {
        assert_eq!(masked(""), "");
        assert_eq!(masked("abc"), "••••");
        assert_eq!(masked("0123456789ABCDEF"), "•••• CDEF");
        assert!(!masked("0123456789ABCDEF").contains("0123"));
    }

    #[test]
    fn a_restart_resumes_at_a_step_that_shows() {
        assert_eq!(resume_step(0, false), 0);
        assert_eq!(resume_step(2, false), 2);
        assert_eq!(resume_step(CONFLICTS_STEP, false), STORES_STEP);
        assert_eq!(resume_step(TWEAKS_STEP, false), LAST_STEP);
        assert_eq!(resume_step(TWEAKS_STEP, true), TWEAKS_STEP);
        assert_eq!(resume_step(99, false), LAST_STEP);
    }

    #[test]
    fn entries_round_trip_their_names() {
        for entry in [Entry::SteamKey, Entry::IgdbId, Entry::IgdbSecret] {
            assert_eq!(Entry::from_name(entry.name()), Some(entry));
        }
        assert_eq!(Entry::from_name("other"), None);
    }
}
