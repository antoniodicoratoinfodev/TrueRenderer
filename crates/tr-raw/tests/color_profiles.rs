mod support;
use support::Fixture;

const D50: [f64; 3] = [0.9642956764295677, 1., 0.8251046025104602];

fn profile() -> Fixture {
    let mut f = Fixture::new(true);
    f.rational(50722, true, &[1., 0., 0., 0., 1., 0., 0., 0., 1.]);
    f.ints(50778, 3, &[17]);
    f.ints(50779, 3, &[21]);
    f.tags.remove(&50728);
    f.rational(50729, false, &[0.3457, 0.3585]);
    f
}

fn forward(f: &mut Fixture, tag: u16, red_offset: f64) {
    f.rational(
        tag,
        true,
        &[D50[0] + red_offset, 0., 0., 0., 1., 0., 0., 0., D50[2]],
    );
}

#[test]
fn invalid_forward_endpoints_must_not_cancel_at_the_selected_white() {
    let mut f = profile();
    // D50 is close to 5000 K: the first weight is ~0.234. Both endpoint
    // errors exceed the existing 0.001 allowance, while their mixture passes.
    // This is an invalid profile regardless of the currently selected white.
    forward(&mut f, 50964, 0.02);
    forward(
        &mut f,
        50965,
        -0.02 * (1. / 5000. - 1. / 6500.) / (1. / 2850. - 1. / 5000.),
    );
    let bytes = f.bytes();
    let error = tr_raw::probe(&bytes).expect_err("Invalid endpoints cancelled at D50");
    assert!(error.to_string().contains("ForwardMatrix"), "{error}");
}

#[test]
fn unused_forward_endpoint_must_be_valid_before_wb_or_export() {
    for invalid_tag in [50964, 50965] {
        let mut f = profile();
        forward(&mut f, 50964, if invalid_tag == 50964 { 0.1 } else { 0. });
        forward(&mut f, 50965, if invalid_tag == 50965 { 0.1 } else { 0. });
        // Select the valid endpoint. Changing WB must not be necessary to
        // discover the malformed calibration at the other illuminant.
        let xy = if invalid_tag == 50965 {
            [0.46, 0.41]
        } else {
            [0.30, 0.31]
        };
        f.rational(50729, false, &xy);
        let bytes = f.bytes();
        assert!(tr_raw::probe(&bytes).is_err(), "accepted tag {invalid_tag}");
        assert!(tr_raw::develop(&bytes, [1.; 3]).is_err());
        assert!(tr_raw::repack_dng(&bytes, 1 << 20).is_err());
    }
}

#[test]
fn rounded_valid_forward_endpoints_reopen_exactly_at_multiple_white_balances() {
    let mut f = profile();
    forward(&mut f, 50964, 0.0003);
    forward(&mut f, 50965, -0.0004);
    let bytes = f.bytes();
    let (_, _, repacked) = tr_raw::repack_dng(&bytes, 1 << 20).unwrap();
    assert_eq!(
        tr_raw::unpack(&bytes).unwrap(),
        tr_raw::unpack(&repacked).unwrap()
    );
    for wb in [[1.; 3], [1.15, 1., 0.85], [0.9, 1., 1.2]] {
        assert_eq!(
            tr_raw::develop(&bytes, wb).unwrap().pixels,
            tr_raw::develop(&repacked, wb).unwrap().pixels
        );
    }
}

#[test]
fn slowly_converging_valid_profile_is_usable_and_reopens_exactly() {
    let reference: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/color-matrix-reference.json")).unwrap();
    // Known solution independently generated from xy and 70-digit matrix
    // arithmetic. The old resolver rejects it; without acceleration it needs
    // 276 passes, so simply increasing the cap from 64 to 256 is insufficient.
    let c = &reference["cases"][11];
    let trial = &c["trials"][2];
    assert_eq!(trial["xy"], serde_json::json!([0.39, 0.37]));
    let mut f = profile();
    // This case deliberately reverses the illuminant order.
    f.ints(50778, 3, &[21]);
    f.ints(50779, 3, &[17]);
    f.tags.remove(&50729);
    let values = |v: &serde_json::Value| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect::<Vec<_>>()
    };
    for (tag, v) in [
        (50721, &c["cm"][0]),
        (50722, &c["cm"][1]),
        (50723, &c["cc"][0]),
        (50724, &c["cc"][1]),
        (50964, &c["fm"][0]),
        (50965, &c["fm"][1]),
    ] {
        let flat: Vec<_> = v.as_array().unwrap().iter().flat_map(&values).collect();
        f.rational(tag, true, &flat);
    }
    f.rational(50727, false, &values(&c["analog"]));
    f.rational(50728, false, &values(&trial["neutral"]));
    let bytes = f.bytes();
    tr_raw::probe(&bytes).unwrap();
    let image = tr_raw::develop(&bytes, [1.; 3]).unwrap();
    let (_, _, repacked) = tr_raw::repack_dng(&bytes, 1 << 20).unwrap();
    assert_eq!(
        tr_raw::unpack(&bytes).unwrap(),
        tr_raw::unpack(&repacked).unwrap()
    );
    assert_eq!(
        image.pixels,
        tr_raw::develop(&repacked, [1.; 3]).unwrap().pixels
    );
}
