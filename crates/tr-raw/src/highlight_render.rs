//! Explicit rendering fallback for censored highlights, after reconstruction.
//! This is a neutral rendering convention, not recovered scene colour.
use crate::{cfa::Cfa, render::Support};
type Neighbors = [[Vec<(isize, isize, f32)>; 3]; 36];

/// Quantized smooth shoulder in sensor codes, before WB. Codes beyond the
/// declared clipping level remain extended data, not inferred saturation.
pub(crate) fn confidence(code: f64, black: f64, clipping: f64) -> u8 {
    if code > clipping || clipping <= black {
        return 0;
    }
    let t = ((code - black) / (clipping - black) - 0.98) / 0.02;
    let t = t.clamp(0., 1.);
    (255. * t * t * (3. - 2. * t)).round() as u8
}

pub(crate) struct Fallback {
    confidence: Vec<u8>,
    width: usize,
    height: usize,
    cfa: Cfa,
    neighbors: Neighbors,
    active: Vec<bool>,
    columns: usize,
}

impl Fallback {
    pub(crate) fn new(
        mut confidence: Vec<u8>,
        state: &[u8],
        support: Support<'_>,
        width: usize,
        active_width: usize,
        height: usize,
        cfa: Cfa,
    ) -> Self {
        // Protect admitted reconstruction and its immediate interpolation
        // neighborhood. This mask is temporary (one byte per 6x6 cell).
        let columns = active_width.div_ceil(6);
        let rows = height.div_ceil(6);
        let mut recovered = vec![0u8; columns * rows];
        for y in 0..height {
            for x in 0..active_width {
                let c = cfa.color(x, y);
                if state[y * width + x] == crate::highlights::SATURATED
                    && !support.unsupported_colour(x, y, c)
                {
                    recovered[y / 6 * columns + x / 6] |= 1 << c;
                }
            }
        }
        for y in 0..height {
            for x in 0..width {
                let q = &mut confidence[y * width + x];
                if x >= active_width {
                    *q = 0;
                } else if *q != 0 {
                    let c = cfa.color(x, y);
                    if (y.saturating_sub(6) / 6..=((y + 6) / 6).min(rows - 1)).any(|yy| {
                        (x.saturating_sub(6) / 6..=((x + 6) / 6).min(columns - 1))
                            .any(|xx| recovered[yy * columns + xx] & (1 << c) != 0)
                    }) {
                        *q = 0;
                    }
                }
            }
        }
        if confidence.iter().all(|q| *q == 0) {
            confidence = Vec::new();
        }
        let columns = width.div_ceil(6);
        let mut active = vec![false; columns * rows];
        for (i, &q) in confidence.iter().enumerate() {
            if q == 0 {
                continue;
            }
            let (x, y) = (i % width, i / width);
            for yy in y.saturating_sub(3) / 6..=((y + 3) / 6).min(rows - 1) {
                for xx in x.saturating_sub(3) / 6..=((x + 3) / 6).min(columns - 1) {
                    active[yy * columns + xx] = true;
                }
            }
        }
        let neighbors = std::array::from_fn(|phase| {
            std::array::from_fn(|c| {
                let mut sites = vec![];
                let (x, y) = (phase % 6, phase / 6);
                let radius: isize = if cfa.period == 2 { 1 } else { 3 };
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let distance = dx * dx + dy * dy;
                        if distance > 0
                            && distance <= 10
                            && cfa.color(
                                (x as isize + dx + 6) as usize,
                                (y as isize + dy + 6) as usize,
                            ) == c
                        {
                            sites.push((dx, dy, 1. / distance as f32));
                        }
                    }
                }
                sites
            })
        });
        Self {
            confidence,
            width,
            height,
            cfa,
            neighbors,
            active,
            columns,
        }
    }

    pub(crate) fn apply(&self, mut rgb: [f32; 3], x: usize, y: usize) -> [f32; 3] {
        if self.confidence.is_empty() || !self.active[y / 6 * self.columns + x / 6] {
            return rgb;
        }
        let maximum = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
        for (c, value) in rgb.iter_mut().enumerate() {
            let confidence = if self.cfa.color(x, y) == c {
                f32::from(self.confidence[y * self.width + x]) / 255.
            } else {
                let (mut sum, mut weights) = (0_f64, 0_f64);
                for &(dx, dy, weight) in &self.neighbors[(y % 6) * 6 + x % 6][c] {
                    let (xx, yy) = (x as isize + dx, y as isize + dy);
                    if xx >= 0 && yy >= 0 && xx < self.width as isize && yy < self.height as isize {
                        sum += f64::from(weight)
                            * f64::from(self.confidence[yy as usize * self.width + xx as usize]);
                        weights += f64::from(weight);
                    }
                }
                if weights > 0. {
                    (sum / (255. * weights)) as f32
                } else {
                    0.
                }
            };
            if confidence >= 1. {
                *value = maximum;
            } else if confidence > 0. {
                *value += confidence * (maximum - *value);
            }
        }
        rgb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn without_fallback(bytes: &[u8], wb: [f64; 3]) -> tr_core::color::LinearImage {
        let raw = crate::dng::Dng::parse(bytes).unwrap();
        let mut mosaic = raw.mosaic().unwrap();
        mosaic.highlight_confidence.fill(0);
        mosaic.develop(wb).unwrap()
    }

    #[test]
    fn dng_pipeline_keeps_unclipped_colour_and_neutralizes_censored_plateaus() {
        for xtrans in [false, true] {
            for orientation in 1..=8 {
                let mut f = crate::test_support::Fixture::new(true);
                f.ints(274, 3, &[orientation]);
                if xtrans {
                    f.ints(33421, 3, &[6, 6]);
                    f.ints(33422, 1, &crate::cfa::XTRANS.map(u32::from));
                }
                for wb in [[1., 1., 1.], [1.5, 1., 0.8], [0.8, 1., 1.25]] {
                    f.pixels.fill(1024);
                    let bytes = f.bytes();
                    let image = crate::develop(&bytes, wb).unwrap();
                    for p in image.pixels {
                        let lo = p[..3].iter().copied().fold(f32::INFINITY, f32::min);
                        let hi = p[..3].iter().copied().fold(f32::NEG_INFINITY, f32::max);
                        assert!(lo > 0. && (hi - lo) / hi < 1e-6, "{p:?}");
                    }
                    for (i, p) in f.pixels.iter_mut().enumerate() {
                        *p = (100 + (i * 37) % 800) as u16;
                    }
                    let bytes = f.bytes();
                    assert_eq!(
                        crate::develop(&bytes, wb).unwrap().pixels,
                        without_fallback(&bytes, wb).pixels
                    );
                }
            }
        }
    }

    #[test]
    fn admitted_coloured_gradient_is_not_replaced_by_neutral_fallback() {
        let mut f = crate::test_support::Fixture::new(true);
        f.width = 60;
        f.height = 48;
        f.ints(256, 4, &[60]);
        f.ints(257, 4, &[48]);
        f.ints(278, 4, &[48]);
        f.ints(50717, 4, &[10000]);
        f.pixels = (0..60 * 48)
            .map(|i| {
                let c = [0, 1, 1, 2][(i / 60 % 2) * 2 + i % 2];
                ((0.6_f64 + 0.05 * (i % 60) as f64) * [0.2, 0.5, 0.12][c] * 10000.)
                    .round()
                    .min(10000.) as u16
            })
            .collect();
        let bytes = f.bytes();
        let before = without_fallback(&bytes, [1.; 3]);
        let after = crate::develop(&bytes, [1.; 3]).unwrap();
        assert_eq!(before.pixels, after.pixels);
    }

    #[test]
    fn shoulder_uses_sensor_codes_not_rgb_and_is_monotonic() {
        for black in [0., 512., 700.] {
            for clip in [4095., 16383., 65535.] {
                assert_eq!(confidence(black, black, clip), 0);
                assert_eq!(confidence(black + 0.98 * (clip - black), black, clip), 0);
                assert_eq!(confidence(clip, black, clip), 255);
                assert_eq!(confidence(clip + 1., black, clip), 0);
                let mut previous = 0;
                for i in 0..=1000 {
                    let code = black + (0.98 + 0.02 * f64::from(i) / 1000.) * (clip - black);
                    let value = confidence(code, black, clip);
                    assert!(value >= previous && value - previous <= 1);
                    previous = value;
                }
            }
        }
    }

    #[test]
    fn full_and_partial_clipping_all_cfa_phases_preserve_unaffected_channels() {
        let (w, h) = (31, 29);
        for base in [
            Cfa::bayer([0, 1, 1, 2]),
            Cfa::new(6, &crate::cfa::XTRANS).unwrap(),
        ] {
            for phase in 0..base.period * base.period {
                let cfa = base.shifted(phase % base.period, phase / base.period);
                for mask in 0..8u8 {
                    let state: Vec<_> = (0..w * h)
                        .map(|i| {
                            if mask & (1 << cfa.color(i % w, i / w)) != 0 {
                                2
                            } else {
                                0
                            }
                        })
                        .collect();
                    let q = state
                        .iter()
                        .map(|s| if *s == 2 { 255 } else { 0 })
                        .collect();
                    let cells = vec![mask; w.div_ceil(6) * h.div_ceil(6)];
                    let fallback =
                        Fallback::new(q, &state, Support::new(&cells, w.div_ceil(6)), w, w, h, cfa);
                    for y in 0..h {
                        for x in 0..w {
                            let input = [-0.1, 1.1, 2.3];
                            let output = fallback.apply(input, x, y);
                            for c in 0..3 {
                                let expected = if mask & (1 << c) != 0 { 2.3 } else { input[c] };
                                assert!(
                                    (output[c] - expected).abs() < 3e-7,
                                    "{phase} {mask} {x}/{y}: {output:?}"
                                );
                                if mask & (1 << c) == 0 {
                                    assert_eq!(output[c].to_bits(), input[c].to_bits());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn recovered_colour_and_inactive_sensor_margin_are_protected() {
        let (w, h, active) = (43usize, 31usize, 37usize);
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let state = vec![2; w * h];
        let q = vec![255; w * h];
        let cells = vec![0; active.div_ceil(6) * h.div_ceil(6)];
        let fallback = Fallback::new(
            q,
            &state,
            Support::new(&cells, active.div_ceil(6)),
            w,
            active,
            h,
            cfa,
        );
        for y in 0..h {
            for x in 0..w {
                assert_eq!(fallback.apply([3.7, 1.2, 0.7], x, y), [3.7, 1.2, 0.7]);
            }
        }
    }

    #[test]
    fn complete_clipping_stays_neutral_when_exposure_is_reduced() {
        let (w, h) = (12, 12);
        let state = vec![2; w * h];
        let cells = vec![7; 4];
        let fallback = Fallback::new(
            vec![255; w * h],
            &state,
            Support::new(&cells, 2),
            w,
            w,
            h,
            Cfa::bayer([0, 1, 1, 2]),
        );
        let rgb = fallback.apply([2.2, 1., 1.5], 5, 5);
        for ev in [-4., -2., -1., 0., 1., 2.] {
            let recipe = tr_core::editing::EditRecipe {
                exposure_ev: ev,
                ..tr_core::editing::EditRecipe::neutral(
                    tr_core::decoder::RawEngine::TrueRendererExperimental,
                )
            };
            let pixel = recipe.apply_pixel([rgb[0], rgb[1], rgb[2], 1.]);
            assert_eq!(
                pixel,
                [2.2 * ev.exp2(), 2.2 * ev.exp2(), 2.2 * ev.exp2(), 1.]
            );
        }
    }
}
