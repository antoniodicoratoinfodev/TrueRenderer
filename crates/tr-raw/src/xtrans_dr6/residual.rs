//! Equivalent implementation of the frozen DR6 median and differential update. Frozen formulas in docs/demosaic-xtrans-experimental.md.
//! No external demosaic implementation.
use crate::cfa::Cfa;

#[inline(always)]
fn order(a: f32, b: f32) -> [f32; 2] {
    if a.total_cmp(&b).is_gt() {
        [b, a]
    } else {
        [a, b]
    }
}
#[inline(always)]
fn sorted_three([a, b, c]: [f32; 3]) -> [f32; 3] {
    let [a, b] = order(a, b);
    let [b, c] = order(b, c);
    let [a, b] = order(a, b);
    [a, b, c]
}
fn median(values: &mut [f32]) -> f32 {
    if values.len() == 9 {
        let a = sorted_three([values[0], values[1], values[2]]);
        let b = sorted_three([values[3], values[4], values[5]]);
        let c = sorted_three([values[6], values[7], values[8]]);
        let low = order(order(a[0], b[0])[1], c[0])[1];
        let mid = sorted_three([a[1], b[1], c[1]])[1];
        let high = order(order(a[2], b[2])[0], c[2])[0];
        sorted_three([low, mid, high])[1]
    } else {
        values.sort_unstable_by(f32::total_cmp);
        let n = values.len();
        if n.is_multiple_of(2) {
            values[n / 2 - 1] * 0.5 + values[n / 2] * 0.5
        } else {
            values[n / 2]
        }
    }
}

fn project_missing_updates(da: f32, db: f32) -> [f32; 2] {
    let chromatic = (da - db) * 0.5 * 1.5;
    [chromatic, -chromatic]
}

#[test]
fn median_network_exhausts_binary_inputs_and_preserves_total_order() {
    // Zero-one principle checks every binary input to the comparator network.
    for bits in 0u16..512 {
        let mut v = std::array::from_fn::<_, 9, _>(|i| f32::from((bits >> i) & 1));
        assert_eq!(median(&mut v), if bits.count_ones() >= 5 { 1. } else { 0. });
    }
    let data = [-10., -0., 0., 0.01, 0.1, 0.99, 1., 1.4, 10.];
    for shift in 0..9 {
        for reverse in [false, true] {
            let mut values = std::array::from_fn::<_, 9, _>(|i| {
                data[((if reverse { 8 - i } else { i }) + shift) % 9]
            });
            let mut reference = values;
            reference.sort_unstable_by(f32::total_cmp);
            assert_eq!(median(&mut values).to_bits(), reference[4].to_bits());
        }
    }
}

pub fn reconstruct(
    mosaic: &[f32],
    width: usize,
    height: usize,
    cfa: Cfa,
    baseline: &[[f32; 3]],
) -> Vec<[f32; 3]> {
    let mut previous = baseline.to_vec();
    let mut next = vec![[0.; 3]; mosaic.len()];
    for pass in 0..3 {
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let mut red = [0.; 9];
                let mut blue = [0.; 9];
                let mut count = 0;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        let p = previous[yy * width + xx];
                        red[count] = p[0] - p[1];
                        blue[count] = p[2] - p[1];
                        count += 1;
                    }
                }
                let dr = median(&mut red[..count]);
                let db = median(&mut blue[..count]);
                let measured = cfa.color(x, y);
                let green = mosaic[i] - [dr, 0., db][measured];
                let hypothesis = [green + dr, green, green + db];
                next[i] = std::array::from_fn(|c| {
                    if c == measured {
                        mosaic[i]
                    } else {
                        previous[i][c] * 0.5 + hypothesis[c] * 0.5
                    }
                });
                if pass == 2 {
                    // Relax only the differential correction. Preserve the
                    // common camera component; this is not a calibrated
                    // colorimetric luminance model.
                    let a = (measured + 1) % 3;
                    let b = (measured + 2) % 3;
                    let delta = project_missing_updates(
                        next[i][a] - previous[i][a],
                        next[i][b] - previous[i][b],
                    );
                    next[i][a] = previous[i][a] + delta[0];
                    next[i][b] = previous[i][b] + delta[1];
                }
            }
        }
        std::mem::swap(&mut previous, &mut next);
    }
    previous
}
