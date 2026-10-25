//! Accent colour: picked in the settings page, saved in shell.json as
//! `#rrggbb`, and shared with the service so the Game Bar widget follows it.
//! The panel tint and the text colour on the accent derive from it.

use slint::{Color, ComponentHandle};

use crate::{AppWindow, Theme};

/// Accent of a fresh install.
pub const DEFAULT: &str = canape_core::settings::DEFAULT_ACCENT;

/// Accent codes saved by earlier versions, before the colour picker.
const LEGACY: &[(&str, &str)] = &[
    ("red", "#ff3d5e"),
    ("blue", "#3b82f6"),
    ("violet", "#8b5cf6"),
    ("green", "#10b981"),
    ("orange", "#f97316"),
    ("cyan", "#06b6d4"),
];

/// Text colours on the accent: whichever contrasts more.
const LIGHT_TEXT: Color = Color::from_rgb_u8(0xff, 0xff, 0xff);
const DARK_TEXT: Color = Color::from_rgb_u8(0x0b, 0x0f, 0x17);

/// `#rrggbb` for a saved value: a hex colour, or a code of an earlier
/// version; anything else gives the default.
pub fn normalize(saved: &str) -> String {
    let saved = saved.trim();
    if canape_core::protocol::is_hex_color(saved) {
        return saved.to_ascii_lowercase();
    }
    LEGACY
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(saved))
        .map_or(DEFAULT, |(_, hex)| hex)
        .to_owned()
}

pub fn parse(hex: &str) -> Option<Color> {
    if !canape_core::protocol::is_hex_color(hex) {
        return None;
    }
    let value = u32::from_str_radix(&hex[1..], 16).ok()?;
    Some(Color::from_argb_encoded(0xff00_0000 | value))
}

pub fn to_hex(color: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        color.red(),
        color.green(),
        color.blue()
    )
}

/// WCAG relative luminance.
fn luminance(color: Color) -> f32 {
    let channel = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red()) + 0.7152 * channel(color.green()) + 0.0722 * channel(color.blue())
}

fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Text colour that reads best on a filled `accent` background.
pub fn text_on(accent: Color) -> Color {
    // Biased towards white: labels on the accent are large and bold, so the
    // 3:1 white gets on mid tones (red, blue, pink) reads well and suits the
    // dark theme; light colours (orange, yellow, green, cyan) get dark text.
    if contrast(accent, LIGHT_TEXT) * 2.0 >= contrast(accent, DARK_TEXT) {
        LIGHT_TEXT
    } else {
        DARK_TEXT
    }
}

/// Shows `color` everywhere. `hsv` is what the picker should show: given
/// when it comes from the picker itself (a grey keeps its hue), derived
/// from the colour otherwise.
pub fn show(ui: &AppWindow, color: Color, hsv: Option<(f32, f32, f32)>) {
    let theme = ui.global::<Theme>();
    let (hue, saturation, value) = hsv.unwrap_or_else(|| {
        let hsva = color.to_hsva();
        (hsva.hue, hsva.saturation, hsva.value)
    });
    theme.set_accent(color);
    theme.set_accent_text(text_on(color));
    theme.set_accent_hex(to_hex(color).into());
    theme.set_accent_hue(hue.rem_euclid(360.0));
    theme.set_accent_saturation(saturation.clamp(0.0, 1.0));
    theme.set_accent_value(value.clamp(0.0, 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_values_are_normalized() {
        assert_eq!(normalize("#2563EB"), "#2563eb");
        assert_eq!(normalize("violet"), "#8b5cf6");
        assert_eq!(normalize("Green"), "#10b981");
        assert_eq!(normalize("mauve"), DEFAULT);
        assert_eq!(normalize(""), DEFAULT);
        assert_eq!(normalize("#fff"), DEFAULT);
    }

    #[test]
    fn hex_round_trips() {
        let color = parse("#10b981").unwrap();
        assert_eq!(
            (color.red(), color.green(), color.blue()),
            (0x10, 0xb9, 0x81)
        );
        assert_eq!(to_hex(color), "#10b981");
        assert!(parse("10b981").is_none());
    }

    #[test]
    fn text_contrasts_with_the_accent() {
        let on = |hex| text_on(parse(hex).unwrap());
        assert_eq!(on("#ff3d5e"), LIGHT_TEXT);
        assert_eq!(on("#3b82f6"), LIGHT_TEXT);
        assert_eq!(on("#8b5cf6"), LIGHT_TEXT);
        assert_eq!(on("#ec4899"), LIGHT_TEXT);
        assert_eq!(on("#000000"), LIGHT_TEXT);
        assert_eq!(on("#f97316"), DARK_TEXT);
        assert_eq!(on("#10b981"), DARK_TEXT);
        assert_eq!(on("#ffffff"), DARK_TEXT);
        assert_eq!(on("#facc15"), DARK_TEXT);
        assert_eq!(on("#22d3ee"), DARK_TEXT);
        assert_eq!(on("#a3e635"), DARK_TEXT);
    }
}
