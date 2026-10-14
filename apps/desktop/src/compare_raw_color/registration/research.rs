//! Opt-in geometry validation on generated fields with known correspondence.
//! No photographed scene or renderer is a colorimetric reference here.
use super::register;
use serde_json::json;

const EDGE: usize = 320;

struct Noise(u64);
impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1_u64 << 53) as f64 * 2. - 1.
    }
}

fn scene(x: f64, y: f64, variant: usize) -> f64 {
    let phase = variant as f64 * 1.731;
    0.31 * (0.071 * x + 0.137 * y + phase).sin()
        + 0.23 * (0.167 * x - 0.083 * y - phase * 0.7).cos()
        + 0.19 * (0.037 * x + 0.051 * y + phase * 0.3).sin()
        + 0.17 * (-0.193 * x + 0.113 * y - phase * 1.3).cos()
        + 0.12 * (0.00043 * x * y + 0.01 * x + phase).cos()
}

fn raster(mut f: impl FnMut(f64, f64) -> f64) -> Vec<f64> {
    (0..EDGE * EDGE)
        .map(|i| f((i % EDGE) as f64, (i / EDGE) as f64))
        .collect()
}

#[test]
#[ignore = "opt-in generated registration validation, TR_REGISTRATION_OUTPUT must be new"]
fn registration_research_campaign() {
    let output = std::env::var("TR_REGISTRATION_OUTPUT").unwrap();
    let mut rows = vec![];
    let (mut incorrect, mut rejected_clean, mut accepted) = (0, 0, 0);
    let mut noise = Noise(0x37f0_8b92_51ce_4ad6);
    for variant in [3, 7] {
        for [dx, dy] in [
            [0, 0],
            [3, -2],
            [-7, 11],
            [19, -17],
            [-23, 22],
            [21, 23],
            [-22, -19],
            [1, 1],
        ] {
            for amplitude in [0., 0.04, 0.12, 0.35] {
                for gain in [0.25, 1., 4.] {
                    let offset = gain * 0.2 - 0.35;
                    let a = raster(|x, y| scene(x, y, variant) + amplitude * noise.next());
                    let b = raster(|x, y| {
                        gain * (scene(x - f64::from(dx), y - f64::from(dy), variant)
                            + amplitude * noise.next())
                            + offset
                    });
                    let registration = register(&a, &b, EDGE, 24, 8);
                    let supported = registration["supported"] == true;
                    let exact = registration["dx"] == dx && registration["dy"] == dy;
                    accepted += usize::from(supported);
                    incorrect += usize::from(supported && !exact);
                    rejected_clean += usize::from(!supported && amplitude == 0.);
                    rows.push(json!({"kind":"translation", "variant":variant,
                        "expected":[dx,dy], "noise_amplitude":amplitude, "gain":gain,
                        "offset":offset, "exact":exact, "registration":registration}));
                }
            }
        }
    }
    let mut false_acceptances = 0;
    for variant in 11..15 {
        for kind in [
            "flat",
            "periodic",
            "unrelated",
            "inverted",
            "conflicting",
            "boundary",
        ] {
            let a = match kind {
                "flat" => vec![0.4; EDGE * EDGE],
                "periodic" => raster(|x, y| {
                    (std::f64::consts::TAU * (x % 8.) / 8.).sin()
                        + (std::f64::consts::TAU * (y % 12.) / 12.).cos()
                }),
                "unrelated" => raster(|_, _| noise.next()),
                _ => raster(|x, y| scene(x, y, variant)),
            };
            let b = match kind {
                "flat" | "periodic" => a.iter().map(|v| 1.7 * v + 0.4).collect(),
                "unrelated" => raster(|_, _| noise.next()),
                "inverted" => a.iter().map(|v| -v + 0.4).collect(),
                "conflicting" => raster(|x, y| {
                    scene(x - if x < EDGE as f64 / 2. { 9. } else { -11. }, y, variant)
                }),
                "boundary" => raster(|x, y| scene(x - 24., y + 5., variant)),
                _ => unreachable!(),
            };
            let registration = register(&a, &b, EDGE, 24, 8);
            false_acceptances += usize::from(registration["supported"] == true);
            rows.push(json!({"kind":kind, "variant":variant, "registration":registration}));
        }
    }
    let passed = incorrect == 0 && rejected_clean == 0 && false_acceptances == 0;
    let result = json!({"protocol":"TRExp-engine-color-v2", "schema":1,
        "seed":"0x37f08b9251ce4ad6", "edge":EDGE,"radius":24,"stride":8,
        "passed":passed,"translation_cases":192,"accepted_translations":accepted,
        "incorrect_accepted_translations":incorrect,"rejected_clean_translations":rejected_clean,
        "refusal_cases":24,"false_acceptances":false_acceptances,"cases":rows,
        "limits":"Integer translations of generated fields with independent uniform fine noise. No camera noise model, fractional/affine registration or physical color qualification."});
    super::super::new_json(std::path::Path::new(&output), &result).unwrap();
    assert!(
        passed,
        "incorrect={incorrect}, rejected_clean={rejected_clean}, false_acceptances={false_acceptances}"
    );
}
