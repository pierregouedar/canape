//! What the Hardware page shows of the fans: which preset is in use and the
//! CPU fan's curve as Path commands.

use canape_core::fan::{FanCurve, FanCurves, FanPreset, TEMP_RANGE};

/// Name of the preset `curves` matches, as the page knows it: "" when the
/// firmware drives the fans, "custom" for curves that match no preset.
pub fn preset_name(curves: &FanCurves) -> &'static str {
    if curves.is_empty() {
        return "";
    }
    FanPreset::ALL
        .into_iter()
        .find(|preset| {
            let curve = preset.curve();
            curves.iter().all(|(_, c)| c == curve)
        })
        .map_or("custom", preset_key)
}

pub fn preset_key(preset: FanPreset) -> &'static str {
    match preset {
        FanPreset::Balanced => "balanced",
        FanPreset::MaxCooling => "max-cooling",
    }
}

pub fn preset_from_key(key: &str) -> Option<FanPreset> {
    FanPreset::ALL.into_iter().find(|&p| preset_key(p) == key)
}

/// Viewbox of the curve: one unit per °C across [`TEMP_RANGE`], one per
/// percent of duty, y = 0 at 100 %.
pub const VIEWBOX_WIDTH: f32 = (TEMP_RANGE.1 - TEMP_RANGE.0) as f32;

/// The curve as a line, then as the area under it, held flat past its
/// first and last points.
pub fn curve_paths(curve: &FanCurve) -> (String, String) {
    let point = |(temp, duty): (u8, u8)| {
        let x = f32::from(temp.clamp(TEMP_RANGE.0, TEMP_RANGE.1) - TEMP_RANGE.0);
        let y = 100.0 - f32::from(duty.min(100));
        (x, y)
    };
    let points: Vec<(f32, f32)> = curve.0.iter().copied().map(point).collect();
    let (first, last) = (points[0], points[points.len() - 1]);
    let mut line = format!("M 0 {} L {} {}", first.1, first.0, first.1);
    for (x, y) in &points[1..] {
        line.push_str(&format!(" L {x} {y}"));
    }
    line.push_str(&format!(" L {VIEWBOX_WIDTH} {}", last.1));
    let area = format!("{line} L {VIEWBOX_WIDTH} 100 L 0 100 Z");
    (line, area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canape_core::fan::Fan;

    #[test]
    fn presets_are_recognised() {
        let mut curves = FanCurves::default();
        assert_eq!(preset_name(&curves), "");
        curves.set(Fan::Cpu, Some(FanPreset::Balanced.curve()));
        curves.set(Fan::Gpu, Some(FanPreset::Balanced.curve()));
        assert_eq!(preset_name(&curves), "balanced");
        curves.set(Fan::Gpu, Some(FanPreset::MaxCooling.curve()));
        assert_eq!(preset_name(&curves), "custom");
        for preset in FanPreset::ALL {
            assert_eq!(preset_from_key(preset_key(preset)), Some(preset));
        }
    }

    #[test]
    fn curve_spans_the_viewbox() {
        let (line, area) = curve_paths(&FanPreset::Balanced.curve());
        // Balanced starts at 40 °C, 10 % and ends at 92 °C, 75 %.
        assert!(line.starts_with("M 0 90 L 20 90"));
        assert!(line.ends_with("L 72 25 L 90 25"));
        assert!(area.ends_with("L 90 100 L 0 100 Z"));
    }
}
