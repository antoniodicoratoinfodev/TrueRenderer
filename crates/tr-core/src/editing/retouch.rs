//! Durable source-space strokes. The decoded RAW is read-only; rendering replays
//! the recipe against its current development. No raster asset is serialized.
use crate::{
    color::LinearImage,
    document::{Brush, ColorSpace, Document, RasterLayer, SelectionMode, Unit},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_STROKES: usize = 128;
pub const MAX_POINTS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Paint,
    Erase,
    Clone,
    Heal,
    AutoHeal,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Paint => "Pennello",
            Self::Erase => "Gomma",
            Self::Clone => "Clona",
            Self::Heal => "Ripara",
            Self::AutoHeal => "Ripara rapido",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub points: Vec<[f32; 2]>,
    pub mode: SelectionMode,
    /// Fraction of oriented source width.
    pub feather: f32,
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (3..=256).contains(&self.points.len()),
            "Selezione: da 3 a 256 punti"
        );
        ensure!(
            self.points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "Coordinate selezione non valide"
        );
        ensure!(
            self.feather.is_finite() && (0. ..=0.25).contains(&self.feather),
            "Sfumatura non valida"
        );
        Ok(())
    }
    pub fn weight(&self, p: [f32; 2], aspect: f32) -> f32 {
        polygon_weight(&self.points, p, aspect, self.feather)
    }
}
pub fn polygon_weight(points: &[[f32; 2]], p: [f32; 2], aspect: f32, feather: f32) -> f32 {
    let mut inside = false;
    let mut distance = f32::INFINITY;
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        let d = [b[0] - a[0], (b[1] - a[1]) * aspect];
        let q = [p[0] - a[0], (p[1] - a[1]) * aspect];
        let len = d[0] * d[0] + d[1] * d[1];
        let t = if len > 0. {
            ((q[0] * d[0] + q[1] * d[1]) / len).clamp(0., 1.)
        } else {
            0.
        };
        distance = distance.min((q[0] - t * d[0]).hypot(q[1] - t * d[1]));
    }
    if points.len() < 3 {
        0.
    } else if feather == 0. {
        f32::from(inside)
    } else {
        (0.5 + (if inside { distance } else { -distance }) / feather).clamp(0., 1.)
    }
}
pub fn selection_weight(selection: &[Selection], p: [f32; 2], aspect: f32) -> f32 {
    let mut value = if selection
        .first()
        .is_some_and(|s| matches!(s.mode, SelectionMode::Add | SelectionMode::Replace))
    {
        0.
    } else {
        1.
    };
    for s in selection {
        let v = s.weight(p, aspect);
        value = match s.mode {
            SelectionMode::Replace => v,
            SelectionMode::Add => 1. - (1. - value) * (1. - v),
            SelectionMode::Subtract => value * (1. - v),
            SelectionMode::Intersect => value * v,
        };
    }
    value
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub offset: [f32; 2],
    pub scale: f32,
    pub angle: f32,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            offset: [0.; 2],
            scale: 1.,
            angle: 0.,
        }
    }
}
impl Transform {
    pub fn forward(self, p: [f32; 2], aspect: f32) -> [f32; 2] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let x = (p[0] - 0.5) * self.scale;
        let y = (p[1] - 0.5) * aspect * self.scale;
        [
            c * x - s * y + 0.5 + self.offset[0],
            (s * x + c * y) / aspect + 0.5 + self.offset[1],
        ]
    }
    pub fn inverse(self, p: [f32; 2], aspect: f32) -> [f32; 2] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let x = (p[0] - 0.5 - self.offset[0]) / self.scale;
        let y = (p[1] - 0.5 - self.offset[1]) * aspect / self.scale;
        [c * x + s * y + 0.5, (-s * x + c * y) / aspect + 0.5]
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub enabled: bool,
    pub kind: Kind,
    pub points: Vec<[f32; 2]>,
    /// Radius and donor offset are source-relative, independent of viewport/mip.
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub color: [f32; 4],
    pub offset: [f32; 2],
    pub selection: Vec<Selection>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retouch {
    pub version: u32,
    pub enabled: bool,
    pub transform: Transform,
    pub strokes: Vec<Stroke>,
}
impl Default for Retouch {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            transform: Default::default(),
            strokes: vec![],
        }
    }
}
impl Retouch {
    pub fn active(&self) -> bool {
        self.enabled
            && (self.transform != Transform::default()
                || self.strokes.iter().any(|s| s.enabled && s.opacity > 0.))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.strokes.len() <= MAX_STROKES,
            "Versione/numero ritocchi non valido"
        );
        ensure!(
            self.transform
                .offset
                .iter()
                .all(|v| v.is_finite() && (-2. ..=2.).contains(v))
                && self.transform.scale.is_finite()
                && (0.01..=100.).contains(&self.transform.scale)
                && self.transform.angle.is_finite()
                && (-360. ..=360.).contains(&self.transform.angle),
            "Trasformazione ritocco non valida"
        );
        let points: usize = self
            .strokes
            .iter()
            .map(|s| s.points.len() + s.selection.iter().map(|s| s.points.len()).sum::<usize>())
            .sum();
        ensure!(
            points <= MAX_POINTS,
            "Ritocco: massimo 1024 punti complessivi"
        );
        for s in &self.strokes {
            ensure!(
                !s.points.is_empty()
                    && s.points
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "Tratto ritocco non valido"
            );
            ensure!(
                s.radius.is_finite()
                    && (1. / 131072. ..=0.25).contains(&s.radius)
                    && s.hardness.is_finite()
                    && (0. ..=1.).contains(&s.hardness)
                    && s.opacity.is_finite()
                    && (0. ..=1.).contains(&s.opacity),
                "Pennello ritocco non valido"
            );
            ensure!(
                s.color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                    && s.offset
                        .iter()
                        .all(|v| v.is_finite() && (-1. ..=1.).contains(v)),
                "Colore/sorgente ritocco non valido"
            );
            ensure!(s.selection.len() <= 16, "Troppe selezioni nel tratto");
            for selection in &s.selection {
                selection.validate()?;
            }
            let length: f32 = s
                .points
                .windows(2)
                .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
                .sum();
            ensure!(
                length / s.radius <= 8192.,
                "Tratto troppo lungo: suddividerlo in più gesti"
            );
        }
        Ok(())
    }
    pub fn apply_strokes(
        &self,
        image: &mut LinearImage,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        if !self.enabled || self.strokes.iter().all(|s| !s.enabled || s.opacity == 0.) {
            return Ok(());
        }
        let width = image.width;
        let height = image.height;
        let layer = RasterLayer::new(
            "Ritocco".into(),
            LinearImage::new(width, height, std::mem::take(&mut image.pixels))?,
        );
        let mut doc = Document {
            version: 2,
            width,
            height,
            ppi: 300.,
            resolution: None,
            unit: Unit::Pixels,
            color_space: ColorSpace::Srgb,
            layers: vec![layer],
            selection: vec![],
        };
        let result = (|| {
            for stroke in self.strokes.iter().filter(|s| s.enabled && s.opacity > 0.) {
                ensure!(!cancelled(), "Editing annullato");
                doc.selection.clear();
                if !stroke.selection.is_empty() {
                    let aspect = height as f32 / width as f32;
                    doc.selection = vec![0.; width as usize * height as usize];
                    super::regional::visit_mut(&mut doc.selection, cancelled, |i, v| {
                        *v = selection_weight(
                            &stroke.selection,
                            [
                                ((i % width as usize) as f32 + 0.5) / width as f32,
                                ((i / width as usize) as f32 + 0.5) / height as f32,
                            ],
                            aspect,
                        );
                    })?;
                }
                let brush = match stroke.kind {
                    Kind::Paint => Brush::Paint(crate::color::from_encoded_srgb(stroke.color)),
                    Kind::Erase => Brush::Erase,
                    Kind::Clone => Brush::Clone,
                    Kind::Heal => Brush::Heal,
                    Kind::AutoHeal => Brush::AutoHeal,
                };
                let donor = matches!(stroke.kind, Kind::Clone | Kind::Heal | Kind::AutoHeal)
                    .then(|| doc.layers[0].clone());
                let offset = [
                    stroke.offset[0] * width as f32,
                    stroke.offset[1] * height as f32,
                ];
                let radius = (stroke.radius * width as f32).clamp(0.5, 1024.);
                let to_pixel = |p: [f32; 2]| [p[0] * width as f32, p[1] * height as f32];
                let mut last = to_pixel(stroke.points[0]);
                let mut count = 0;
                for (i, p) in stroke.points.iter().copied().map(to_pixel).enumerate() {
                    let steps = if i == 0 {
                        1
                    } else {
                        ((p[0] - last[0]).hypot(p[1] - last[1]) / (radius * 0.3).max(0.5))
                            .ceil()
                            .max(1.) as usize
                    };
                    count += steps;
                    ensure!(
                        count <= 32768,
                        "Tratto troppo lungo: suddividerlo in più gesti"
                    );
                    for n in 1..=steps {
                        ensure!(!cancelled(), "Editing annullato");
                        let t = n as f32 / steps as f32;
                        let at = [
                            last[0] + (p[0] - last[0]) * t,
                            last[1] + (p[1] - last[1]) * t,
                        ];
                        doc.dab(
                            0,
                            at,
                            radius,
                            stroke.hardness,
                            stroke.opacity,
                            brush,
                            donor.as_ref().map(|d| (d, offset)),
                        )?;
                    }
                    last = p;
                }
            }
            Ok(())
        })();
        image.pixels = std::mem::take(&mut doc.layers[0].pixels);
        result
    }
    pub fn apply_transform(
        &self,
        image: &mut LinearImage,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        if !self.enabled || self.transform == Transform::default() {
            return Ok(());
        }
        let width = image.width;
        let height = image.height;
        let aspect = height as f32 / width as f32;
        let source = RasterLayer::new("".into(), image.clone());
        super::regional::visit_mut(&mut image.pixels, cancelled, |i, p| {
            let uv = self.transform.inverse(
                [
                    ((i % width as usize) as f32 + 0.5) / width as f32,
                    ((i / width as usize) as f32 + 0.5) / height as f32,
                ],
                aspect,
            );
            let x = uv[0] * width as f32 - 0.5;
            let y = uv[1] * height as f32 - 0.5;
            let fx = x - x.floor();
            let fy = y - y.floor();
            *p = [0.; 4];
            for (dx, dy, w) in [
                (0., 0., (1. - fx) * (1. - fy)),
                (1., 0., fx * (1. - fy)),
                (0., 1., (1. - fx) * fy),
                (1., 1., fx * fy),
            ] {
                let sample = source.sample([x.floor() + dx + 0.5, y.floor() + dy + 0.5]);
                for c in 0..4 {
                    p[c] += sample[c] * w;
                }
            }
        })
    }
}
