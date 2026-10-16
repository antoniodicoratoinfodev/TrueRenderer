//! Independent 70-digit Decimal matrix vectors (DNG 1.7.1 pp. 100–103).
//! No renderer is photographic truth; this isolates arithmetic from demosaic.
use crate::color::{self, Calibration, Matrix};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn matrix(value: &Value) -> Matrix {
    serde_json::from_value(value.clone()).unwrap()
}
fn triple(value: &Value) -> [f64; 3] {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn dng_specification_color_vectors() {
    let bytes = match std::env::var_os("TR_COLOR_VECTORS") {
        Some(path) => std::fs::read(path).unwrap(),
        None => include_bytes!("../tests/fixtures/color-matrix-reference.json").to_vec(),
    };
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(data["decimal_precision"], 70);
    let mut vectors = 0;
    let mut max_abs = 0.0f64;
    let mut max_fraction_of_tolerance = 0.0f64;
    let mut max_matrix_error = 0.0f64;
    let mut max_neutral_error = 0.0f64;
    let mut signed_outputs = 0;
    let mut extended_outputs = 0;
    let mut unchanged_calibrations = 0;
    let mut formerly_rejected = 0;
    assert_eq!(
        format!("{:x}", Sha256::digest(include_bytes!("color_cal1.rs"))),
        "4792f42736503cf239fcc9b300887e2d804bdb0f1f990e32e17fc42c5c5f9f1a"
    );
    for case in data["cases"].as_array().unwrap() {
        let calibration = Calibration {
            cm: [matrix(&case["cm"][0]), matrix(&case["cm"][1])],
            cc: [matrix(&case["cc"][0]), matrix(&case["cc"][1])],
            fm: std::array::from_fn(|i| (!case["fm"][i].is_null()).then(|| matrix(&case["fm"][i]))),
            temperatures: serde_json::from_value(case["temperatures"].clone()).unwrap(),
            analog: triple(&case["analog"]),
            neutral: triple(&case["neutral"]),
        };
        let baseline = crate::color_cal1::Calibration {
            cm: calibration.cm,
            cc: calibration.cc,
            fm: calibration.fm,
            temperatures: calibration.temperatures,
            analog: calibration.analog,
            neutral: calibration.neutral,
        };
        for trial in case["trials"].as_array().unwrap() {
            let xy = serde_json::from_value(trial["xy"].clone()).unwrap();
            let actual_neutral = calibration.neutral_from_xy(xy).unwrap();
            let expected_neutral = triple(&trial["neutral"]);
            for c in 0..3 {
                let error = (actual_neutral[c] - expected_neutral[c]).abs();
                max_neutral_error = max_neutral_error.max(error);
                assert!(error < 2e-10, "{}: neutral {error}", case["id"]);
            }
            let (gains, resolved) = calibration
                .resolve(triple(&trial["relative"]))
                .unwrap_or_else(|e| panic!("{} xy={xy:?}: {e}", case["id"]));
            match baseline.resolve(triple(&trial["relative"])) {
                Ok((old_gains, old_matrix)) => {
                    assert_eq!(gains, old_gains);
                    assert_eq!(resolved, old_matrix);
                    unchanged_calibrations += 1;
                }
                Err(e) => {
                    assert!(e.to_string().contains("non convergente"), "{e}");
                    formerly_rejected += 1;
                }
            }
            let reference = matrix(&trial["camera_to_rec2020"]);
            // Account for the public fp32 gains: their last-bit roundoff is
            // included, never removed by fitting exposure or RGB scales.
            for r in 0..3 {
                for (c, gain) in gains.iter().enumerate() {
                    let error = (resolved[r][c] * f64::from(*gain) - reference[r][c]).abs();
                    max_matrix_error = max_matrix_error.max(error);
                    assert!(error < 2e-6, "{}: matrix {error}", case["id"]);
                }
            }
            for entry in trial["vectors"].as_array().unwrap() {
                let camera = triple(&entry["camera"]).map(|v| v as f32);
                let balanced = std::array::from_fn(|c| camera[c] * gains[c]);
                let actual = color::project(resolved, balanced);
                let expected = triple(&entry["expected"]);
                for c in 0..3 {
                    let error = (f64::from(actual[c]) - expected[c]).abs();
                    let tolerance = 2e-6 + 2e-6 * expected[c].abs();
                    max_abs = max_abs.max(error);
                    max_fraction_of_tolerance = max_fraction_of_tolerance.max(error / tolerance);
                    assert!(
                        actual[c].is_finite() && error <= tolerance,
                        "{} {camera:?}: {actual:?} expected {expected:?}",
                        case["id"]
                    );
                    if expected[c] < -tolerance {
                        assert!(actual[c] < 0.);
                        signed_outputs += 1;
                    }
                    if expected[c] > 1. + tolerance {
                        assert!(actual[c] > 1.);
                        extended_outputs += 1;
                    }
                }
                vectors += 1;
            }
        }
    }
    assert!(vectors > 0 && signed_outputs > 0 && extended_outputs > 0);
    if let Some(path) = std::env::var_os("TR_COLOR_REPORT") {
        let report = json!({"passed":true, "vectors":vectors,
            "profiles":data["cases"].as_array().unwrap().len(), "seed":data["seed"],
            "reference_sha256":format!("{:x}",Sha256::digest(&bytes)),
            "color_source_sha256":format!("{:x}",Sha256::digest(include_bytes!("../src/color.rs"))),
            "max_absolute_fp32_error":max_abs, "max_fraction_of_tolerance":max_fraction_of_tolerance,
            "max_matrix_error_with_fp32_gains":max_matrix_error, "max_neutral_error":max_neutral_error,
            "negative_components_preserved":signed_outputs, "over_one_components_preserved":extended_outputs,
            "previously_accepted_calibrations_bit_exact":unchanged_calibrations,
            "previously_rejected_calibrations_now_match_reference":formerly_rejected,
            "tolerance":"2e-6 + 2e-6 * abs(reference)",
            "scope":"Matrix arithmetic only; no photographic calibration, demosaic/HL quality or general renderer superiority"});
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap() + "\n").unwrap();
    }
}
