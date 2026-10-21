//! Interface language: the Windows display language by default, or the one
//! picked in the settings page, saved in `%LOCALAPPDATA%\Canape\shell.json`.
//!
//! The strings themselves are translated by Slint (`@tr` in the `.slint`
//! files, catalogs in `lang/<code>/LC_MESSAGES/canape-shell.po`, compiled in
//! by build.rs). This module only decides which catalog to select.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Value of [`ShellSettings::language`] that follows the system language.
pub const AUTO: &str = "auto";

/// Bundled languages: catalog folder name. English is the source language
/// and has no catalog.
pub const LANGUAGES: &[&str] = &[
    "en", "fr", "es", "de", "it", "pt", "nl", "pl", "ru", "tr", "ja", "ko", "zh_CN",
];

/// Shell-only preferences (the hardware settings live in the service).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellSettings {
    /// [`AUTO`] or one of [`LANGUAGES`].
    pub language: String,
    /// Accent colour, `#rrggbb` (older versions saved a preset name, see
    /// [`crate::accent::normalize`]).
    pub accent: String,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            language: AUTO.into(),
            accent: crate::accent::DEFAULT.into(),
        }
    }
}

fn settings_path() -> PathBuf {
    canape_core::log::dir("LOCALAPPDATA").join("shell.json")
}

impl ShellSettings {
    /// Saved settings, or the defaults when the file is missing or unreadable.
    pub fn load() -> Self {
        std::fs::read_to_string(settings_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
}

/// Bundled language for a locale tag such as `fr-FR`, `pt_BR.UTF-8` or
/// `zh-Hans-CN`, if any.
pub fn match_locale(tag: &str) -> Option<&'static str> {
    // Drop the encoding and modifier: `de_DE.UTF-8@euro` -> `de_DE`.
    let tag = tag.split(['.', '@']).next()?.trim().to_ascii_lowercase();
    let base = tag.split(['-', '_']).next()?;
    match base {
        "" | "c" | "posix" => None,
        // Only simplified Chinese is bundled; it beats English for any reader.
        "zh" => Some("zh_CN"),
        _ => LANGUAGES.iter().copied().find(|code| *code == base),
    }
}

/// Language to show: the saved choice if it is bundled, else the first
/// preferred system language that is, else English.
pub fn resolve<S: AsRef<str>>(choice: &str, system: &[S]) -> &'static str {
    if let Some(code) = LANGUAGES.iter().copied().find(|code| *code == choice) {
        return code;
    }
    system
        .iter()
        .find_map(|tag| match_locale(tag.as_ref()))
        .unwrap_or("en")
}

/// Preferred UI languages of the user, most preferred first.
#[cfg(windows)]
pub fn system_languages() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

    let (mut count, mut len) = (0u32, 0u32);
    if unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut len,
        )
    } == 0
        || len == 0
    {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    if unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buffer.as_mut_ptr(), &mut len)
    } == 0
    {
        return Vec::new();
    }
    // A double-NUL-terminated list: "fr-FR\0en-US\0\0".
    buffer
        .split(|&c| c == 0)
        .filter(|name| !name.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// Preferred languages from the POSIX environment, most preferred first.
#[cfg(not(windows))]
pub fn system_languages() -> Vec<String> {
    let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
    let mut tags = Vec::new();
    tags.extend(var("LC_ALL"));
    tags.extend(var("LC_MESSAGES"));
    if let Some(list) = var("LANGUAGE") {
        tags.extend(list.split(':').map(str::to_owned));
    }
    tags.extend(var("LANG"));
    tags
}

/// Switches the interface to `choice` ([`AUTO`] or a language code); the
/// change shows at once. Returns the language actually selected.
pub fn apply(choice: &str) -> &'static str {
    let code = resolve(choice, &system_languages());
    // English is the source language: an empty name selects it.
    let name = if code == "en" { "" } else { code };
    if let Err(err) = slint::select_bundled_translation(name) {
        crate::log(&format!("language {code}: {err}"));
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_map_to_bundled_languages() {
        assert_eq!(match_locale("fr-FR"), Some("fr"));
        assert_eq!(match_locale("pt_BR.UTF-8"), Some("pt"));
        assert_eq!(match_locale("de_DE.UTF-8@euro"), Some("de"));
        assert_eq!(match_locale("zh-Hans-CN"), Some("zh_CN"));
        assert_eq!(match_locale("zh-TW"), Some("zh_CN"));
        assert_eq!(match_locale("JA-jp"), Some("ja"));
        assert_eq!(match_locale("en-GB"), Some("en"));
        assert_eq!(match_locale("sv-SE"), None);
        assert_eq!(match_locale("C.UTF-8"), None);
        assert_eq!(match_locale(""), None);
    }

    #[test]
    fn saved_choice_wins_over_the_system() {
        assert_eq!(resolve("ko", &["fr-FR"]), "ko");
        assert_eq!(resolve("en", &["fr-FR"]), "en");
    }

    #[test]
    fn auto_takes_the_first_bundled_system_language() {
        assert_eq!(resolve(AUTO, &["sv-SE", "nl-BE", "fr-FR"]), "nl");
        assert_eq!(resolve(AUTO, &["sv-SE"]), "en");
        assert_eq!(resolve::<&str>(AUTO, &[]), "en");
        // An unknown saved value (older or hand-edited file) behaves like auto.
        assert_eq!(resolve("xx", &["es-MX"]), "es");
    }

    #[test]
    fn settings_file_tolerates_missing_fields() {
        let settings: ShellSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, ShellSettings::default());
        let settings: ShellSettings = serde_json::from_str(r#"{"language":"ja"}"#).unwrap();
        assert_eq!(settings.language, "ja");
        assert_eq!(settings.accent, crate::accent::DEFAULT);
    }

    /// Reads the string literal at the start of `s`; returns it, unescaped,
    /// and the rest of `s`.
    fn literal(s: &str) -> Option<(String, &str)> {
        let mut chars = s.strip_prefix('"')?.char_indices();
        let mut out = String::new();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((out, &s[i + 2..])),
                '\\' => match chars.next()?.1 {
                    'n' => out.push('\n'),
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }

    type Key = (Option<String>, String);

    /// Every `@tr("…")` / `@tr("context" => "…")` of the .slint sources.
    fn source_strings() -> Vec<Key> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
        let mut keys = Vec::new();
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        for file in files
            .iter()
            .filter(|f| f.extension().is_some_and(|e| e == "slint"))
        {
            let text = std::fs::read_to_string(file).unwrap();
            for (at, _) in text.match_indices("@tr(") {
                let (first, rest) = literal(text[at + 4..].trim_start()).unwrap();
                let key = match rest.trim_start().strip_prefix("=>") {
                    Some(rest) => (Some(first), literal(rest.trim_start()).unwrap().0),
                    None => (None, first),
                };
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }

    /// Entries of a catalog written one line per field, header excluded.
    fn catalog(code: &str) -> Vec<(Key, String)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("lang")
            .join(code)
            .join("LC_MESSAGES/canape-shell.po");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let (mut entries, mut context, mut id) = (Vec::new(), None, None);
        for line in text.lines() {
            let field = |name: &str| line.strip_prefix(name).and_then(|v| literal(v.trim()));
            if let Some((v, _)) = field("msgctxt ") {
                context = Some(v);
            } else if let Some((v, _)) = field("msgid ") {
                id = Some(v);
            } else if let Some((v, _)) = field("msgstr ") {
                let id = id.take().unwrap();
                if !id.is_empty() {
                    entries.push(((context.take(), id), v));
                }
            }
        }
        entries
    }

    #[test]
    fn every_language_translates_every_string() {
        let sources = source_strings();
        assert!(sources.len() > 50, "the @tr scanner found too few strings");
        for code in LANGUAGES.iter().filter(|c| **c != "en") {
            let entries = catalog(code);
            for key in &sources {
                let (_, text) = entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .unwrap_or_else(|| panic!("{code}: missing {key:?}"));
                assert!(!text.is_empty(), "{code}: empty translation for {key:?}");
                assert_eq!(
                    text.matches("{}").count(),
                    key.1.matches("{}").count(),
                    "{code}: placeholders of {key:?}"
                );
            }
            for (key, _) in &entries {
                assert!(sources.contains(key), "{code}: stale entry {key:?}");
            }
        }
    }
}
