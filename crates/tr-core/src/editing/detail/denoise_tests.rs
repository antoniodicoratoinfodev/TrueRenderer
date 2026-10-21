use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn noise(state: &mut u64) -> f32 {
    let mut uniform = || {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (((*state >> 40) as f64 + 0.5) / 16_777_216.) as f32
    };
    (-2. * uniform().ln()).sqrt() * (std::f32::consts::TAU * uniform()).cos()
}
fn fixture() -> (LinearImage, LinearImage) {
    let (w, h) = (256, 192);
    let mut state = 0x175a_ff76_1729;
    let mut clean = Vec::new();
    let mut noisy = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let level = if y < 64 {
                0.04
            } else if y < 128 {
                0.25
            } else {
                1.2
            };
            let structure = if x < 64 {
                0.
            } else if x < 128 {
                if x < 96 { -0.04 } else { 0.08 }
            } else {
                0.035 * (x as f32 * 0.8).sin() + 0.025 * (y as f32 * 1.3).cos()
            };
            let v = level + structure;
            let p = [v * 0.8, v, v * 1.1, 1.];
            let sigma = 0.035 * (level + 0.003_f32).sqrt();
            let n = noise(&mut state) * sigma;
            let r = noise(&mut state) * sigma * 1.4;
            let b = noise(&mut state) * sigma * 1.8;
            clean.push(p);
            noisy.push([
                p[0] + n + r,
                p[1] + n - (0.2627 * r + 0.0593 * b) / 0.678,
                p[2] + n + b,
                1.,
            ]);
        }
    }
    (
        LinearImage::new(w, h, clean).unwrap(),
        LinearImage::new(w, h, noisy).unwrap(),
    )
}
fn mse(a: &LinearImage, b: &LinearImage, x0: usize, x1: usize) -> f64 {
    let mut sum = 0.;
    let mut count = 0;
    for y in 16..a.height as usize - 16 {
        for x in x0..x1 {
            let i = y * a.width as usize + x;
            for c in 0..3 {
                sum += f64::from(a.pixels[i][c] - b.pixels[i][c]).powi(2);
                count += 1;
            }
        }
    }
    sum / count as f64
}
#[test]
fn spectral_reduces_random_noise_and_preserves_more_texture_than_legacy() {
    let (clean, noisy) = fixture();
    let d = Detail {
        luminance_noise: 80.,
        chroma_noise: 80.,
        ..Default::default()
    };
    let mut new = noisy.clone();
    d.apply(&mut new, [256, 192]);
    let mut old = noisy.clone();
    Detail {
        denoise: Denoise::Legacy,
        ..d
    }
    .apply(&mut old, [256, 192]);
    let flat = [
        mse(&noisy, &clean, 16, 48),
        mse(&old, &clean, 16, 48),
        mse(&new, &clean, 16, 48),
    ];
    let texture = [
        mse(&noisy, &clean, 144, 240),
        mse(&old, &clean, 144, 240),
        mse(&new, &clean, 144, 240),
    ];
    eprintln!("flat MSE noisy/legacy/spectral: {flat:?}; texture: {texture:?}");
    assert!(
        flat[2] < flat[0] * 0.1,
        "strong noise attenuation: {flat:?}"
    );
    assert!(
        texture[2] < texture[1] * 0.8,
        "detail compared with legacy: {texture:?}"
    );
    assert!(
        texture[2] < texture[0] * 0.5,
        "detail against ground truth: {texture:?}"
    );
}
#[test]
fn spectral_zero_flat_alpha_and_extended_values_are_preserved() {
    let (_, source) = fixture();
    let mut zero = source.clone();
    Detail::default().apply(&mut zero, [256, 192]);
    assert_eq!(zero.pixels, source.pixels);
    for size in [[1, 1], [1, 37], [37, 1], [7, 9], [37, 29], [129, 131]] {
        for p in [
            [0., 0., 0., 0.],
            [-0.1, 0.2, 1.4, 0.5],
            [0.1, 0.2, 0.3, 0.5],
        ] {
            let mut flat =
                LinearImage::new(size[0], size[1], vec![p; (size[0] * size[1]) as usize]).unwrap();
            Detail {
                luminance_noise: 100.,
                chroma_noise: 100.,
                ..Default::default()
            }
            .apply(&mut flat, size);
            assert!(flat.pixels.iter().all(|v| *v == p), "flat {size:?} {p:?}");
        }
    }
    let mut partial = source.clone();
    for (i, p) in partial.pixels.iter_mut().enumerate() {
        let alpha = if i % 257 == 0 {
            0.
        } else if i % 7 == 0 {
            0.25
        } else {
            0.75
        };
        for v in &mut p[..3] {
            *v *= alpha;
        }
        p[3] = alpha;
    }
    let before = partial.clone();
    Detail {
        luminance_noise: 90.,
        chroma_noise: 90.,
        ..Default::default()
    }
    .apply(&mut partial, [256, 192]);
    for (p, q) in partial.pixels.iter().zip(before.pixels) {
        assert_eq!(p[3], q[3]);
        assert!(p.iter().all(|v| v.is_finite()));
        if q[3] == 0. {
            assert_eq!(*p, q);
        }
    }
    assert!(partial.pixels.iter().any(|p| p[2] > p[3]));
}
#[test]
fn chroma_and_luminance_controls_are_independent() {
    let (_, source) = fixture();
    for (luminance, chroma) in [(0., 100.), (100., 0.)] {
        let mut image = source.clone();
        Detail {
            luminance_noise: luminance,
            chroma_noise: chroma,
            ..Default::default()
        }
        .apply(&mut image, [256, 192]);
        for (p, q) in image.pixels.iter().zip(&source.pixels) {
            let y = luma(straight(*p));
            let by = luma(straight(*q));
            if luminance == 0. {
                assert!((y - by).abs() < 5e-7);
            } else {
                for c in 0..3 {
                    assert!(((p[c] - y) - (q[c] - by)).abs() < 5e-7);
                }
            }
        }
    }
}
#[test]
fn spectral_cancellation_is_transactional_and_checked_inside_tiles() {
    let (_, source) = fixture();
    let mut image = source.clone();
    let calls = AtomicUsize::new(0);
    let result = Detail {
        luminance_noise: 80.,
        ..Default::default()
    }
    .apply_cancellable(&mut image, [256, 192], &|| {
        calls.fetch_add(1, Ordering::Relaxed) > 25
    });
    assert!(result.is_err());
    assert_eq!(image.pixels, source.pixels);
}
#[test]
fn historical_serialization_digests_and_pixels_keep_the_old_filter() {
    let legacy = r#"{"texture":0.0,"clarity":0.0,"dehaze":0.0,"sharpen":0.0,"radius":1.0,"threshold":0.0,"luminance_noise":80.0,"chroma_noise":80.0}"#;
    let d: Detail = serde_json::from_str(legacy).unwrap();
    assert_eq!(d.denoise, Denoise::Legacy);
    assert_eq!(serde_json::to_string(&d).unwrap(), legacy);
    let mut recipe = crate::editing::EditRecipe::neutral(crate::decoder::RawEngine::TrueRenderer);
    recipe.process_version = 3;
    recipe.advanced = Some(Box::default());
    recipe.advanced.as_mut().unwrap().detail = d.clone();
    let digest = serde_json::to_vec(&recipe).unwrap();
    recipe.advanced.as_mut().unwrap().detail.denoise = Denoise::SpectralV1;
    assert_ne!(digest, serde_json::to_vec(&recipe).unwrap());
    let mut a = fixture().1;
    let mut b = a.clone();
    d.apply(&mut a, [256, 192]);
    Detail {
        denoise: Denoise::Legacy,
        ..d
    }
    .apply(&mut b, [256, 192]);
    assert_eq!(a.pixels, b.pixels);
    assert!(
        serde_json::from_str::<Detail>(
            &legacy.replace("\"texture\"", "\"denoise\":\"Unknown\",\"texture\"")
        )
        .is_err()
    );
}

#[test]
fn clean_edges_ramps_and_periodic_texture_are_not_mistaken_for_noise() {
    for pattern in 0..3 {
        let source = LinearImage::new(
            256,
            192,
            (0..256 * 192)
                .map(|i| {
                    let x = (i % 256) as f32;
                    let y = (i / 256) as f32;
                    let v = match pattern {
                        0 => {
                            if x < 128. {
                                -0.05
                            } else {
                                1.2
                            }
                        }
                        1 => -0.1 + x / 256. * 1.5 + y / 192. * 0.1,
                        _ => 0.2 + 0.05 * (x * 0.8).sin() + 0.04 * (y * 1.3).cos(),
                    };
                    [v, v, v, 1.]
                })
                .collect(),
        )
        .unwrap();
        let mut result = source.clone();
        Detail {
            luminance_noise: 100.,
            chroma_noise: 100.,
            ..Default::default()
        }
        .apply(&mut result, [256, 192]);
        let maximum = result
            .pixels
            .iter()
            .zip(&source.pixels)
            .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
            .fold(0_f32, f32::max);
        assert!(maximum < 1e-5, "clean pattern {pattern}: {maximum}");
    }
}

#[test]
fn repeated_runs_and_provisional_scales_remain_finite_and_deterministic() {
    let (_, source) = fixture();
    let d = Detail {
        luminance_noise: 80.,
        chroma_noise: 80.,
        ..Default::default()
    };
    let mut a = source.clone();
    d.apply(&mut a, [256, 192]);
    let mut b = source.clone();
    d.apply(&mut b, [256, 192]);
    assert_eq!(a.pixels, b.pixels);
    for native in [[512, 384], [1024, 768]] {
        let mut image = source.clone();
        d.apply(&mut image, native);
        assert!(image.pixels.iter().flatten().all(|v| v.is_finite()));
        assert!(mse(&image, &fixture().0, 16, 48) < mse(&source, &fixture().0, 16, 48));
    }
}
