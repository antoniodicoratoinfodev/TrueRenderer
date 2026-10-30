//! Portable raster documents. Linear premultiplied Rec.2020, independent assets.
use crate::{
    color::{LinearImage, Pixel},
    editing::EditRecipe,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const MAX_CANVAS_PIXELS: usize = crate::color::MAX_PIXELS;
pub const MAX_LAYERS: usize = 64;

/// Conservative host envelope: current assets, undo/redo and a transaction,
/// native render/filter scratch, plus display and bounded metadata IO.
/// This admits allocations; it is not a physical RSS guarantee.
pub fn admission_bytes(storage: u64, largest_pixels: u64) -> u64 {
    let history = storage.max(256 * 1024 * 1024);
    (storage * 3 + history * 2 + largest_pixels * 96 + 128 * 1024 * 1024)
        .max(3 * 1024 * 1024 * 1024)
}

/// Native projects are browser assets, never inputs to photographic decoders.
pub fn is_document_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("truerenderer"))
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Unit {
    Pixels,
    Centimeters,
    Millimeters,
    Inches,
}
impl Unit {
    pub fn pixels(self, value: f64, ppi: f64) -> Result<u32> {
        ensure!(
            value.is_finite() && value > 0. && ppi.is_finite() && (1. ..=9600.).contains(&ppi),
            "Dimensioni/PPI non validi"
        );
        let px = match self {
            Self::Pixels => value,
            Self::Centimeters => value * ppi / 2.54,
            Self::Millimeters => value * ppi / 25.4,
            Self::Inches => value * ppi,
        };
        ensure!(
            (1. ..=65536.).contains(&px.round()),
            "Dimensione fuori quota"
        );
        Ok(px.round() as u32)
    }
}

pub use crate::export_spaces::ColorSpace;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub angle: f32,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            scale: 1.,
            angle: 0.,
        }
    }
}
impl Transform {
    pub fn forward(self, p: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.angle.to_radians().sin_cos();
        [
            self.x + self.scale * (c * p[0] - s * p[1]),
            self.y + self.scale * (s * p[0] + c * p[1]),
        ]
    }
    pub fn inverse(self, p: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let x = (p[0] - self.x) / self.scale;
        let y = (p[1] - self.y) / self.scale;
        [c * x + s * y, -s * x + c * y]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterLayer {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend: Blend,
    pub transform: Transform,
    pub recipe: EditRecipe,
    /// Layer-local coverage. Empty means all selected.
    #[serde(skip)]
    pub mask: Vec<f32>,
    #[serde(skip)]
    pub pixels: Vec<Pixel>,
}
impl RasterLayer {
    pub fn new(name: String, image: LinearImage) -> Self {
        Self {
            name,
            width: image.width,
            height: image.height,
            pixels: image.pixels,
            visible: true,
            locked: false,
            opacity: 1.,
            blend: Blend::Normal,
            transform: Default::default(),
            recipe: EditRecipe::neutral(crate::decoder::RawEngine::default()),
            mask: vec![],
        }
    }
    pub fn sample(&self, p: [f32; 2]) -> Pixel {
        let x = p[0].floor() as i64;
        let y = p[1].floor() as i64;
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return [0.; 4];
        }
        self.pixels[y as usize * self.width as usize + x as usize]
    }
    pub fn source_point(&self, canvas: [f32; 2]) -> Option<[f32; 2]> {
        let p = self.transform.inverse(canvas);
        let native = [self.width, self.height];
        let output = self
            .recipe
            .advanced
            .as_ref()
            .map_or(native, |a| a.geometry.output_size(native));
        self.recipe
            .source_point(
                [
                    p[0] as f64 / output[0] as f64,
                    p[1] as f64 / output[1] as f64,
                ],
                native,
            )
            .map(|p| {
                [
                    p[0] as f32 * self.width as f32,
                    p[1] as f32 * self.height as f32,
                ]
            })
    }
    pub fn canvas_point(&self, source: [f32; 2]) -> Option<[f32; 2]> {
        let source = if let Some(r) = &self.recipe.retouch
            && r.enabled
        {
            let uv = r.transform.forward(
                [
                    source[0] / self.width as f32,
                    source[1] / self.height as f32,
                ],
                self.height as f32 / self.width as f32,
            );
            [uv[0] * self.width as f32, uv[1] * self.height as f32]
        } else {
            source
        };
        let p = if let Some(a) = &self.recipe.advanced {
            let native = [self.width, self.height];
            let output = a.geometry.output_size(native);
            let p = a.geometry.output_point(
                [
                    source[0] as f64 / self.width as f64,
                    source[1] as f64 / self.height as f64,
                ],
                native,
            )?;
            [
                p[0] as f32 * output[0] as f32,
                p[1] as f32 * output[1] as f32,
            ]
        } else {
            source
        };
        Some(self.transform.forward(p))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub ppi: f64,
    /// v2: missing means the source did not declare a physical density.
    /// v1 has only the legacy square `ppi` value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<crate::protocol::Resolution>,
    pub unit: Unit,
    pub color_space: ColorSpace,
    pub layers: Vec<RasterLayer>,
    /// Canvas selection; empty means unrestricted.
    #[serde(skip)]
    pub selection: Vec<f32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Brush {
    Paint(Pixel),
    Erase,
    Clone,
    Heal,
    AutoHeal,
}

fn area(width: u32, height: u32) -> Result<usize> {
    let count = u64::from(width) * u64::from(height);
    ensure!(
        width > 0 && height > 0 && count <= MAX_CANVAS_PIXELS as u64,
        "Tela/raster fuori quota (64 Mi pixel)"
    );
    Ok(count as usize)
}
impl Document {
    pub fn new(width: u32, height: u32, ppi: f64, background: Pixel) -> Result<Self> {
        let n = area(width, height)?;
        let image = LinearImage::new(width, height, vec![background; n])?;
        let document = Self {
            version: 2,
            width,
            height,
            ppi,
            resolution: Some(crate::protocol::Resolution { x: ppi, y: ppi }),
            unit: Unit::Pixels,
            color_space: ColorSpace::Srgb,
            layers: vec![RasterLayer::new("Sfondo".into(), image)],
            selection: vec![],
        };
        document.validate()?;
        Ok(document)
    }
    pub fn largest_pixels(&self) -> u64 {
        self.layers
            .iter()
            .map(|l| u64::from(l.width) * u64::from(l.height))
            .chain(std::iter::once(
                u64::from(self.width) * u64::from(self.height),
            ))
            .max()
            .unwrap_or(0)
    }
    pub fn physical_resolution(&self) -> Option<crate::protocol::Resolution> {
        if self.version == 1 {
            Some(crate::protocol::Resolution {
                x: self.ppi,
                y: self.ppi,
            })
        } else {
            self.resolution
        }
    }
    pub fn storage_bytes(&self) -> usize {
        self.selection.len() * 4
            + self
                .layers
                .iter()
                .map(|l| l.pixels.len() * 16 + l.mask.len() * 4)
                .sum::<usize>()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [1, 2].contains(&self.version),
            "Versione documento non supportata"
        );
        if let Some(r) = self.resolution {
            ensure!(
                self.version == 2 && self.ppi == r.x,
                "Risoluzione/versione documento incoerente"
            );
            r.validate()?;
        }
        let n = area(self.width, self.height)?;
        ensure!(
            self.ppi.is_finite() && (1. ..=9600.).contains(&self.ppi),
            "PPI non valido"
        );
        ensure!(
            self.layers.len() <= MAX_LAYERS && self.storage_bytes() <= MAX_DOCUMENT_BYTES,
            "Documento fuori quota"
        );
        validate_mask(&self.selection, n)?;
        for l in &self.layers {
            ensure!(l.name.len() <= 1024, "Nome livello troppo lungo");
            let n = area(l.width, l.height)?;
            ensure!(l.pixels.len() == n, "Raster incompleto");
            ensure!(
                l.pixels
                    .iter()
                    .all(|p| p.iter().all(|v| v.is_finite()) && (0. ..=1.).contains(&p[3])),
                "Pixel non validi"
            );
            ensure!((0. ..=1.).contains(&l.opacity), "Opacità non valida");
            let t = l.transform;
            ensure!(
                [t.x, t.y, t.scale, t.angle].into_iter().all(f32::is_finite)
                    && (0.01..=100.).contains(&t.scale)
                    && t.x.abs() <= 1e6
                    && t.y.abs() <= 1e6
                    && t.angle.abs() <= 36000.,
                "Trasformazione non valida"
            );
            validate_mask(&l.mask, n)?;
            l.recipe.validate()?;
        }
        Ok(())
    }
    pub fn insert(&mut self, layer: RasterLayer) -> Result<()> {
        ensure!(
            self.layers.len() < MAX_LAYERS
                && self
                    .storage_bytes()
                    .saturating_add(layer.pixels.len() * 16 + layer.mask.len() * 4)
                    <= MAX_DOCUMENT_BYTES,
            "Memoria livelli esaurita"
        );
        self.layers.push(layer);
        if let Err(e) = self.validate() {
            self.layers.pop();
            return Err(e);
        }
        Ok(())
    }
    pub fn duplicate(&mut self, index: usize) -> Result<()> {
        ensure!(self.layers.len() < MAX_LAYERS, "Troppi livelli");
        let layer = self
            .layers
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Livello mancante"))?;
        ensure!(
            self.storage_bytes() + layer.pixels.len() * 16 + layer.mask.len() * 4
                <= MAX_DOCUMENT_BYTES,
            "Memoria copia esaurita"
        );
        let mut copy = layer.clone();
        copy.name.push_str(" · copia");
        copy.locked = false;
        self.insert(copy)?;
        let copy = self.layers.pop().unwrap();
        self.layers.insert(index + 1, copy);
        Ok(())
    }
    /// Copy selected source pixels without flattening the photographic recipe.
    /// Selection is in canvas coordinates; its coverage is baked into a private
    /// layer-local mask so the new piece can subsequently move independently.
    pub fn copy_selection(&mut self, index: usize) -> Result<()> {
        self.validate()?;
        ensure!(
            self.selection.iter().any(|v| *v > 0.),
            "Selezionare una parte dell'immagine"
        );
        let layer = self
            .layers
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Livello mancante"))?;
        ensure!(
            self.layers.len() < MAX_LAYERS
                && self.storage_bytes() + layer.pixels.len() * 20 <= MAX_DOCUMENT_BYTES,
            "Memoria copia esaurita"
        );
        let mut copy = layer.clone();
        copy.name.push_str(" · selezione");
        copy.locked = false;
        copy.mask = (0..layer.pixels.len())
            .map(|i| {
                let coverage = layer
                    .canvas_point([
                        (i % layer.width as usize) as f32 + 0.5,
                        (i / layer.width as usize) as f32 + 0.5,
                    ])
                    .map_or(0., |p| {
                        let x = p[0].floor() as i64;
                        let y = p[1].floor() as i64;
                        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
                            0.
                        } else {
                            self.selection[y as usize * self.width as usize + x as usize]
                        }
                    });
                coverage * layer.mask.get(i).copied().unwrap_or(1.)
            })
            .collect();
        ensure!(
            copy.mask.iter().any(|v| *v > 0.),
            "La selezione non interseca il livello"
        );
        self.insert(copy)?;
        let copy = self.layers.pop().unwrap();
        self.layers.insert(index + 1, copy);
        self.selection.clear();
        Ok(())
    }
    pub fn remove_layer(&mut self, index: usize) -> Result<()> {
        ensure!(index > 0, "Il livello 0 resta alla base della composizione");
        ensure!(
            self.layers.get(index).is_some_and(|l| !l.locked),
            "Livello assente o bloccato"
        );
        self.layers.remove(index);
        Ok(())
    }
    pub fn move_layer(&mut self, from: usize, to: usize) -> Result<()> {
        ensure!(
            from > 0 && to > 0,
            "Il livello 0 resta alla base della composizione"
        );
        ensure!(
            to < self.layers.len() && self.layers.get(from).is_some_and(|l| !l.locked),
            "Livello assente o bloccato"
        );
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        Ok(())
    }
    pub fn render(&self) -> Result<LinearImage> {
        self.validate()?;
        let mut output = LinearImage::new(
            self.width,
            self.height,
            vec![[0.; 4]; area(self.width, self.height)?],
        )?;
        for layer in self.layers.iter().filter(|l| l.visible && l.opacity > 0.) {
            let mut image = LinearImage::new(layer.width, layer.height, layer.pixels.clone())?;
            if !layer.mask.is_empty() {
                for (pixel, coverage) in image.pixels.iter_mut().zip(&layer.mask) {
                    for value in pixel {
                        *value *= coverage;
                    }
                }
            }
            layer.recipe.apply(&mut image)?;
            for (i, dest) in output.pixels.iter_mut().enumerate() {
                let p = layer.transform.inverse([
                    (i % self.width as usize) as f32 + 0.5,
                    (i / self.width as usize) as f32 + 0.5,
                ]);
                // Bilinear sampling in premultiplied linear light, transparent outside.
                let x = (p[0] - 0.5).floor() as i64;
                let y = (p[1] - 0.5).floor() as i64;
                let fx = p[0] - 0.5 - x as f32;
                let fy = p[1] - 0.5 - y as f32;
                let mut src = [0.; 4];
                for (dx, dy, w) in [
                    (0, 0, (1. - fx) * (1. - fy)),
                    (1, 0, fx * (1. - fy)),
                    (0, 1, (1. - fx) * fy),
                    (1, 1, fx * fy),
                ] {
                    let xx = x + dx;
                    let yy = y + dy;
                    if xx >= 0 && yy >= 0 && xx < image.width as i64 && yy < image.height as i64 {
                        let j = yy as usize * image.width as usize + xx as usize;
                        for (c, v) in src.iter_mut().enumerate() {
                            *v += image.pixels[j][c] * w * layer.opacity;
                        }
                    }
                }
                composite(dest, src, layer.blend);
            }
        }
        Ok(output)
    }
    pub fn select_polygon(
        &mut self,
        points: &[[f32; 2]],
        mode: SelectionMode,
        feather: f32,
    ) -> Result<()> {
        ensure!(
            points.len() >= 3
                && points.len() <= 4096
                && points.iter().flatten().all(|v| v.is_finite())
                && (0. ..=1000.).contains(&feather),
            "Selezione non valida"
        );
        let n = area(self.width, self.height)?;
        ensure!(
            self.storage_bytes() + if self.selection.is_empty() { n * 4 } else { 0 }
                <= MAX_DOCUMENT_BYTES,
            "Memoria selezione esaurita"
        );
        let initial = if mode == SelectionMode::Intersect || mode == SelectionMode::Subtract {
            1.
        } else {
            0.
        };
        if self.selection.is_empty() {
            self.selection = vec![initial; n];
        }
        for (i, old) in self.selection.iter_mut().enumerate() {
            let p = [
                (i % self.width as usize) as f32 + 0.5,
                (i / self.width as usize) as f32 + 0.5,
            ];
            let mut inside = false;
            let mut distance = f32::MAX;
            for j in 0..points.len() {
                let a = points[j];
                let b = points[(j + 1) % points.len()];
                if (a[1] > p[1]) != (b[1] > p[1])
                    && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
                {
                    inside = !inside;
                }
                let d = [b[0] - a[0], b[1] - a[1]];
                let len = d[0] * d[0] + d[1] * d[1];
                let t = if len > 0. {
                    ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len
                } else {
                    0.
                };
                distance = distance.min(
                    (p[0] - a[0] - t.clamp(0., 1.) * d[0])
                        .hypot(p[1] - a[1] - t.clamp(0., 1.) * d[1]),
                );
            }
            let v = if feather == 0. {
                f32::from(inside)
            } else {
                (0.5 + (if inside { distance } else { -distance }) / feather).clamp(0., 1.)
            };
            *old = match mode {
                SelectionMode::Replace => v,
                SelectionMode::Add => old.max(v),
                SelectionMode::Subtract => *old * (1. - v),
                SelectionMode::Intersect => *old * v,
            };
        }
        Ok(())
    }
    /// One dab; callers interpolate gestures and retain a pre-stroke source.
    #[allow(clippy::too_many_arguments)]
    pub fn dab(
        &mut self,
        index: usize,
        center: [f32; 2],
        radius: f32,
        hardness: f32,
        opacity: f32,
        brush: Brush,
        source: Option<(&RasterLayer, [f32; 2])>,
    ) -> Result<()> {
        ensure!(
            center.into_iter().all(f32::is_finite)
                && (0.5..=1024.).contains(&radius)
                && (0. ..=1.).contains(&hardness)
                && (0. ..=1.).contains(&opacity),
            "Pennello non valido"
        );
        if let Brush::Paint(pixel) = brush {
            ensure!(
                pixel.iter().all(|v| v.is_finite()) && (0. ..=1.).contains(&pixel[3]),
                "Colore non valido"
            );
        }
        let layer = self
            .layers
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("Livello mancante"))?;
        ensure!(!layer.locked, "Livello bloccato");
        if matches!(brush, Brush::Clone | Brush::Heal) {
            ensure!(
                source.is_some(),
                "Campionare la sorgente prima di ritoccare"
            );
        }
        let canvas_radius = radius;
        let Some(p) = layer.source_point(center) else {
            return Ok(());
        };
        let nonlinear = layer.recipe.advanced.as_ref().is_some_and(|a| {
            a.geometry.perspective != [0.; 2]
                || a.geometry.distortion != 0.
                || a.geometry.moustache != 0.
        });
        // An affine footprint is bounded by its four corners. Nonlinear geometry
        // uses the complete source domain so extreme warps cannot omit pixels.
        let corners = [
            [-radius, -radius],
            [radius, -radius],
            [radius, radius],
            [-radius, radius],
        ]
        .map(|v| {
            layer
                .source_point([center[0] + v[0], center[1] + v[1]])
                .unwrap_or(p)
        });
        let left = if nonlinear {
            0
        } else {
            corners
                .iter()
                .map(|p| p[0])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.) as u32
        };
        let top = if nonlinear {
            0
        } else {
            corners
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.) as u32
        };
        let right = if nonlinear {
            layer.width
        } else {
            (corners
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .max(0.) as u32)
                .min(layer.width)
        };
        let bottom = if nonlinear {
            layer.height
        } else {
            (corners
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .max(0.) as u32)
                .min(layer.height)
        };
        let radius = corners
            .iter()
            .map(|q| (q[0] - p[0]).hypot(q[1] - p[1]))
            .fold(0., f32::max)
            / std::f32::consts::SQRT_2;
        let sampled = source.map(|(s, _)| s).unwrap_or(layer);
        let mut offset = source.map_or([0.; 2], |(_, offset)| offset);
        // Compare boundary patches, retaining the best texture-bearing source.
        if brush == Brush::AutoHeal {
            let mut best = f32::INFINITY;
            for ring in [2.5, 4.] {
                for k in 0..32 {
                    let angle = k as f32 * std::f32::consts::TAU / 32.;
                    let candidate = [
                        angle.cos() * (radius * ring + 2.),
                        angle.sin() * (radius * ring + 2.),
                    ];
                    let mut error = 0.;
                    let mut valid = true;
                    for j in 0..16 {
                        let a = j as f32 * std::f32::consts::TAU / 16.;
                        let q = [
                            p[0] + a.cos() * (radius + 1.),
                            p[1] + a.sin() * (radius + 1.),
                        ];
                        let target = sampled.sample(q);
                        let donor = sampled.sample([q[0] + candidate[0], q[1] + candidate[1]]);
                        if target[3] < 0.99 || donor[3] < 0.99 {
                            valid = false;
                            break;
                        }
                        for c in 0..3 {
                            error += (target[c] - donor[c]).powi(2);
                        }
                    }
                    if valid && error < best {
                        best = error;
                        offset = candidate;
                    }
                }
            }
            ensure!(
                best.is_finite(),
                "Nessuna sorgente completa per Ripara rapido vicino al bordo"
            );
        }
        let mut correction = [0.; 3];
        if matches!(brush, Brush::Heal | Brush::AutoHeal) {
            let mut count = 0.;
            for j in 0..32 {
                let a = j as f32 * std::f32::consts::TAU / 32.;
                let q = [
                    p[0] + a.cos() * (radius + 1.),
                    p[1] + a.sin() * (radius + 1.),
                ];
                let target = layer.sample(q);
                let donor = sampled.sample([q[0] + offset[0], q[1] + offset[1]]);
                if target[3] > 0.99 && donor[3] > 0.99 {
                    for c in 0..3 {
                        correction[c] += target[c] - donor[c];
                    }
                    count += 1.;
                }
            }
            ensure!(count > 0., "Bordo del correttivo senza campioni opachi");
            correction = correction.map(|v| v / count);
        }
        // Freeze a donor for automatic repair if the caller did not supply a stroke snapshot.
        let owned = if brush == Brush::AutoHeal && source.is_none() {
            Some(layer.clone())
        } else {
            None
        };
        let sampled = source.map(|(s, _)| s).or(owned.as_ref());
        for y in top..bottom {
            for x in left..right {
                let Some(canvas) = layer.canvas_point([x as f32 + 0.5, y as f32 + 0.5]) else {
                    continue;
                };
                let distance = (canvas[0] - center[0]).hypot(canvas[1] - center[1]) / canvas_radius;
                if distance > 1. {
                    continue;
                }
                let mut weight = opacity
                    * if hardness == 1. || distance <= hardness {
                        1.
                    } else {
                        (1. - distance) / (1. - hardness)
                    };
                if !self.selection.is_empty() {
                    let cx = canvas[0].floor() as i64;
                    let cy = canvas[1].floor() as i64;
                    if cx < 0 || cy < 0 || cx >= self.width as i64 || cy >= self.height as i64 {
                        continue;
                    }
                    weight *= self.selection[cy as usize * self.width as usize + cx as usize];
                }
                let i = y as usize * layer.width as usize + x as usize;
                let mut src = match brush {
                    Brush::Paint(v) => v,
                    Brush::Erase => [0.; 4],
                    Brush::Clone | Brush::Heal | Brush::AutoHeal => sampled
                        .unwrap()
                        .sample([x as f32 + 0.5 + offset[0], y as f32 + 0.5 + offset[1]]),
                };
                if matches!(brush, Brush::Heal | Brush::AutoHeal) {
                    for c in 0..3 {
                        src[c] += correction[c] * src[3];
                    }
                }
                if matches!(brush, Brush::Paint(_)) {
                    composite(&mut layer.pixels[i], src.map(|v| v * weight), Blend::Normal);
                } else if weight == 1. {
                    layer.pixels[i] = src;
                } else {
                    for (c, v) in src.iter().enumerate() {
                        layer.pixels[i][c] += (v - layer.pixels[i][c]) * weight;
                    }
                }
            }
        }
        Ok(())
    }
}
fn validate_mask(mask: &[f32], n: usize) -> Result<()> {
    ensure!(
        (mask.is_empty() || mask.len() == n) && mask.iter().all(|v| (0. ..=1.).contains(v)),
        "Maschera non valida"
    );
    Ok(())
}
fn composite(dest: &mut Pixel, src: Pixel, blend: Blend) {
    let sa = src[3];
    let da = dest[3];
    for c in 0..3 {
        let a = if sa > 0. { src[c] / sa } else { 0. };
        let b = if da > 0. { dest[c] / da } else { 0. };
        let blended = match blend {
            Blend::Normal => a,
            Blend::Multiply => a * b,
            Blend::Screen => a + b - a * b,
        };
        dest[c] = src[c] * (1. - da) + dest[c] * (1. - sa) + blended * sa * da;
    }
    dest[3] = sa + da * (1. - sa);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_pixels_linear_composite_and_transform() {
        let mut d = Document::new(4, 3, 300., [0.2, 0.1, 0., 1.]).unwrap();
        d.duplicate(0).unwrap();
        d.layers[1].pixels.fill([0.5, 0., 0., 0.5]);
        d.layers[1].transform.x = 1.;
        assert_eq!(d.layers[0].pixels[0], [0.2, 0.1, 0., 1.]);
        let image = d.render().unwrap();
        assert_eq!(image.pixels[0], [0.2, 0.1, 0., 1.]);
        assert_eq!(image.pixels[1], [0.6, 0.05, 0., 1.]);
        d.layers[1].visible = false;
        assert_eq!(d.render().unwrap().pixels, d.layers[0].pixels);
    }
    #[test]
    fn selection_boolean_feather_and_erase_respect_canvas_coordinates() {
        let mut d = Document::new(8, 8, 300., [1.; 4]).unwrap();
        let square = [[2., 2.], [6., 2.], [6., 6.], [2., 6.]];
        d.select_polygon(&square, SelectionMode::Replace, 0.)
            .unwrap();
        assert_eq!(d.selection.iter().sum::<f32>(), 16.);
        d.layers[0].transform.x = 2.;
        d.dab(0, [2.5, 2.5], 0.5, 1., 1., Brush::Erase, None)
            .unwrap();
        assert_eq!(d.layers[0].pixels[2 * 8], [0.; 4]);
        assert_eq!(d.layers[0].pixels[2 * 8 + 2], [1.; 4]);
        d.select_polygon(&square, SelectionMode::Subtract, 0.)
            .unwrap();
        assert!(d.selection.iter().all(|v| *v == 0.));
        d.select_polygon(&square, SelectionMode::Add, 2.).unwrap();
        assert_eq!(d.selection[2 * 8 + 2], 0.75);
        d.select_polygon(&square, SelectionMode::Intersect, 0.)
            .unwrap();
        assert_eq!(d.selection[8 + 2], 0.);
    }
    #[test]
    fn clone_uses_frozen_donor_and_heal_removes_impulse() {
        let mut d = Document::new(40, 40, 300., [0.2, 0.3, 0.4, 1.]).unwrap();
        d.layers[0].pixels[20 * 40 + 20] = [1.; 4];
        let donor = d.layers[0].clone();
        d.dab(
            0,
            [20.5, 20.5],
            1.,
            1.,
            1.,
            Brush::AutoHeal,
            Some((&donor, [0.; 2])),
        )
        .unwrap();
        assert_eq!(d.layers[0].pixels[20 * 40 + 20], [0.2, 0.3, 0.4, 1.]);
        d.dab(
            0,
            [2.5, 2.5],
            0.5,
            1.,
            1.,
            Brush::Clone,
            Some((&donor, [18., 18.])),
        )
        .unwrap();
        assert_eq!(d.layers[0].pixels[2 * 40 + 2], [1.; 4]);
        d.layers[0].locked = true;
        assert!(d.dab(0, [2., 2.], 1., 1., 1., Brush::Erase, None).is_err());
    }
    #[test]
    fn retouch_maps_crop_rotation_optics_and_canvas_selection() {
        let mut d = Document::new(32, 24, 300., [0.2, 0.3, 0.4, 1.]).unwrap();
        let a = d.layers[0]
            .recipe
            .advanced
            .get_or_insert_with(Default::default);
        a.geometry.crop = [0.1, 0.2, 0.9, 0.9];
        a.geometry.quarter_turns = 1;
        a.geometry.flip_horizontal = true;
        a.geometry.angle = 12.;
        a.geometry.scale = 1.2;
        a.geometry.perspective = [15., -12.];
        a.geometry.distortion = 40.;
        a.geometry.moustache = -30.;
        d.layers[0].recipe.require_process(3);
        d.layers[0].transform = Transform {
            x: 5.,
            y: 2.,
            scale: 1.3,
            angle: -8.,
        };
        for source in [[12.5, 10.5], [15.5, 11.5], [8.5, 6.5]] {
            let canvas = d.layers[0].canvas_point(source).unwrap();
            let back = d.layers[0].source_point(canvas).unwrap();
            assert!((source[0] - back[0]).abs() < 0.0001 && (source[1] - back[1]).abs() < 0.0001);
        }
        let target = [15.5, 11.5];
        let canvas = d.layers[0].canvas_point(target).unwrap();
        d.selection = vec![0.; 32 * 24];
        let i = canvas[1].floor() as usize * 32 + canvas[0].floor() as usize;
        d.selection[i] = 1.;
        d.dab(0, canvas, 0.5, 1., 1., Brush::Erase, None).unwrap();
        assert_eq!(d.layers[0].pixels[11 * 32 + 15], [0.; 4]);
        assert_eq!(d.layers[0].pixels[0], [0.2, 0.3, 0.4, 1.]);
    }
    #[test]
    fn base_is_fixed_and_selection_copy_follows_transformed_source() {
        let mut d = Document::new(8, 6, 300., [0.2, 0.1, 0., 1.]).unwrap();
        d.layers[0].transform.x = 2.;
        d.layers[0].mask = vec![0.5; 48];
        d.select_polygon(
            &[[3., 1.], [5., 1.], [5., 3.], [3., 3.]],
            SelectionMode::Replace,
            0.,
        )
        .unwrap();
        let base = d.layers[0].clone();
        d.copy_selection(0).unwrap();
        assert_eq!(d.layers[1].mask.iter().sum::<f32>(), 2.);
        assert_eq!(d.layers[1].mask[9], 0.5);
        assert_eq!(d.layers[1].mask[8], 0.);
        assert_eq!(d.layers[0], base);
        assert!(d.selection.is_empty());
        d.duplicate(0).unwrap();
        assert_eq!(d.layers.len(), 3);
        assert!(d.remove_layer(0).is_err());
        assert!(d.move_layer(0, 1).is_err());
        assert!(d.move_layer(1, 0).is_err());
        d.move_layer(1, 2).unwrap();
        assert_eq!(d.layers[0], base);
        d.layers[1].pixels[0] = [0.; 4];
        assert_eq!(d.layers[0], base);
    }
    #[test]
    fn physical_units_and_invalid_metadata() {
        assert_eq!(Unit::Centimeters.pixels(2.54, 300.).unwrap(), 300);
        assert_eq!(Unit::Millimeters.pixels(25.4, 300.).unwrap(), 300);
        assert_eq!(Unit::Inches.pixels(1., 300.).unwrap(), 300);
        assert!(Unit::Pixels.pixels(f64::INFINITY, 300.).is_err());
        assert!(Document::new(u32::MAX, u32::MAX, 300., [0.; 4]).is_err());
        let mut d = Document::new(1, 1, 72., [0.; 4]).unwrap();
        d.layers[0].transform.scale = 0.;
        assert!(d.validate().is_err());
    }
}
