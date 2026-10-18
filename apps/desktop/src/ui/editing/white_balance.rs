//! UI coordinates for native sensor WB; stored gains and decoder semantics stay unchanged.
use super::{Language, adjustment_default, egui};
use tr_core::decoder::{RawEngine, RawWhiteBalance};

// A relative, invertible log-gain basis, not a camera illuminant/Kelvin model.
// Positive temperature raises R/B; positive tint raises R and B relative to G.
fn coordinates(wb: RawWhiteBalance) -> [f64; 2] {
    let [r, _, b] = wb.gains_f64().map(f64::ln);
    let scale = 50. / 4_f64.ln();
    [(r - b) * scale, (r + b) * scale]
}

fn from_coordinates(
    engine: RawEngine,
    [temperature, tint]: [f64; 2],
) -> anyhow::Result<RawWhiteBalance> {
    anyhow::ensure!(
        temperature.is_finite()
            && tint.is_finite()
            && temperature.abs() + tint.abs() <= 100. + 1e-9,
        "Relative WB outside its UI domain"
    );
    // The UI restricts |temperature| + |tint| <= 100. Clamp only fp roundoff.
    let red = 4_f64.powf((tint + temperature) / 100.).clamp(0.25, 4.);
    let blue = 4_f64.powf((tint - temperature) / 100.).clamp(0.25, 4.);
    if engine == RawEngine::TrueRendererExperimental {
        RawWhiteBalance::precise_sensor_gains([red, 1., blue])
    } else {
        Ok(RawWhiteBalance {
            red: (red * 1000.).round() as u16,
            blue: (blue * 1000.).round() as u16,
            ..Default::default()
        })
    }
}

pub(super) fn sensor_wb_controls(
    ui: &mut egui::Ui,
    lang: Language,
    engine: RawEngine,
    wb: &mut RawWhiteBalance,
) -> (bool, bool) {
    let (mut changed, mut commit) = (false, false);
    for (axis, label, hint) in [
        (
            0,
            "Temperatura",
            "Freddo ↔ caldo · scala relativa, non Kelvin",
        ),
        (1, "Tinta", "Verde ↔ magenta · scala relativa"),
    ] {
        let mut pair = coordinates(*wb);
        // Keep the other coordinate fixed, including at the old gain limits.
        // Display rounding must never rewrite an existing/Auto/picker recipe.
        let limit = ((100. - pair[1 - axis].abs()).max(0.) * 100.).floor() / 100.;
        let initial = ((pair[axis] * 100.).round() / 100.).clamp(-limit, limit);
        let mut value = initial;
        let (response, reset) = adjustment_default(
            ui,
            lang,
            lang.text(label),
            &mut value,
            -limit..=limit,
            "",
            Some(0.01),
            0.,
        );
        let response = response.on_hover_text(format!(
            "{}\n{}", lang.text(hint),
            lang.text("La corsa disponibile dipende dall’altro controllo per rispettare i limiti del WB RAW."),
        ));
        let mut value_changed = false;
        if reset || (response.changed() && (value - initial).abs() > 1e-9) {
            pair[axis] = value;
            let Ok(next) = from_coordinates(engine, pair) else {
                continue;
            };
            value_changed = next != *wb;
            if value_changed {
                *wb = next;
            }
            changed |= value_changed;
        }
        commit |= response.drag_stopped() || (value_changed && !response.dragged());
    }
    (changed, commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_axes_are_invertible_for_all_stored_gain_domains() {
        for engine in [
            RawEngine::LibRawBilinear,
            RawEngine::LibRawAhd,
            RawEngine::TrueRenderer,
            RawEngine::TrueRendererExperimental,
        ] {
            for r in [250, 251, 800, 1000, 1333, 3999, 4000] {
                for b in [250, 251, 800, 1000, 1333, 3999, 4000] {
                    let wb = RawWhiteBalance {
                        red: r,
                        blue: b,
                        ..Default::default()
                    };
                    let pair = coordinates(wb);
                    assert!(pair[0].abs() + pair[1].abs() <= 100. + 1e-10);
                    let back = from_coordinates(engine, pair).unwrap();
                    assert_eq!((back.red, back.blue), (r, b));
                    back.validate_for(engine).unwrap();
                }
            }
        }
        let wb = RawWhiteBalance::precise_sensor_gains([1.333317, 1., 0.800273]).unwrap();
        assert_eq!(
            from_coordinates(RawEngine::TrueRendererExperimental, coordinates(wb)).unwrap(),
            wb
        );
    }

    #[test]
    fn relative_axes_have_photographic_directions_and_neutral_reset() {
        let engine = RawEngine::TrueRendererExperimental;
        let warm = from_coordinates(engine, [25., 0.]).unwrap().gains_f64();
        assert!(warm[0] > 1. && warm[2] < 1.);
        let magenta = from_coordinates(engine, [0., 25.]).unwrap().gains_f64();
        assert!(magenta[0] > 1. && magenta[0] == magenta[2]);
        let green = from_coordinates(engine, [0., -25.]).unwrap().gains_f64();
        assert!(green[0] < 1. && green[0] == green[2]);
        assert!(from_coordinates(engine, [0., 0.]).unwrap().is_as_shot());
        for pair in [[f64::NAN, 0.], [0., f64::INFINITY], [75., 75.]] {
            assert!(from_coordinates(engine, pair).is_err());
        }
    }
}
