//! Inline durable vector masks. Source-space coordinates survive crop/rotation;
//! no external files or caches are needed to restore a stroke.
use super::detail::luma;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    #[default]
    Radial,
    Linear,
    Brush,
    Luminance,
    Hue,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    pub enabled: bool,
    pub shape: Shape,
    pub center: [f32; 2],
    pub radius: f32,
    pub feather: f32,
    pub angle: f32,
    pub invert: bool,
    pub interval: [f32; 2],
    pub points: Vec<[f32; 2]>,
    pub breaks: Vec<usize>,
    pub exposure: f32,
    pub warmth: f32,
    pub saturation: f32,
}
impl Default for Mask {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: Shape::Radial,
            center: [0.5; 2],
            radius: 0.25,
            feather: 0.5,
            angle: 0.,
            invert: false,
            interval: [0.25, 0.75],
            points: vec![],
            breaks: vec![],
            exposure: 0.,
            warmth: 0.,
            saturation: 0.,
        }
    }
}
impl Mask {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.breaks.iter().all(|i| *i > 0 && *i < self.points.len())
                && self.breaks.windows(2).all(|w| w[0] < w[1]),
            "Tratti pennello non validi"
        );
        for p in std::iter::once(&self.center).chain(&self.points) {
            for v in p {
                super::range(*v, 0., 1., "Coordinate maschera")?;
            }
        }
        super::range(self.radius, 0.005, 1., "Raggio maschera")?;
        super::range(self.feather, 0.01, 1., "Sfumatura")?;
        super::range(self.angle, -180., 180., "Angolo maschera")?;
        for v in self.interval {
            super::range(v, 0., 1., "Intervallo maschera")?;
        }
        ensure!(
            self.interval[0] < self.interval[1],
            "Intervallo maschera invertito"
        );
        super::range(self.exposure, -5., 5., "Esposizione locale")?;
        super::range(self.warmth, -100., 100., "Temperatura locale")?;
        super::range(self.saturation, -100., 100., "Saturazione locale")
    }
    pub fn active(&self) -> bool {
        self.enabled && (self.exposure != 0. || self.warmth != 0. || self.saturation != 0.)
    }
    pub fn weight(&self, p: [f32; 2], guide: [f32; 3], aspect: f32) -> f32 {
        let distance = |point: [f32; 2]| {
            ((p[0] - point[0]).powi(2) + (aspect * (p[1] - point[1])).powi(2)).sqrt()
        };
        let smooth = |t: f32| {
            let t = t.clamp(0., 1.);
            t * t * (3. - 2. * t)
        };
        let radial = |d: f32| 1. - smooth((d / self.radius - (1. - self.feather)) / self.feather);
        let weight = match self.shape {
            Shape::Radial => radial(distance(self.center)),
            Shape::Linear => {
                let (sin, cos) = self.angle.to_radians().sin_cos();
                let d = (p[0] - self.center[0]) * cos + (p[1] - self.center[1]) * aspect * sin;
                smooth(0.5 + d / (2. * self.radius))
            }
            Shape::Brush => {
                let mut d = self
                    .points
                    .iter()
                    .map(|q| distance(*q))
                    .fold(f32::INFINITY, f32::min);
                for (index, pair) in self.points.windows(2).enumerate() {
                    if self.breaks.contains(&(index + 1)) {
                        continue;
                    }
                    let a = [pair[0][0], pair[0][1] * aspect];
                    let b = [pair[1][0], pair[1][1] * aspect];
                    let v = [b[0] - a[0], b[1] - a[1]];
                    let len = v[0] * v[0] + v[1] * v[1];
                    if len > 1e-12 {
                        let t = (((p[0] - a[0]) * v[0] + (p[1] * aspect - a[1]) * v[1]) / len)
                            .clamp(0., 1.);
                        d = d.min(
                            ((p[0] - a[0] - t * v[0]).powi(2)
                                + (p[1] * aspect - a[1] - t * v[1]).powi(2))
                            .sqrt(),
                        );
                    }
                }
                radial(d)
            }
            Shape::Luminance | Shape::Hue => {
                let v = if self.shape == Shape::Luminance {
                    luma(guide)
                } else {
                    super::color::hue_chroma(guide).0 / 360.
                };
                let edge = (self.interval[1] - self.interval[0]) * self.feather * 0.5;
                smooth((v - self.interval[0]) / edge) * smooth((self.interval[1] - v) / edge)
            }
        };
        if self.invert { 1. - weight } else { weight }
    }
    pub fn apply(&self, rgb: &mut [f32; 3], guide: [f32; 3], p: [f32; 2], aspect: f32) {
        if !self.active() {
            return;
        }
        let weight = self.weight(p, guide, aspect);
        self.apply_weight(rgb, weight);
    }
    fn apply_weight(&self, rgb: &mut [f32; 3], weight: f32) {
        let exposure = self.exposure.exp2();
        let gains = [
            (self.warmth * 0.0018).exp(),
            1.,
            (-self.warmth * 0.0018).exp(),
        ];
        let mut edited = std::array::from_fn(|c| rgb[c] * exposure * gains[c]);
        let y = luma(edited);
        for c in 0..3 {
            edited[c] = y + (edited[c] - y) * (1. + self.saturation / 100.);
            rgb[c] += weight * (edited[c] - rgb[c]);
        }
    }
}

/// A bounded segment tree avoids scanning every brush segment for every native
/// pixel. Purely derived scratch; the durable representation stays the vectors.
pub(super) struct Prepared<'a> {
    mask: &'a Mask,
    brush: Option<Node>,
    aspect: f32,
}
impl<'a> Prepared<'a> {
    pub fn new(mask: &'a Mask, aspect: f32) -> Self {
        let brush = if mask.active() && mask.shape == Shape::Brush && !mask.points.is_empty() {
            let point = |p: [f32; 2]| [p[0], p[1] * aspect];
            let mut segments: Vec<_> = mask.points.iter().map(|p| [point(*p); 2]).collect();
            for (i, pair) in mask.points.windows(2).enumerate() {
                if !mask.breaks.contains(&(i + 1)) {
                    segments.push([point(pair[0]), point(pair[1])]);
                }
            }
            Some(Node::new(segments))
        } else {
            None
        };
        Self {
            mask,
            brush,
            aspect,
        }
    }
    pub fn apply(&self, rgb: &mut [f32; 3], guide: [f32; 3], p: [f32; 2]) {
        if !self.mask.active() {
            return;
        }
        if let Some(brush) = &self.brush {
            let mask = self.mask;
            let inner = mask.radius * (1. - mask.feather);
            let d = brush
                .nearest(
                    [p[0], p[1] * self.aspect],
                    mask.radius.powi(2),
                    inner.powi(2),
                )
                .sqrt();
            let t = ((d / mask.radius - (1. - mask.feather)) / mask.feather).clamp(0., 1.);
            let weight = 1. - t * t * (3. - 2. * t);
            mask.apply_weight(rgb, if mask.invert { 1. - weight } else { weight });
        } else {
            self.mask.apply(rgb, guide, p, self.aspect);
        }
    }
}
struct Node {
    bounds: [f32; 4],
    segments: Vec<[[f32; 2]; 2]>,
    children: Option<Box<[Node; 2]>>,
}
impl Node {
    fn new(mut segments: Vec<[[f32; 2]; 2]>) -> Self {
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for point in segments.iter().flatten() {
            for axis in 0..2 {
                bounds[axis] = bounds[axis].min(point[axis]);
                bounds[axis + 2] = bounds[axis + 2].max(point[axis]);
            }
        }
        let children = if segments.len() > 8 {
            let axis = usize::from(bounds[3] - bounds[1] > bounds[2] - bounds[0]);
            segments.sort_unstable_by(|a, b| {
                (a[0][axis] + a[1][axis]).total_cmp(&(b[0][axis] + b[1][axis]))
            });
            let right = segments.split_off(segments.len() / 2);
            Some(Box::new([
                Self::new(std::mem::take(&mut segments)),
                Self::new(right),
            ]))
        } else {
            None
        };
        Self {
            bounds,
            segments,
            children,
        }
    }
    fn distance(&self, p: [f32; 2]) -> f32 {
        (0..2)
            .map(|c| (p[c] - p[c].clamp(self.bounds[c], self.bounds[c + 2])).powi(2))
            .sum()
    }
    fn nearest(&self, p: [f32; 2], mut best: f32, inner: f32) -> f32 {
        if best <= inner || self.distance(p) >= best {
            return best;
        }
        if let Some(children) = &self.children {
            let first = usize::from(children[1].distance(p) < children[0].distance(p));
            best = children[first].nearest(p, best, inner);
            children[1 - first].nearest(p, best, inner)
        } else {
            for [a, b] in &self.segments {
                let v = [b[0] - a[0], b[1] - a[1]];
                let len = v[0] * v[0] + v[1] * v[1];
                let t = if len > 1e-12 {
                    (((p[0] - a[0]) * v[0] + (p[1] - a[1]) * v[1]) / len).clamp(0., 1.)
                } else {
                    0.
                };
                best =
                    best.min((p[0] - a[0] - v[0] * t).powi(2) + (p[1] - a[1] - v[1] * t).powi(2));
                if best <= inner {
                    break;
                }
            }
            best
        }
    }
}
