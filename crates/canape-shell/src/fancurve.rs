//! The fan curve editor behind the Hardware page's custom curves: eight
//! points per fan, moved with the D-pad. Every change is checked with the
//! same rules the service applies ([`FanCurve::validate`]), so a curve the
//! editor shows can always be sent.

use canape_core::fan::{Fan, FanCurve, FanCurves, FanPreset, POINTS, TEMP_RANGE};
use slint::{ModelRc, VecModel};

use crate::{App, FanPointView};

/// What one Up or Down press moves.
const TEMP_STEP: i32 = 2;
const DUTY_STEP: i32 = 5;

/// Which value of the selected point Up and Down change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Temp,
    Duty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanEditor {
    pub fan: Fan,
    pub curve: FanCurve,
    pub selected: usize,
    pub field: Field,
}

impl FanEditor {
    /// Opens the editor on `fan`'s custom curve, or on the balanced preset
    /// when it has none.
    pub fn new(fan: Fan, curves: &FanCurves) -> Self {
        Self {
            fan,
            curve: curves
                .get(fan)
                .copied()
                .unwrap_or_else(|| FanPreset::Balanced.curve()),
            selected: 0,
            field: Field::Duty,
        }
    }

    /// The other fan the Ally has: CPU then GPU then back.
    pub fn next_fan(&self) -> Fan {
        match self.fan {
            Fan::Cpu => Fan::Gpu,
            _ => Fan::Cpu,
        }
    }

    pub fn select(&mut self, delta: i32) {
        self.selected = (self.selected as i32 + delta).clamp(0, POINTS as i32 - 1) as usize;
    }

    pub fn toggle_field(&mut self) {
        self.field = match self.field {
            Field::Temp => Field::Duty,
            Field::Duty => Field::Temp,
        };
    }

    /// Moves the selected value by `steps` presses (negative: down). The
    /// neighbours are pushed along so the curve stays ascending: raising a
    /// duty lifts the points after it, lowering one drops the points before
    /// it. A change the rules refuse (the safety floor, the temperature
    /// range) leaves the curve as it was and says why.
    pub fn change(&mut self, steps: i32) -> Result<(), String> {
        let mut points = self.curve.0.map(|(t, d)| (i32::from(t), i32::from(d)));
        let i = self.selected;
        match self.field {
            Field::Duty => {
                let duty = (points[i].1 + steps * DUTY_STEP).clamp(0, 100);
                points[i].1 = duty;
                for p in points.iter_mut().skip(i + 1) {
                    p.1 = p.1.max(duty);
                }
                for p in points.iter_mut().take(i) {
                    p.1 = p.1.min(duty);
                }
            }
            Field::Temp => {
                let (low, high) = (i32::from(TEMP_RANGE.0), i32::from(TEMP_RANGE.1));
                let temp = (points[i].0 + steps * TEMP_STEP).clamp(low, high);
                points[i].0 = temp;
                for j in i + 1..POINTS {
                    points[j].0 = points[j].0.max(points[j - 1].0 + 1);
                }
                for j in (0..i).rev() {
                    points[j].0 = points[j].0.min(points[j + 1].0 - 1);
                }
            }
        }
        let byte = |v: i32| u8::try_from(v).map_err(|_| "fan curve: value out of range".to_owned());
        let mut next = [(0u8, 0u8); POINTS];
        for (slot, (t, d)) in next.iter_mut().zip(points) {
            *slot = (byte(t)?, byte(d)?);
        }
        let curve = FanCurve(next);
        curve.validate()?;
        self.curve = curve;
        Ok(())
    }
}

/// Puts the editor on the page: the points, their line and the note.
pub fn show(app: &App, editor: &FanEditor, note: String) {
    let points: Vec<FanPointView> = editor
        .curve
        .0
        .iter()
        .map(|&(temp, duty)| FanPointView {
            temp: i32::from(temp),
            duty: i32::from(duty),
        })
        .collect();
    let (line, area) = crate::hardware::curve_paths(&editor.curve);
    app.set_fan_edit_points(ModelRc::new(VecModel::from(points)));
    app.set_fan_edit_line(line.into());
    app.set_fan_edit_area(area.into());
    app.set_fan_edit_fan(i32::from(editor.fan != Fan::Cpu));
    app.set_fan_edit_sel(editor.selected as i32);
    app.set_fan_edit_field(i32::from(editor.field == Field::Temp));
    app.set_fan_edit_note(note.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> FanEditor {
        FanEditor::new(Fan::Cpu, &FanCurves::default())
    }

    #[test]
    fn opens_on_the_custom_curve_or_the_balanced_preset() {
        assert_eq!(editor().curve, FanPreset::Balanced.curve());
        let mut curves = FanCurves::default();
        curves.set(Fan::Cpu, Some(FanPreset::MaxCooling.curve()));
        assert_eq!(
            FanEditor::new(Fan::Cpu, &curves).curve,
            FanPreset::MaxCooling.curve()
        );
        assert_eq!(
            FanEditor::new(Fan::Gpu, &curves).curve,
            FanPreset::Balanced.curve()
        );
    }

    #[test]
    fn raising_a_duty_lifts_the_points_after_it() {
        let mut e = editor();
        e.select(1);
        let before = e.curve.0;
        e.change(10).unwrap();
        assert!(e.curve.0[1].1 > before[1].1);
        assert!(e.curve.0.windows(2).all(|w| w[0].1 <= w[1].1));
        assert_eq!(e.curve.0[0], before[0]);
    }

    #[test]
    fn lowering_a_duty_drops_the_points_before_it() {
        let mut e = editor();
        e.select(POINTS as i32 - 1);
        e.change(-1).unwrap();
        assert!(e.curve.0.windows(2).all(|w| w[0].1 <= w[1].1));
        assert_eq!(e.curve.0[POINTS - 1].1, 70);
    }

    #[test]
    fn the_safety_floor_refuses_a_curve_that_is_too_quiet_when_hot() {
        let mut e = editor();
        e.select(POINTS as i32 - 1);
        e.field = Field::Duty;
        let before = e.curve;
        // Down until the last point would fall under the floor at 100 °C.
        let mut refused = None;
        for _ in 0..30 {
            if let Err(why) = e.change(-1) {
                refused = Some(why);
                break;
            }
        }
        assert!(refused.is_some_and(|why| why.contains("minimum")));
        assert!(e.curve.validate().is_ok());
        assert_ne!(e.curve, before);
    }

    #[test]
    fn temperatures_stay_strictly_ascending() {
        let mut e = editor();
        e.field = Field::Temp;
        e.select(3);
        for _ in 0..40 {
            let _ = e.change(1);
            assert!(e.curve.0.windows(2).all(|w| w[0].0 < w[1].0));
        }
        let mut e = editor();
        e.field = Field::Temp;
        e.select(4);
        for _ in 0..60 {
            let _ = e.change(-1);
            assert!(e.curve.0.windows(2).all(|w| w[0].0 < w[1].0));
            assert!(e.curve.0[0].0 >= TEMP_RANGE.0);
        }
    }

    #[test]
    fn selection_and_fan_switch() {
        let mut e = editor();
        e.select(-1);
        assert_eq!(e.selected, 0);
        e.select(100);
        assert_eq!(e.selected, POINTS - 1);
        assert_eq!(e.next_fan(), Fan::Gpu);
        e.toggle_field();
        assert_eq!(e.field, Field::Temp);
    }
}
