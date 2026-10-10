//! DNG 1.7.1 chapter 6, three-channel matrix profiles. Calculations in f64.
use anyhow::{Result, bail, ensure};
pub(crate) type Matrix = [[f64; 3]; 3];
pub(crate) const ID: Matrix = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const D50: [f64; 3] = [0.9642956764295677, 1., 0.8251046025104602];
const D65: [f64; 3] = [0.9504559270516716, 1., 1.0890577507598784];
const XYZ_TO_2020: Matrix = [
    [1.716651187971268, -0.355670783776392, -0.253366281373660],
    [-0.666684351832489, 1.616481236634939, 0.015768545813911],
    [0.017639857445311, -0.042770613257809, 0.942103121235474],
];
pub(crate) fn vector(m: Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r.into_iter().zip(v).map(|(a, b)| a * b).sum())
}
pub(crate) fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
pub(crate) fn diagonal(v: [f64; 3]) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| if r == c { v[r] } else { 0. }))
}
pub(crate) fn inverse(m: Matrix) -> Result<Matrix> {
    ensure!(
        m.iter().flatten().all(|x| x.is_finite() && x.abs() <= 1e6),
        "RAW: matrice non finita/fuori scala"
    );
    let mut a = [[0.; 6]; 3];
    for r in 0..3 {
        a[r][..3].copy_from_slice(&m[r]);
        a[r][r + 3] = 1.;
    }
    for c in 0..3 {
        let pivot = (c..3)
            .max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))
            .unwrap();
        ensure!(a[pivot][c].abs() > 1e-12, "RAW: matrice singolare");
        a.swap(c, pivot);
        let scale = a[c][c];
        for x in &mut a[c] {
            *x /= scale;
        }
        for r in 0..3 {
            if r != c {
                let factor = a[r][c];
                let pivot_row = a[c];
                for (value, pivot) in a[r].iter_mut().zip(pivot_row) {
                    *value -= factor * pivot;
                }
            }
        }
    }
    let inv = std::array::from_fn(|r| std::array::from_fn(|c| a[r][c + 3]));
    let norm = |m: Matrix| {
        m.map(|r| r.into_iter().map(f64::abs).sum::<f64>())
            .into_iter()
            .fold(0., f64::max)
    };
    ensure!(
        norm(m) * norm(inv) <= 10_000.,
        "RAW: matrice mal condizionata"
    );
    Ok(inv)
}
fn mix(a: Matrix, b: Matrix, first: f64) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| first * a[r][c] + (1. - first) * b[r][c]))
}
pub(crate) fn xyz(xy: [f64; 2]) -> Result<[f64; 3]> {
    ensure!(
        xy[0].is_finite() && xy[1].is_finite() && xy[0] > 0. && xy[1] > 0. && xy[0] + xy[1] < 1.,
        "DNG: cromaticità non valida"
    );
    Ok([xy[0] / xy[1], 1., (1. - xy[0] - xy[1]) / xy[1]])
}
fn xy(v: [f64; 3]) -> Result<[f64; 2]> {
    ensure!(
        v.into_iter().all(|x| x.is_finite() && x > 0.),
        "DNG: bianco non fisico"
    );
    let sum = v.into_iter().sum::<f64>();
    Ok([v[0] / sum, v[1] / sum])
}
fn adapt(from: [f64; 3], to: [f64; 3]) -> Result<Matrix> {
    let bradford = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let a = vector(bradford, from);
    let b = vector(bradford, to);
    ensure!(
        a.into_iter().chain(b).all(|x| x > 1e-8 && x.is_finite()),
        "DNG: adattamento cromatico invalido"
    );
    Ok(multiply(
        inverse(bradford)?,
        multiply(diagonal(std::array::from_fn(|c| b[c] / a[c])), bradford),
    ))
}
pub(crate) fn illuminant(tag: u32) -> Result<f64> {
    Ok(match tag {
        1 | 4 | 9 | 20 => 5500.,
        3 | 17 => 2850.,
        10 | 21 => 6500.,
        11 | 22 => 7500.,
        18 => 4874.,
        19 => 6774.,
        23 => 5000.,
        24 => 3200.,
        _ => bail!("DNG: illuminante {tag} non qualificato"),
    })
}
// Robertson isotemperature data from Colour (BSD-3-Clause), see
// third_party/colour-robertson/LICENSE and manifest.json. No Python code imported.
const ISOTHERMS: [[f64; 4]; 31] = [
    [0., 0.18006, 0.26352, -0.24341],
    [10., 0.18066, 0.26589, -0.25479],
    [20., 0.18133, 0.26846, -0.26876],
    [30., 0.18208, 0.27119, -0.28539],
    [40., 0.18293, 0.27407, -0.30470],
    [50., 0.18388, 0.27709, -0.32675],
    [60., 0.18494, 0.28021, -0.35156],
    [70., 0.18611, 0.28342, -0.37915],
    [80., 0.18740, 0.28668, -0.40955],
    [90., 0.18880, 0.28997, -0.44278],
    [100., 0.19032, 0.29326, -0.47888],
    [125., 0.19462, 0.30141, -0.58204],
    [150., 0.19962, 0.30921, -0.70471],
    [175., 0.20525, 0.31647, -0.84901],
    [200., 0.21142, 0.32312, -1.0182],
    [225., 0.21807, 0.32909, -1.2168],
    [250., 0.22511, 0.33439, -1.4512],
    [275., 0.23247, 0.33904, -1.7298],
    [300., 0.24010, 0.34308, -2.0637],
    [325., 0.24792, 0.34655, -2.4681],
    [350., 0.25591, 0.34951, -2.9641],
    [375., 0.26400, 0.35200, -3.5814],
    [400., 0.27218, 0.35407, -4.3633],
    [425., 0.28039, 0.35577, -5.3762],
    [450., 0.28863, 0.35714, -6.7262],
    [475., 0.29685, 0.35823, -8.5955],
    [500., 0.30505, 0.35907, -11.324],
    [525., 0.31320, 0.35968, -15.628],
    [550., 0.32129, 0.36011, -23.325],
    [575., 0.32931, 0.36038, -40.770],
    [600., 0.33724, 0.36051, -116.45],
];
fn temperature(xy: [f64; 2]) -> Result<f64> {
    let denominator = -2. * xy[0] + 12. * xy[1] + 3.;
    ensure!(denominator > 0., "DNG: bianco fuori dominio CCT");
    let (u, v) = (4. * xy[0] / denominator, 6. * xy[1] / denominator);
    let distance = |r: [f64; 4]| ((v - r[2]) - r[3] * (u - r[1])) / (1. + r[3] * r[3]).sqrt();
    for pair in ISOTHERMS.windows(2) {
        let (a, b) = (distance(pair[0]), distance(pair[1]));
        if a >= 0. && b <= 0. {
            let f = if a - b > 0. { a / (a - b) } else { 0. };
            let mired = pair[0][0] + f * (pair[1][0] - pair[0][0]);
            ensure!(mired > 0., "DNG: CCT infinita");
            return Ok(1e6 / mired);
        }
    }
    bail!("DNG: bianco oltre il dominio Robertson")
}

#[derive(Clone, Debug)]
pub(crate) struct Calibration {
    pub cm: [Matrix; 2],
    pub cc: [Matrix; 2],
    pub fm: [Option<Matrix>; 2],
    pub temperatures: [f64; 2],
    pub analog: [f64; 3],
    pub neutral: [f64; 3],
}
impl Calibration {
    fn matrices(&self, point: [f64; 2]) -> Result<(Matrix, Matrix, Option<Matrix>)> {
        let [a, b] = self.temperatures;
        let weight = if a == b {
            1.
        } else {
            ((temperature(point)?.recip() - b.recip()) / (a.recip() - b.recip())).clamp(0., 1.)
        };
        let individual = multiply(diagonal(self.analog), mix(self.cc[0], self.cc[1], weight));
        let cm = multiply(individual, mix(self.cm[0], self.cm[1], weight));
        let fm = match self.fm {
            [Some(a), Some(b)] => Some(mix(a, b, weight)),
            [None, None] => None,
            _ => bail!("DNG: ForwardMatrix incompleta"),
        };
        Ok((cm, individual, fm))
    }
    pub fn neutral_from_xy(&self, point: [f64; 2]) -> Result<[f64; 3]> {
        let (cm, _, _) = self.matrices(point)?;
        let v = vector(cm, xyz(point)?);
        ensure!(
            v.into_iter().all(|x| x > 0. && x.is_finite()),
            "DNG: neutro invalido"
        );
        Ok(v.map(|x| x / v[1]))
    }
    /// Returns pre-demosaic gains and a matrix for balanced camera RGB -> Rec.2020.
    pub fn resolve(&self, relative: [f64; 3]) -> Result<([f32; 3], Matrix)> {
        ensure!(
            self.neutral
                .into_iter()
                .chain(self.analog)
                .chain(relative)
                .all(|v| v.is_finite() && v > 0. && v < 1e6),
            "DNG: WB/AnalogBalance invalidi"
        );
        let n: [f64; 3] = std::array::from_fn(|c| self.neutral[c] / self.neutral[1] / relative[c]);
        let gains = n.map(f64::recip);
        ensure!(
            gains.into_iter().all(|v| (1e-4..=1e4).contains(&v)),
            "DNG: WB fuori dominio numerico"
        );
        let mut point = [0.3457, 0.3585];
        let mut converged = false;
        for _ in 0..64 {
            let (cm, _, _) = self.matrices(point)?;
            let next = xy(vector(inverse(cm)?, n))?;
            if (next[0] - point[0]).abs() + (next[1] - point[1]).abs() < 1e-10 {
                point = next;
                converged = true;
                break;
            }
            point = [(point[0] + next[0]) * 0.5, (point[1] + next[1]) * 0.5];
        }
        ensure!(converged, "DNG: risoluzione WB non convergente");
        let (cm, individual, fm) = self.matrices(point)?;
        let camera_to_d50 = if let Some(fm) = fm {
            let reference = vector(inverse(individual)?, n);
            ensure!(
                reference.into_iter().all(|v| v > 0.),
                "DNG: neutro di riferimento non valido"
            );
            let white = vector(fm, [1.; 3]);
            ensure!(
                (0..3).all(|c| (white[c] - D50[c]).abs() < 0.001),
                "DNG: ForwardMatrix non normalizzata D50"
            );
            // Enforce the unit-neutral -> D50 definition exactly despite
            // rational-coefficient rounding in otherwise valid profiles.
            let fm = std::array::from_fn(|r| fm[r].map(|v| v * D50[r] / white[r]));
            multiply(
                fm,
                multiply(diagonal(reference.map(f64::recip)), inverse(individual)?),
            )
        } else {
            let inv = inverse(cm)?;
            let white = vector(inv, n);
            let normalized = inv.map(|r| r.map(|v| v / white[1]));
            multiply(adapt(white.map(|v| v / white[1]), D50)?, normalized)
        };
        let final_matrix = multiply(
            XYZ_TO_2020,
            multiply(adapt(D50, D65)?, multiply(camera_to_d50, diagonal(n))),
        );
        ensure!(
            final_matrix
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() < 64.),
            "DNG: trasformata fuori scala"
        );
        Ok((gains.map(|v| v as f32), final_matrix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounded_forward_matrix_keeps_the_selected_neutral_exact() {
        let calibration = Calibration {
            cm: [ID; 2],
            cc: [ID; 2],
            fm: [Some(diagonal([D50[0] + 0.0003, 0.9998, D50[2] + 0.0004])); 2],
            temperatures: [6500.; 2],
            analog: [1.; 3],
            neutral: D65,
        };
        let (gains, matrix) = calibration.resolve([1.; 3]).unwrap();
        let balanced = std::array::from_fn(|c| D65[c] * f64::from(gains[c]));
        for v in vector(matrix, balanced) {
            assert!((v - 1.).abs() < 1e-6, "neutral {v}");
        }
    }
    #[test]
    fn robertson_and_matrix_reference() {
        assert!((temperature([0.3127, 0.3290]).unwrap() - 6504.).abs() < 2.);
        let d50 = temperature([0.3457, 0.3585]).unwrap();
        assert!((d50 - 5000.).abs() < 3., "D50 CCT {d50}");
        let m = [[1., 2., 3.], [0., 1., 4.], [5., 6., 0.]];
        let result = multiply(m, inverse(m).unwrap());
        for r in 0..3 {
            for c in 0..3 {
                assert!((result[r][c] - ID[r][c]).abs() < 1e-12);
            }
        }
        assert!(inverse([[1.; 3]; 3]).is_err());
    }

    #[test]
    fn dual_illuminant_matrix_wb_and_forward_neutrality() {
        let cc = [[1., 0.015, 0.], [-0.02, 1.03, 0.], [0., 0.01, 0.97]];
        let mut calibration = Calibration {
            cm: [
                [[1.1, -0.1, 0.02], [0.05, 0.9, 0.03], [0.02, -0.1, 1.3]],
                [[0.9, 0.03, 0.01], [-0.05, 1.1, 0.02], [0.01, 0.04, 1.]],
            ],
            cc: [cc, ID],
            fm: [None, None],
            temperatures: [2850., 6500.],
            analog: [1.1, 0.95, 1.02],
            neutral: [1.; 3],
        };
        for point in [[0.44757, 0.40745], [0.3457, 0.3585], [0.3127, 0.329]] {
            calibration.neutral = calibration.neutral_from_xy(point).unwrap();
            for forward in [false, true] {
                calibration.fm = if forward {
                    [Some(diagonal(D50)); 2]
                } else {
                    [None, None]
                };
                for relative in [[1.; 3], [1.05, 1., 0.95], [0.8, 1., 1.2]] {
                    let (g, m) = calibration.resolve(relative).unwrap();
                    // The selected neutral, after its inverse gain, must map to
                    // unit Rec.2020 for every matrix family and WB interpolation.
                    let neutral: [f64; 3] = std::array::from_fn(|c| {
                        calibration.neutral[c] / calibration.neutral[1] / relative[c]
                    });
                    let balanced = std::array::from_fn(|c| neutral[c] * f64::from(g[c]));
                    let result = vector(m, balanced);
                    assert!(
                        result.into_iter().all(|v| (v - 1.).abs() < 2e-6),
                        "{result:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn xyz_camera_profile_preserves_non_neutral_xyz_colors() {
        let calibration = Calibration {
            cm: [ID; 2],
            cc: [ID; 2],
            fm: [None, None],
            temperatures: [6500.; 2],
            analog: [1.; 3],
            neutral: D65,
        };
        let (g, m) = calibration.resolve([1.; 3]).unwrap();
        for xyz in [[0.2, 0.3, 0.4], [-0.1, 0.02, 1.8], [1.3, 0.4, 0.]] {
            let balanced = std::array::from_fn(|c| xyz[c] * f64::from(g[c]));
            let actual = vector(m, balanced);
            let expected = vector(XYZ_TO_2020, xyz);
            for c in 0..3 {
                assert!(
                    (actual[c] - expected[c]).abs() < 2e-7,
                    "{actual:?} {expected:?}"
                );
            }
        }
    }
}
