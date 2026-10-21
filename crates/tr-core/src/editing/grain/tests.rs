use super::*;
use crate::editing::{Advanced, EditRecipe, detail::Detail};

fn grain() -> Grain {
    Grain {
        enabled: true,
        ..Default::default()
    }
}
fn flat(w: u32, h: u32, alpha: f32) -> LinearImage {
    LinearImage::new(
        w,
        h,
        vec![[0.25 * alpha, 0.25 * alpha, 0.25 * alpha, alpha]; (w * h) as usize],
    )
    .unwrap()
}
fn render(g: &Grain) -> LinearImage {
    let mut image = flat(256, 192, 1.);
    g.apply_cancellable(&mut image, [256, 192], &|| false)
        .unwrap();
    image
}

#[test]
fn disabled_and_zero_controls_are_exact_identity_with_extended_values() {
    for g in [
        Grain::default(),
        Grain {
            amount: 0.,
            ..grain()
        },
        Grain {
            opacity: 0.,
            ..grain()
        },
    ] {
        let mut image = LinearImage::new(
            3,
            1,
            vec![[-0.4, 1.5, 0.3, 1.], [0.; 4], [0.1, 0.2, 0.3, 0.5]],
        )
        .unwrap();
        let before = image.pixels.clone();
        g.apply_cancellable(&mut image, [3, 1], &|| false).unwrap();
        assert_eq!(image.pixels, before);
    }
}

#[test]
fn distributions_have_zero_mean_equal_energy_and_distinct_tails() {
    for distribution in [Distribution::Uniform, Distribution::Gaussian] {
        let g = Grain {
            distribution,
            ..grain()
        };
        let values: Vec<f64> = (0..131072)
            .map(|i| g.random(i % 512, i / 512, 0) as f64)
            .collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let fourth = values.iter().map(|v| (v - mean).powi(4)).sum::<f64>()
            / values.len() as f64
            / variance.powi(2);
        assert!(mean.abs() < 0.015, "{distribution:?}: {mean}");
        assert!(
            (variance - 1.).abs() < 0.025,
            "{distribution:?}: {variance}"
        );
        let expected = if distribution == Distribution::Uniform {
            1.8
        } else {
            3.
        };
        assert!(
            (fourth - expected).abs() < 0.1,
            "{distribution:?}: {fourth}"
        );
    }
}

#[test]
fn alpha_monochrome_and_color_are_independent_of_coverage() {
    let g = Grain {
        distribution: Distribution::Gaussian,
        amount: 100.,
        ..grain()
    };
    let opaque = render(&g);
    let mut half = flat(256, 192, 0.5);
    half.pixels[0] = [0.; 4];
    g.apply_cancellable(&mut half, [256, 192], &|| false)
        .unwrap();
    assert_eq!(half.pixels[0], [0.; 4]);
    for (p, q) in opaque.pixels.iter().zip(&half.pixels).skip(1) {
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(q[3], 0.5);
        for c in 0..3 {
            assert_eq!(p[c] * 0.5, q[c]);
        }
    }
    assert!(opaque.pixels.iter().any(|p| p[0] < 0.));
    let color = render(&Grain {
        monochrome: false,
        ..g
    });
    assert!(
        color
            .pixels
            .iter()
            .filter(|p| p[0] != p[1] && p[1] != p[2])
            .count()
            > 48000
    );
}

#[test]
fn size_adds_spatial_correlation_and_opacity_scales_existing_pattern() {
    let fine = render(&grain());
    let coarse = render(&Grain {
        size: 5.5,
        ..grain()
    });
    let edge_energy = |image: &LinearImage| {
        image
            .pixels
            .chunks(256)
            .flat_map(|row| row.windows(2))
            .map(|p| (p[0][0] - p[1][0]).powi(2))
            .sum::<f32>()
    };
    assert!(edge_energy(&coarse) < edge_energy(&fine) * 0.1);
    let faded = render(&Grain {
        opacity: 25.,
        ..grain()
    });
    for (p, q) in fine.pixels.iter().zip(&faded.pixels) {
        assert!(((p[0] - 0.25) * 0.25 - (q[0] - 0.25)).abs() < 3e-8);
    }
}

#[test]
fn pattern_survives_serialization_and_thread_counts_but_changes_with_seed() {
    let g = Grain {
        distribution: Distribution::Gaussian,
        size: 2.7,
        monochrome: false,
        seed: 8192,
        ..grain()
    };
    crate::compute::configure(1).unwrap();
    let a = render(&g);
    let restored: Grain = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
    crate::compute::configure(4).unwrap();
    let b = render(&restored);
    assert_eq!(a.pixels, b.pixels);
    assert_ne!(
        a.pixels,
        render(&Grain {
            seed: g.seed + 1,
            ..g
        })
        .pixels
    );
}

#[test]
fn old_detail_json_is_byte_stable_and_invalid_grain_is_rejected() {
    let original = r#"{"texture":0.0,"clarity":0.0,"dehaze":0.0,"sharpen":0.0,"radius":1.0,"threshold":0.0,"luminance_noise":0.0,"chroma_noise":0.0}"#;
    let d: Detail = serde_json::from_str(original).unwrap();
    assert_eq!(serde_json::to_string(&d).unwrap(), original);
    assert!(!d.active());
    for g in [
        Grain {
            size: 0.,
            ..grain()
        },
        Grain {
            size: 20.1,
            ..grain()
        },
        Grain {
            amount: f32::NAN,
            ..grain()
        },
        Grain {
            amount: 101.,
            ..grain()
        },
        Grain {
            opacity: -1.,
            ..grain()
        },
    ] {
        assert!(g.validate().is_err());
    }
    let json = serde_json::to_string(&grain()).unwrap();
    assert!(serde_json::from_str::<Grain>(&json.replace("Uniform", "Future")).is_err());
}

#[test]
fn grain_is_after_color_denoise_and_geometry_and_part_of_recipe_identity() {
    let mut r = EditRecipe::neutral(Default::default());
    r.process_version = 3;
    let mut a = Advanced::default();
    a.detail.luminance_noise = 35.;
    a.detail.chroma_noise = 40.;
    a.color.monochrome = true;
    a.geometry.crop = [0.1, 0.2, 0.9, 0.8];
    a.geometry.quarter_turns = 1;
    r.advanced = Some(Box::new(a));
    let mut expected = flat(65, 43, 1.);
    r.apply(&mut expected).unwrap();
    let g = Grain {
        monochrome: false,
        size: 1.5,
        ..grain()
    };
    let dimensions = [expected.width, expected.height];
    g.apply_cancellable(&mut expected, dimensions, &|| false)
        .unwrap();
    let before = serde_json::to_string(&r).unwrap();
    r.advanced.as_mut().unwrap().detail.grain = g;
    assert_ne!(serde_json::to_string(&r).unwrap(), before);
    let mut actual = flat(65, 43, 1.);
    r.apply(&mut actual).unwrap();
    assert_eq!(actual.pixels, expected.pixels);
    assert_eq!([actual.width, actual.height], dimensions);
}

#[test]
fn provisional_mips_are_finite_at_odd_sizes_and_cancellation_is_observed() {
    let g = Grain {
        size: 2.4,
        distribution: Distribution::Gaussian,
        ..grain()
    };
    let mut image = flat(33, 22, 1.);
    g.apply_cancellable(&mut image, [65, 43], &|| false)
        .unwrap();
    assert!(image.pixels.iter().flatten().all(|v| v.is_finite()));
    use std::sync::atomic::{AtomicUsize, Ordering};
    let count = AtomicUsize::new(0);
    assert!(
        g.apply_cancellable(&mut flat(256, 192, 1.), [256, 192], &|| count
            .fetch_add(1, Ordering::Relaxed)
            > 2)
            .is_err()
    );
}
