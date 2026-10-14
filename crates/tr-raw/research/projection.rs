//! Matrix-stage reference generated independently with Decimal, 110 digits.
use crate::color::{Matrix, project};

#[test]
fn matrix_projection_matches_decimal_reference() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/projection-decimal-v1.json")).unwrap();
    let mut old_wrong = 0;
    let mut old_max_error = 0f64;
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 640);
    for case in cases {
        let matrix: Matrix = serde_json::from_value(case["matrix"].clone()).unwrap();
        let camera: [f32; 3] = serde_json::from_value(case["camera"].clone()).unwrap();
        let expected: [u32; 3] = serde_json::from_value(case["expected_bits"].clone()).unwrap();
        let actual = project(matrix, camera);
        assert_eq!(
            actual.map(f32::to_bits),
            expected,
            "{} {camera:?}",
            case["model"]
        );
        let old = matrix.map(|r| {
            let r = r.map(|v| v as f32);
            r[0] * camera[0] + r[1] * camera[1] + r[2] * camera[2]
        });
        for c in 0..3 {
            old_wrong += usize::from(old[c].to_bits() != expected[c]);
            old_max_error = old_max_error
                .max((f64::from(old[c]) - f64::from(f32::from_bits(expected[c]))).abs());
        }
    }
    assert!(old_wrong > 0);
    if let Ok(destination) = std::env::var("TR_PROJECTION_REPORT") {
        use std::io::Write;
        std::fs::OpenOptions::new().write(true).create_new(true).open(destination).unwrap()
            .write_all(serde_json::to_string_pretty(&serde_json::json!({
                "recipe":crate::RECIPE,"cases":cases.len(),"components":cases.len()*3,
                "reference_precision_decimal_digits":110,"old_incorrectly_rounded_components":old_wrong,
                "candidate_incorrectly_rounded_components":0,"old_maximum_absolute_error_to_rounded_reference":old_max_error,
                "candidate_maximum_absolute_error_to_rounded_reference":0,
                "limits":"Final matrix only. Demosaic and pre-matrix WB remain fp32; physical camera accuracy is not measured."
            })).unwrap().as_bytes()).unwrap();
    }
}
