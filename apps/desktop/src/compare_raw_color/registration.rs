//! Symmetric two-scale integer registration. Filtering is geometry-only;
//! the caller keeps original linear RGB for all color statistics.
use super::MARGIN;
use serde_json::{Value, json};

#[cfg(test)]
mod research;

fn search(
    a: &[f64],
    b: &[f64],
    edge: usize,
    radius: i32,
    stride: usize,
    minimum_cosine: f64,
) -> Value {
    let margin = MARGIN.max(radius as usize + 2);
    let mut points = vec![];
    let mut energy = 0.;
    for y in (margin..edge - margin).step_by(stride) {
        for x in (margin..edge - margin).step_by(stride) {
            let i = y * edge + x;
            let gradient = [a[i + 1] - a[i - 1], a[i + edge] - a[i - edge]];
            energy += gradient[0] * gradient[0] + gradient[1] * gradient[1];
            points.push((i, gradient));
        }
    }
    if energy < 1e-12 {
        return json!({"supported":false,"reason":"insufficient gradient energy","dx":0,"dy":0});
    }
    let mut scores = vec![];
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let offset = dy as isize * edge as isize + dx as isize;
            let (mut dot, mut norm) = (0., 0.);
            for &(i, g) in &points {
                let i = (i as isize + offset) as usize;
                let h = [b[i + 1] - b[i - 1], b[i + edge] - b[i - edge]];
                dot += g[0] * h[0] + g[1] * h[1];
                norm += h[0] * h[0] + h[1] * h[1];
            }
            let score = if norm > 1e-12 {
                dot / (energy * norm).sqrt()
            } else {
                -1.
            };
            scores.push((score, dx, dy));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, dx, dy) = scores[0];
    let runner = scores
        .iter()
        .find(|&&(_, x, y)| (x - dx).abs() > 2 || (y - dy).abs() > 2)
        .unwrap()
        .0;
    let boundary = dx.abs().max(dy.abs()) == radius;
    json!({"dx":dx,"dy":dy,"gradient_cosine":best,"distant_runner_up_cosine":runner,
        "peak_gap":best-runner,"at_search_boundary":boundary,
        "supported":best >= minimum_cosine && best-runner >= 0.002 && !boundary})
}

fn smooth(input: &[f64], edge: usize, kernel: &[u32]) -> Vec<f64> {
    let radius = kernel.len() / 2;
    let divisor = f64::from(kernel.iter().sum::<u32>());
    let mut horizontal = vec![0.; input.len()];
    for y in 0..edge {
        for x in 0..edge {
            horizontal[y * edge + x] = kernel
                .iter()
                .enumerate()
                .map(|(k, &w)| {
                    let xx = (x + k).saturating_sub(radius).min(edge - 1);
                    f64::from(w) * input[y * edge + xx]
                })
                .sum::<f64>()
                / divisor;
        }
    }
    let mut output = vec![0.; input.len()];
    for y in 0..edge {
        for x in 0..edge {
            output[y * edge + x] = kernel
                .iter()
                .enumerate()
                .map(|(k, &w)| {
                    let yy = (y + k).saturating_sub(radius).min(edge - 1);
                    f64::from(w) * horizontal[yy * edge + x]
                })
                .sum::<f64>()
                / divisor;
        }
    }
    output
}

pub(super) fn register(a: &[f64], b: &[f64], edge: usize, radius: i32, stride: usize) -> Value {
    let fine = search(a, b, edge, radius, stride, 0.85);
    let mut scales = Vec::new();
    for kernel in [&[1, 4, 6, 4, 1][..], &[1, 8, 28, 56, 70, 56, 28, 8, 1][..]] {
        let aa = smooth(a, edge, kernel);
        let bb = smooth(b, edge, kernel);
        scales.push(json!({"radius":kernel.len()/2,
            "forward":search(&aa, &bb, edge, radius, stride, 0.90),
            "reverse":search(&bb, &aa, edge, radius, stride, 0.90)}));
    }
    let dx = scales[0]["forward"]["dx"].as_i64().unwrap();
    let dy = scales[0]["forward"]["dy"].as_i64().unwrap();
    let supported = scales.iter().all(|s| {
        s["forward"]["supported"] == true
            && s["reverse"]["supported"] == true
            && s["forward"]["dx"] == dx
            && s["forward"]["dy"] == dy
            && s["reverse"]["dx"] == -dx
            && s["reverse"]["dy"] == -dy
    });
    let minimum_cosine = scales
        .iter()
        .flat_map(|s| [&s["forward"], &s["reverse"]])
        .map(|s| s["gradient_cosine"].as_f64())
        .collect::<Option<Vec<_>>>()
        .map(|v| v.into_iter().fold(f64::INFINITY, f64::min));
    json!({"method":"symmetric-binomial-v2", "dx":dx,"dy":dy,"supported":supported,
        "minimum_gradient_cosine":minimum_cosine,"scales":scales,"fine":fine})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_filters_preserve_dc_and_affine_interiors() {
        let edge = 32;
        for kernel in [&[1, 4, 6, 4, 1][..], &[1, 8, 28, 56, 70, 56, 28, 8, 1][..]] {
            let flat = vec![0.375; edge * edge];
            assert_eq!(smooth(&flat, edge, kernel), flat);
            let plane: Vec<_> = (0..edge * edge)
                .map(|i| 0.125 * (i % edge) as f64 - 0.25 * (i / edge) as f64 + 0.375)
                .collect();
            let output = smooth(&plane, edge, kernel);
            let radius = kernel.len() / 2;
            for y in radius..edge - radius {
                for x in radius..edge - radius {
                    assert_eq!(output[y * edge + x], plane[y * edge + x]);
                }
            }
        }
    }

    #[test]
    fn ambiguous_periodic_and_flat_fields_cannot_be_registered() {
        let edge = 160;
        let flat = vec![0.5; edge * edge];
        let stripes: Vec<_> = (0..edge * edge)
            .map(|i| ((i % edge % 8) as f64 * std::f64::consts::TAU / 8.).sin())
            .collect();
        for (a, b) in [
            (&flat, &flat),
            (&stripes, &stripes),
            (&flat, &stripes),
            (&stripes, &flat),
        ] {
            assert_eq!(register(a, b, edge, 12, 4)["supported"], false);
        }
    }
}
