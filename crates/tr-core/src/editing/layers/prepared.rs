//! Recipe constants, prepared once per render rather than per pixel.
//! The public scalar operators remain the independent numerical reference.
use super::{ColorRange, SampleColor, TonalLevel, hue_chroma, hue_delta, hue_rgb, luma, smooth};

pub(super) struct Range<'a> {
    source: &'a ColorRange,
    hue: f32,
    chroma: f32,
    luminance: f32,
    gray_width: f32,
    inner: f32,
}
impl<'a> Range<'a> {
    pub(super) fn new(source: &'a ColorRange) -> Self {
        let (hue, chroma) = hue_chroma(source.reference);
        Self {
            source,
            hue,
            chroma,
            luminance: luma(source.reference),
            gray_width: (chroma * 0.01).max(1e-6),
            inner: 1. - source.softness,
        }
    }
    pub(super) fn weight(&self, rgb: [f32; 3]) -> f32 {
        let (h, c) = hue_chroma(rgb);
        let falloff =
            |d: f32, width: f32| 1. - smooth((d.abs() / width - self.inner) / self.source.softness);
        let hue = if self.chroma <= 1e-6 {
            1.
        } else {
            falloff(hue_delta(h, self.hue), self.source.width[0]) * smooth(c / self.gray_width)
        };
        hue * falloff(c - self.chroma, self.source.width[1])
            * falloff(luma(rgb) - self.luminance, self.source.width[2])
    }
}

pub(super) enum Operator<'a> {
    Sample {
        source: &'a SampleColor,
        range: Range<'a>,
        gain: f32,
    },
    Light {
        gain: f32,
        gains: [f32; 3],
        saturation: f32,
    },
    Levels {
        source: &'a [TonalLevel; 4],
        gamma: [f32; 4],
        span: [f32; 4],
        output: [f32; 4],
        neutral: [bool; 4],
    },
    Other(&'a super::Operator),
}
impl<'a> Operator<'a> {
    pub(super) fn new(source: &'a super::Operator) -> Self {
        if source.is_neutral() {
            return Self::Other(source);
        }
        match source {
            super::Operator::SampleColor(s) => Self::Sample {
                source: s,
                range: Range::new(&s.range),
                gain: (s.correction[2] / 100.).exp2(),
            },
            super::Operator::Light {
                exposure,
                temperature,
                tint,
                saturation,
            } => {
                let t = temperature / 100.;
                let m = tint / 100.;
                Self::Light {
                    gain: exposure.exp2(),
                    gains: [
                        (0.18 * t + 0.07 * m).exp(),
                        (-0.14 * m).exp(),
                        (-0.18 * t + 0.07 * m).exp(),
                    ],
                    saturation: 1. + saturation / 100.,
                }
            }
            super::Operator::Levels(s) => Self::Levels {
                source: s,
                gamma: s.map(|v| 1. / v.gamma),
                span: s.map(|v| v.white - v.black),
                output: s.map(|v| v.output[1] - v.output[0]),
                neutral: s.map(|v| v == TonalLevel::default()),
            },
            _ => Self::Other(source),
        }
    }
    pub(super) fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Sample {
                source: s,
                range,
                gain,
            } => {
                let weight = range.weight(guide);
                if weight == 0. {
                    return rgb;
                }
                let (mut h, mut c) = hue_chroma(rgb);
                let (rh, rc) = (range.hue, range.chroma);
                let mut y = luma(rgb);
                if c > 1e-8 {
                    if rc > 1e-6 {
                        h += hue_delta(rh, h) * s.uniformity[0] / 100.;
                    }
                    h += s.correction[0];
                    c += (rc - c) * s.uniformity[1] / 100.;
                    c *= 1. + s.correction[1] / 100.;
                }
                y += (range.luminance - y) * s.uniformity[2] / 100.;
                let unit = hue_rgb(h);
                let uy = luma(unit);
                std::array::from_fn(|i| {
                    rgb[i] + weight * ((y + (unit[i] - uy) * c) * gain - rgb[i])
                })
            }
            Self::Light {
                gain,
                gains,
                saturation,
            } => {
                // Do not combine the gains: the canonical multiplication order
                // is part of the historical recipe's floating-point result.
                let v = std::array::from_fn(|c| rgb[c] * gain * gains[c]);
                let y = luma(v);
                v.map(|v| y + (v - y) * saturation)
            }
            Self::Levels {
                source,
                gamma,
                span,
                output,
                neutral,
            } => {
                let apply = |v: f32, i: usize| {
                    if neutral[i] {
                        return v;
                    }
                    let x = (v - source[i].black) / span[i];
                    source[i].output[0] + x.signum() * x.abs().powf(gamma[i]) * output[i]
                };
                std::array::from_fn(|i| apply(apply(rgb[i], 0), i + 1))
            }
            Self::Other(source) => source.apply(rgb, guide),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_constants_match_scalar_operators_bit_for_bit() {
        let mut random = 48271_u32;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            (random as f32 / u32::MAX as f32 - 0.2) * 4.
        };
        for reference in [[0.1, 0.2, 0.6], [0.3; 3], [-0.5, 0.2, 4.], [0.; 3]] {
            for softness in [0.01, 0.5, 1.] {
                let range = ColorRange {
                    reference,
                    softness,
                    ..Default::default()
                };
                let operators = [
                    super::super::Operator::SampleColor(SampleColor {
                        range: range.clone(),
                        correction: [-72., 35., -21.],
                        uniformity: [43., 12., 26.],
                    }),
                    super::super::Operator::SampleColor(SampleColor::default()),
                    super::super::Operator::Light {
                        exposure: -1.3,
                        temperature: 71.,
                        tint: -36.,
                        saturation: 27.,
                    },
                    super::super::Operator::Levels([
                        TonalLevel {
                            black: 0.03,
                            white: 0.9,
                            gamma: 1.12,
                            output: [0.07, 0.95],
                        },
                        TonalLevel::default(),
                        TonalLevel {
                            gamma: 0.7,
                            ..Default::default()
                        },
                        TonalLevel {
                            white: 1.2,
                            ..Default::default()
                        },
                    ]),
                ];
                let prepared: Vec<_> = operators.iter().map(Operator::new).collect();
                let prepared_range = Range::new(&range);
                for i in 0..2000 {
                    let guide = if i < 7 {
                        [
                            [0.; 3],
                            [-0.; 3],
                            [1e-10; 3],
                            [-1.; 3],
                            [4.; 3],
                            [1., 0., 0.],
                            [1000., -1000., 0.5],
                        ][i]
                    } else {
                        std::array::from_fn(|_| next())
                    };
                    assert_eq!(
                        range.weight(guide).to_bits(),
                        prepared_range.weight(guide).to_bits()
                    );
                    // A following operator sees the original layer input as its
                    // guide, not the partially modified colour.
                    let mut expected = guide;
                    let mut actual = guide;
                    for (operator, prepared) in operators.iter().zip(&prepared) {
                        expected = operator.apply(expected, guide);
                        actual = prepared.apply(actual, guide);
                        assert_eq!(expected.map(f32::to_bits), actual.map(f32::to_bits));
                    }
                }
            }
        }
    }
}
