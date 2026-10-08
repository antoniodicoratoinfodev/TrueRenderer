//! Photographic layers, process 5. Bottom-to-top, straight extended Rec.2020.
//! Effect masks never modify coverage. All guides are the input of their layer.
//! Inline vectors share the existing 48 KiB recipe budget; no raster asset is
//! stored in a cache or smuggled into IPC. New content types need their own gate.
use super::{
    CurvePoint,
    color::{hue_chroma, hue_rgb},
    detail::luma,
    masks::Mask,
    range,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
mod curves;
mod levels;
mod selective;
pub use curves::{CurveSpace, LuminanceCurve, ParametricCurve};
pub use levels::{
    LevelAdjustments, LevelAnalysis, LevelStatistics, LevelsAction, MAX_LEVEL_SAMPLES,
};
mod toning;
pub use selective::{Colorize, ExposureGamma, SelectiveColor, SelectiveMethod, TonalAdjustments};
pub use toning::{BlackAndWhite, ColorFilter, GradientMap, GradientStop, Grading, GradingZone};
pub type Id = Uuid;

pub const MAX_LAYERS: usize = 16;
pub const MAX_MASK_NODES: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerStack {
    /// The base remains the historical evaluator; later base edits can promote
    /// this version explicitly, without distributing its parameters into nodes.
    pub base_process: u32,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub locked: bool,
    pub opacity: f32,
    pub mask: MaskGraph,
    pub operators: Vec<Operator>,
}
impl Layer {
    pub fn new(name: impl Into<String>, operator: Operator) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            locked: false,
            opacity: 1.,
            mask: MaskGraph::default(),
            operators: vec![operator],
        }
    }
    pub fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        copy.id = Uuid::new_v4();
        copy.mask.renew_ids();
        copy
    }
    pub fn active(&self) -> bool {
        self.enabled && self.opacity != 0. && self.operators.iter().any(|o| !o.is_neutral())
    }
    pub fn apply(&self, rgb: [f32; 3], xy: [f32; 2], aspect: f32) -> [f32; 3] {
        if !self.active() {
            return rgb;
        }
        let w = self.opacity * self.mask.weight(xy, rgb, aspect);
        if w == 0. {
            return rgb;
        }
        let mut edited = rgb;
        for operator in &self.operators {
            edited = operator.apply(edited, rgb);
        }
        std::array::from_fn(|c| rgb[c] + w * (edited[c] - rgb[c]))
    }
}
impl LayerStack {
    /// Projection for analysis of an operator's input. Call on a temporary
    /// recipe: earlier operators run in full before the target layer's blend.
    pub fn retain_before_operator(&mut self, id: Id, operator: usize) -> Result<()> {
        let i = self
            .layers
            .iter()
            .position(|l| l.id == id)
            .ok_or_else(|| anyhow::anyhow!("Livello analisi mancante"))?;
        ensure!(
            operator < self.layers[i].operators.len(),
            "Operatore analisi mancante"
        );
        if operator == 0 {
            self.layers.truncate(i);
        } else {
            self.layers.truncate(i + 1);
            let layer = &mut self.layers[i];
            layer.operators.truncate(operator);
            layer.mask = Default::default();
            layer.opacity = 1.;
            layer.enabled = true;
        }
        Ok(())
    }
    pub(super) fn prepare(&self, aspect: f32) -> Vec<PreparedLayer<'_>> {
        self.layers
            .iter()
            .filter(|l| l.active())
            .map(|layer| {
                let index = |id| {
                    layer
                        .mask
                        .nodes
                        .iter()
                        .position(|n| n.id == id)
                        .expect("validated mask")
                };
                let nodes = layer
                    .mask
                    .nodes
                    .iter()
                    .map(|n| match &n.kind {
                        MaskKind::Shape(m) => {
                            PreparedMask::Shape(super::masks::Prepared::new(m, aspect))
                        }
                        MaskKind::Color(c) => PreparedMask::Color(c),
                        MaskKind::Constant(v) => PreparedMask::Constant(*v),
                        MaskKind::Add(a, b) => {
                            PreparedMask::Pair(Combine::Add, index(*a), index(*b))
                        }
                        MaskKind::Subtract(a, b) => {
                            PreparedMask::Pair(Combine::Subtract, index(*a), index(*b))
                        }
                        MaskKind::Intersect(a, b) => {
                            PreparedMask::Pair(Combine::Intersect, index(*a), index(*b))
                        }
                        MaskKind::Invert(a) => PreparedMask::Invert(index(*a)),
                    })
                    .collect();
                PreparedLayer {
                    opacity: layer.opacity,
                    operators: layer.operators.iter().filter(|o| !o.is_neutral()).collect(),
                    root: layer.mask.root.map(index),
                    nodes,
                }
            })
            .collect()
    }
    pub fn is_neutral(&self) -> bool {
        !self.layers.iter().any(Layer::active)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=4).contains(&self.base_process),
            "Processo base non supportato"
        );
        ensure!(
            self.layers.len() <= MAX_LAYERS,
            "Massimo 16 livelli fotografici"
        );
        let mut ids = HashSet::new();
        let mut nodes = 0;
        for layer in &self.layers {
            ensure!(
                !layer.id.is_nil() && ids.insert(layer.id),
                "Identità livello duplicata o nulla"
            );
            ensure!(
                !layer.name.trim().is_empty()
                    && layer.name.len() <= 128
                    && !layer.name.chars().any(char::is_control),
                "Nome livello non valido"
            );
            range(layer.opacity, 0., 1., "Intensità livello")?;
            ensure!(
                !layer.operators.is_empty() && layer.operators.len() <= 8,
                "Operatori fuori quota"
            );
            for operator in &layer.operators {
                operator.validate()?;
            }
            layer.mask.validate()?;
            for node in &layer.mask.nodes {
                ensure!(ids.insert(node.id), "Identità maschera duplicata");
            }
            nodes += layer.mask.nodes.len();
        }
        ensure!(nodes <= MAX_MASK_NODES, "Massimo 64 componenti di maschera");
        Ok(())
    }
    pub fn point_count(&self) -> usize {
        self.layers
            .iter()
            .flat_map(|l| &l.mask.nodes)
            .map(|n| match &n.kind {
                MaskKind::Shape(m) => m.points.len(),
                _ => 0,
            })
            .sum()
    }
    pub fn apply(&self, mut rgb: [f32; 3], xy: [f32; 2], aspect: f32) -> [f32; 3] {
        for layer in &self.layers {
            rgb = layer.apply(rgb, xy, aspect);
        }
        rgb
    }
}

pub(super) struct PreparedLayer<'a> {
    opacity: f32,
    operators: Vec<&'a Operator>,
    root: Option<usize>,
    nodes: Vec<PreparedMask<'a>>,
}
enum PreparedMask<'a> {
    Shape(super::masks::Prepared<'a>),
    Color(&'a ColorRange),
    Constant(f32),
    Pair(Combine, usize, usize),
    Invert(usize),
}
impl PreparedLayer<'_> {
    pub fn apply(&self, rgb: [f32; 3], xy: [f32; 2]) -> [f32; 3] {
        let mut values = [0.; MAX_MASK_NODES];
        for (i, node) in self.nodes.iter().enumerate() {
            values[i] = match node {
                PreparedMask::Shape(m) => m.weight(xy, rgb),
                PreparedMask::Color(c) => c.weight(rgb),
                PreparedMask::Constant(v) => *v,
                PreparedMask::Invert(a) => 1. - values[*a],
                PreparedMask::Pair(op, a, b) => match op {
                    Combine::Add => 1. - (1. - values[*a]) * (1. - values[*b]),
                    Combine::Subtract => values[*a] * (1. - values[*b]),
                    Combine::Intersect => values[*a] * values[*b],
                },
            };
        }
        let w = self.opacity * self.root.map_or(1., |i| values[i]);
        if w == 0. {
            return rgb;
        }
        let mut edited = rgb;
        for o in &self.operators {
            edited = o.apply(edited, rgb);
        }
        std::array::from_fn(|c| rgb[c] + w * (edited[c] - rgb[c]))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskGraph {
    /// None is an all-white mask. Nodes are topologically ordered, children
    /// precede parents. This forbids cycles before any pixel work or recursion.
    pub root: Option<Uuid>,
    pub nodes: Vec<MaskNode>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskNode {
    pub id: Uuid,
    pub kind: MaskKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum MaskKind {
    Shape(Mask),
    Color(ColorRange),
    Constant(f32),
    Add(Uuid, Uuid),
    Subtract(Uuid, Uuid),
    Intersect(Uuid, Uuid),
    Invert(Uuid),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Add,
    Subtract,
    Intersect,
}
impl MaskKind {
    fn children(&self) -> Vec<Uuid> {
        match self {
            Self::Add(a, b) | Self::Subtract(a, b) | Self::Intersect(a, b) => vec![*a, *b],
            Self::Invert(a) => vec![*a],
            _ => vec![],
        }
    }
    fn remap(&mut self, ids: &HashMap<Uuid, Uuid>) {
        match self {
            Self::Add(a, b) | Self::Subtract(a, b) | Self::Intersect(a, b) => {
                *a = ids[a];
                *b = ids[b];
            }
            Self::Invert(a) => *a = ids[a],
            _ => (),
        }
    }
}
impl MaskGraph {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.nodes.len() <= MAX_MASK_NODES, "Maschera oltre quota");
        let mut depths = HashMap::new();
        for node in &self.nodes {
            ensure!(
                !node.id.is_nil() && !depths.contains_key(&node.id),
                "Identità maschera non valida"
            );
            let mut depth = 1;
            for child in node.kind.children() {
                let d = depths
                    .get(&child)
                    .ok_or_else(|| anyhow::anyhow!("Maschera ciclica o riferimento mancante"))?;
                depth = depth.max(d + 1);
            }
            ensure!(depth <= 16, "Maschera troppo profonda");
            match &node.kind {
                MaskKind::Shape(m) => {
                    m.validate()?;
                    ensure!(
                        m.enabled && m.exposure == 0. && m.warmth == 0. && m.saturation == 0.,
                        "La componente maschera contiene solo copertura"
                    );
                }
                MaskKind::Color(c) => c.validate()?,
                MaskKind::Constant(v) => range(*v, 0., 1., "Copertura")?,
                _ => (),
            }
            depths.insert(node.id, depth);
        }
        ensure!(
            self.root.is_none_or(|id| depths.contains_key(&id)),
            "Radice maschera mancante"
        );
        ensure!(
            self.root.is_some() || self.nodes.is_empty(),
            "Componenti senza radice"
        );
        Ok(())
    }
    pub fn renew_ids(&mut self) {
        let ids: HashMap<_, _> = self.nodes.iter().map(|n| (n.id, Uuid::new_v4())).collect();
        self.root = self.root.map(|id| ids[&id]);
        for n in &mut self.nodes {
            n.id = ids[&n.id];
            n.kind.remap(&ids);
        }
    }
    pub fn append(&mut self, kind: MaskKind, combine: Combine) -> Uuid {
        let leaf = Uuid::new_v4();
        self.nodes.push(MaskNode { id: leaf, kind });
        if let Some(root) = self.root {
            let id = Uuid::new_v4();
            let kind = match combine {
                Combine::Add => MaskKind::Add(root, leaf),
                Combine::Subtract => MaskKind::Subtract(root, leaf),
                Combine::Intersect => MaskKind::Intersect(root, leaf),
            };
            self.nodes.push(MaskNode { id, kind });
            self.root = Some(id);
        } else if combine == Combine::Subtract {
            let id = Uuid::new_v4();
            self.nodes.push(MaskNode {
                id,
                kind: MaskKind::Invert(leaf),
            });
            self.root = Some(id);
        } else {
            self.root = Some(leaf);
        }
        leaf
    }
    pub fn invert(&mut self) {
        let id = Uuid::new_v4();
        let kind = self.root.map_or(MaskKind::Constant(0.), MaskKind::Invert);
        self.nodes.push(MaskNode { id, kind });
        self.root = Some(id);
    }
    /// Delete a component, collapsing its Boolean parents without turning the
    /// rest of the tree into a bitmap. Any now-unreachable nodes are removed.
    pub fn remove(&mut self, id: Uuid) {
        let mut replacements = HashMap::<Uuid, Option<Uuid>>::new();
        for n in &mut self.nodes {
            if n.id == id {
                replacements.insert(n.id, None);
                continue;
            }
            let resolve = |id| replacements.get(&id).copied().unwrap_or(Some(id));
            match &mut n.kind {
                MaskKind::Add(a, b) | MaskKind::Subtract(a, b) | MaskKind::Intersect(a, b) => {
                    match (resolve(*a), resolve(*b)) {
                        (Some(x), Some(y)) => {
                            *a = x;
                            *b = y;
                        }
                        (x, y) => {
                            replacements.insert(n.id, x.or(y));
                        }
                    }
                }
                MaskKind::Invert(a) => {
                    if let Some(x) = resolve(*a) {
                        *a = x;
                    } else {
                        replacements.insert(n.id, None);
                    }
                }
                _ => (),
            }
        }
        self.root = self
            .root
            .and_then(|id| replacements.get(&id).copied().unwrap_or(Some(id)));
        self.nodes.retain(|n| !replacements.contains_key(&n.id));
        let mut needed: HashSet<_> = self.root.into_iter().collect();
        for n in self.nodes.iter().rev() {
            if needed.contains(&n.id) {
                needed.extend(n.kind.children());
            }
        }
        self.nodes.retain(|n| needed.contains(&n.id));
    }
    pub fn weight(&self, xy: [f32; 2], guide: [f32; 3], aspect: f32) -> f32 {
        // Bounded stack scratch, no allocations per sample. Validation precedes rendering.
        let mut values = [0.; MAX_MASK_NODES];
        let value = |id, upto: usize, v: &[f32]| {
            self.nodes[..upto]
                .iter()
                .position(|n| n.id == id)
                .map_or(0., |i| v[i])
        };
        for (i, n) in self.nodes.iter().enumerate() {
            let v = |id| value(id, i, &values);
            values[i] = match &n.kind {
                MaskKind::Shape(m) => m.weight(xy, guide, aspect),
                MaskKind::Color(c) => c.weight(guide),
                MaskKind::Constant(c) => *c,
                MaskKind::Add(a, b) => 1. - (1. - v(*a)) * (1. - v(*b)),
                MaskKind::Subtract(a, b) => v(*a) * (1. - v(*b)),
                MaskKind::Intersect(a, b) => v(*a) * v(*b),
                MaskKind::Invert(a) => 1. - v(*a),
            };
        }
        self.root
            .map_or(1., |id| value(id, self.nodes.len(), &values))
    }
}

/// Hue/chroma/luminance in extended linear Rec.2020, not perceptual HSL or Lab.
/// Circular hue; gray references select by chroma/Y without inventing a hue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorRange {
    pub reference: [f32; 3],
    pub width: [f32; 3],
    pub softness: f32,
}
impl Default for ColorRange {
    fn default() -> Self {
        Self {
            reference: [0.1, 0.2, 0.6],
            width: [40., 0.4, 0.4],
            softness: 0.5,
        }
    }
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn hue_delta(a: f32, b: f32) -> f32 {
    (a - b + 180.).rem_euclid(360.) - 180.
}
impl ColorRange {
    pub fn validate(&self) -> Result<()> {
        for c in self.reference {
            range(c, -65504., 65504., "Campione lineare")?;
        }
        range(self.width[0], 0.1, 180., "Intervallo tonalità")?;
        for w in &self.width[1..] {
            range(*w, 0.0001, 65504., "Intervallo cromia/luminanza")?;
        }
        range(self.softness, 0.01, 1., "Sfumatura colore")
    }
    pub fn weight(&self, rgb: [f32; 3]) -> f32 {
        let (h, c) = hue_chroma(rgb);
        let (rh, rc) = hue_chroma(self.reference);
        let falloff = |d: f32, width: f32| {
            1. - smooth((d.abs() / width - (1. - self.softness)) / self.softness)
        };
        let hue = if rc <= 1e-6 {
            1.
        } else {
            falloff(hue_delta(h, rh), self.width[0]) * smooth(c / (rc * 0.01).max(1e-6))
        };
        hue * falloff(c - rc, self.width[1])
            * falloff(luma(rgb) - luma(self.reference), self.width[2])
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleColor {
    pub range: ColorRange,
    /// Degrees, chroma percent, luminance EV/100.
    pub correction: [f32; 3],
    /// Percent of shortest hue arc, chroma difference, and luminance difference.
    pub uniformity: [f32; 3],
}
impl SampleColor {
    fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        if self.correction == [0.; 3] && self.uniformity == [0.; 3] {
            return rgb;
        }
        let weight = self.range.weight(guide);
        if weight == 0. {
            return rgb;
        }
        let (mut h, mut c) = hue_chroma(rgb);
        let (rh, rc) = hue_chroma(self.range.reference);
        let mut y = luma(rgb);
        if c > 1e-8 {
            if rc > 1e-6 {
                h += hue_delta(rh, h) * self.uniformity[0] / 100.;
            }
            h += self.correction[0];
            c += (rc - c) * self.uniformity[1] / 100.;
            c *= 1. + self.correction[1] / 100.;
        }
        y += (luma(self.range.reference) - y) * self.uniformity[2] / 100.;
        let gain = (self.correction[2] / 100.).exp2();
        let unit = hue_rgb(h);
        let uy = luma(unit);
        std::array::from_fn(|i| rgb[i] + weight * ((y + (unit[i] - uy) * c) * gain - rgb[i]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TonalLevel {
    pub black: f32,
    pub white: f32,
    pub gamma: f32,
    pub output: [f32; 2],
}
impl Default for TonalLevel {
    fn default() -> Self {
        Self {
            black: 0.,
            white: 1.,
            gamma: 1.,
            output: [0., 1.],
        }
    }
}
impl TonalLevel {
    fn apply(&self, v: f32) -> f32 {
        if *self == Self::default() {
            return v;
        }
        let x = (v - self.black) / (self.white - self.black);
        // Signed power: no clipping of negatives or HDR, no undefined pow.
        self.output[0]
            + x.signum() * x.abs().powf(1. / self.gamma) * (self.output[1] - self.output[0])
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Operator {
    SampleColor(SampleColor),
    Light {
        exposure: f32,
        temperature: f32,
        tint: f32,
        saturation: f32,
    },
    /// Composite RGB, R, G, B; monotone linear interpolation, process-4 tails.
    Curves([Vec<CurvePoint>; 4]),
    Levels([TonalLevel; 4]),
    Mixer(super::color::Color),
    Channels {
        matrix: [[f32; 3]; 3],
        offset: [f32; 3],
    },
    Grading(Grading),
    BlackAndWhite(BlackAndWhite),
    ColorFilter(ColorFilter),
    GradientMap(GradientMap),
    SelectiveColor(SelectiveColor),
    Colorize(Colorize),
    TonalAdjustments(TonalAdjustments),
    ExposureGamma(ExposureGamma),
    TonalLevels(Box<LevelAdjustments>),
    LuminanceCurve(LuminanceCurve),
    ParametricCurve(ParametricCurve),
}
pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub keywords: &'static str,
}
pub const TOOLS: &[Tool] = &[
    Tool {
        id: "sample",
        name: "Colore a campioni",
        description: "Correggi e uniforma un intervallo cromatico",
        keywords: "sample point color uniformity tinta hue dominante pelle",
    },
    Tool {
        id: "light",
        name: "Luce e colore locale",
        description: "Esposizione, temperatura, tinta e saturazione",
        keywords: "light exposure temperature tint saturation",
    },
    Tool {
        id: "curves",
        name: "Curve RGB a punti",
        description: "Curva composita e curve separate R/G/B",
        keywords: "curves rgb punti contrasto",
    },
    Tool {
        id: "levels",
        name: "Livelli tonali",
        description: "Nero, bianco, gamma e intervallo di uscita",
        keywords: "tonal levels black white gamma auto histogram istogramma contagocce picker neutral",
    },
    Tool {
        id: "mixer",
        name: "Mixer e monocromia",
        description: "Otto famiglie cromatiche e bianco e nero",
        keywords: "hsl mixer nero e bianco black white grading",
    },
    Tool {
        id: "channels",
        name: "Mixer dei canali",
        description: "Matrice RGB e offset per canale",
        keywords: "channel mixer matrix offset",
    },
    Tool {
        id: "grading",
        name: "Grading a quattro ruote",
        description: "Ombre, mezzitoni, luci e globale con bilanciamento",
        keywords: "color grading wheels ruote viraggio split toning balance ombre luci",
    },
    Tool {
        id: "black_white",
        name: "Mixer bianco e nero",
        description: "Conversione e luminosità di otto famiglie cromatiche",
        keywords: "black white monochrome bianco nero b&w bw mixer",
    },
    Tool {
        id: "filter",
        name: "Filtro cromatico",
        description: "Tinta, densità e conservazione della luminanza",
        keywords: "photo color filter warm cool filtro caldo freddo",
    },
    Tool {
        id: "gradient_map",
        name: "Mappa gradiente",
        description: "Associa la luminanza a una tavolozza modificabile",
        keywords: "gradient map duotone viraggio tavolozza sfumatura",
    },
    Tool {
        id: "selective_color",
        name: "Colore selettivo",
        description: "Componenti C/M/Y/K in nove famiglie, relativo o assoluto",
        keywords: "selective color colour cmyk cyan magenta yellow black relativo assoluto neutri",
    },
    Tool {
        id: "colorize",
        name: "Colorizza",
        description: "Tonalità comune, cromia e luminosità separate",
        keywords: "colorize colourize tint viraggio monocromia",
    },
    Tool {
        id: "tonal",
        name: "Controlli tonali",
        description: "Luce, contrasto, ombre, luci, bianchi e neri sul livello",
        keywords: "tonal tone exposure brightness contrast shadows highlights whites blacks pivot luminosità",
    },
    Tool {
        id: "exposure_gamma",
        name: "Esposizione, offset e gamma",
        description: "Guadagno, offset e potenza dei canali RGB",
        keywords: "exposure offset gamma technical tecnico",
    },
    Tool {
        id: "luminance_curve",
        name: "Curva di luminanza",
        description: "Curva a punti su Y, con differenze cromatiche conservate",
        keywords: "luminance luma curve tonal punti tono y",
    },
    Tool {
        id: "parametric_curve",
        name: "Curve parametriche",
        description: "Quattro zone e tre confini, luminanza o RGB",
        keywords: "parametric curves zones shadows highlights confini ombre luci",
    },
];
impl Tool {
    pub fn operator(&self) -> Operator {
        match self.id {
            "sample" => Operator::SampleColor(SampleColor::default()),
            "light" => Operator::Light {
                exposure: 0.,
                temperature: 0.,
                tint: 0.,
                saturation: 0.,
            },
            "curves" => Operator::Curves(Default::default()),
            "levels" => Operator::TonalLevels(Box::default()),
            "mixer" => Operator::Mixer(Default::default()),
            "channels" => Operator::Channels {
                matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                offset: [0.; 3],
            },
            "grading" => Operator::Grading(Grading::default()),
            "black_white" => Operator::BlackAndWhite(BlackAndWhite::default()),
            "filter" => Operator::ColorFilter(ColorFilter::default()),
            "gradient_map" => Operator::GradientMap(GradientMap::default()),
            "selective_color" => Operator::SelectiveColor(SelectiveColor::default()),
            "colorize" => Operator::Colorize(Colorize::default()),
            "tonal" => Operator::TonalAdjustments(TonalAdjustments::default()),
            "exposure_gamma" => Operator::ExposureGamma(ExposureGamma::default()),
            "luminance_curve" => Operator::LuminanceCurve(LuminanceCurve::default()),
            "parametric_curve" => Operator::ParametricCurve(ParametricCurve::default()),
            _ => unreachable!("static tool registry"),
        }
    }
}
impl Operator {
    pub fn tool(&self) -> &'static Tool {
        &TOOLS[match self {
            Self::SampleColor(_) => 0,
            Self::Light { .. } => 1,
            Self::Curves(_) => 2,
            Self::Levels(_) | Self::TonalLevels(_) => 3,
            Self::Mixer(_) => 4,
            Self::Channels { .. } => 5,
            Self::Grading(_) => 6,
            Self::BlackAndWhite(_) => 7,
            Self::ColorFilter(_) => 8,
            Self::GradientMap(_) => 9,
            Self::SelectiveColor(_) => 10,
            Self::Colorize(_) => 11,
            Self::TonalAdjustments(_) => 12,
            Self::ExposureGamma(_) => 13,
            Self::LuminanceCurve(_) => 14,
            Self::ParametricCurve(_) => 15,
        }]
    }
    pub fn is_neutral(&self) -> bool {
        match self {
            Self::SampleColor(s) => s.correction == [0.; 3] && s.uniformity == [0.; 3],
            Self::Curves(curves) => curves
                .iter()
                .all(|c| !super::curve_has_adjusted_endpoints(c) && c.iter().all(|p| p.x == p.y)),
            Self::Grading(g) => g.is_neutral(),
            Self::BlackAndWhite(g) => g.amount == 0.,
            Self::ColorFilter(g) => g.density == 0. || g.saturation == 0.,
            Self::GradientMap(g) => g.amount == 0.,
            Self::SelectiveColor(g) => g.is_neutral(),
            Self::Colorize(g) => g.amount == 0.,
            Self::TonalAdjustments(g) => g.is_neutral(),
            Self::ExposureGamma(g) => g.is_neutral(),
            Self::Levels(g) => *g == [TonalLevel::default(); 4],
            Self::TonalLevels(g) => g.is_neutral(),
            Self::LuminanceCurve(g) => g.is_neutral(),
            Self::ParametricCurve(g) => g.is_neutral(),
            _ => *self == self.tool().operator(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::SampleColor(s) => {
                s.range.validate()?;
                range(s.correction[0], -180., 180., "Correzione tonalità")?;
                for v in &s.correction[1..] {
                    range(*v, -100., 100., "Correzione cromatica")?;
                }
                for v in s.uniformity {
                    range(v, 0., 100., "Uniformità")?;
                }
            }
            Self::Light {
                exposure,
                temperature,
                tint,
                saturation,
            } => {
                range(*exposure, -10., 10., "Esposizione")?;
                for v in [temperature, tint, saturation] {
                    range(*v, -100., 100., "Colore locale")?;
                }
            }
            Self::Curves(curves) => {
                for curve in curves {
                    let mut r = super::EditRecipe::neutral(Default::default());
                    r.process_version = 4;
                    r.curve = curve.clone();
                    r.validate()?;
                }
            }
            Self::Levels(levels) => {
                for l in levels {
                    range(l.black, -16., 16., "Nero ingresso")?;
                    range(l.white, -16., 16., "Bianco ingresso")?;
                    ensure!(
                        l.white - l.black >= 0.001,
                        "Intervallo tonale troppo piccolo"
                    );
                    range(l.gamma, 0.1, 10., "Gamma")?;
                    for v in l.output {
                        range(v, -16., 16., "Livelli uscita")?;
                    }
                    ensure!(l.output[0] <= l.output[1], "Livelli uscita invertiti");
                }
            }
            Self::Mixer(m) => m.validate()?,
            Self::Grading(g) => g.validate()?,
            Self::BlackAndWhite(g) => g.validate()?,
            Self::ColorFilter(g) => g.validate()?,
            Self::GradientMap(g) => g.validate()?,
            Self::SelectiveColor(g) => g.validate()?,
            Self::Colorize(g) => g.validate()?,
            Self::TonalAdjustments(g) => g.validate()?,
            Self::ExposureGamma(g) => g.validate()?,
            Self::TonalLevels(g) => g.validate()?,
            Self::LuminanceCurve(g) => g.validate()?,
            Self::ParametricCurve(g) => g.validate()?,
            Self::Channels { matrix, offset } => {
                for v in matrix.iter().flatten().chain(offset) {
                    range(*v, -4., 4., "Mixer canali")?;
                }
            }
        }
        Ok(())
    }
    pub fn apply(&self, rgb: [f32; 3], guide: [f32; 3]) -> [f32; 3] {
        if self.is_neutral() {
            return rgb;
        }
        match self {
            Self::SampleColor(s) => s.apply(rgb, guide),
            Self::Light {
                exposure,
                temperature,
                tint,
                saturation,
            } => {
                let gain = exposure.exp2();
                let t = temperature / 100.;
                let m = tint / 100.;
                let gains = [
                    (0.18 * t + 0.07 * m).exp(),
                    (-0.14 * m).exp(),
                    (-0.18 * t + 0.07 * m).exp(),
                ];
                let v = std::array::from_fn(|c| rgb[c] * gain * gains[c]);
                let y = luma(v);
                v.map(|v| y + (v - y) * (1. + saturation / 100.))
            }
            Self::Curves(c) => std::array::from_fn(|i| {
                super::tone_curve_value(super::tone_curve_value(rgb[i], &c[0]), &c[i + 1])
            }),
            Self::Levels(l) => std::array::from_fn(|i| l[i + 1].apply(l[0].apply(rgb[i]))),
            Self::Mixer(m) => m.apply(rgb),
            Self::Grading(g) => g.apply(rgb, guide),
            Self::BlackAndWhite(g) => g.apply(rgb, guide),
            Self::ColorFilter(g) => g.apply(rgb),
            Self::GradientMap(g) => g.apply(rgb),
            Self::SelectiveColor(g) => g.apply(rgb, guide),
            Self::Colorize(g) => g.apply(rgb),
            Self::TonalAdjustments(g) => g.apply(rgb, guide),
            Self::ExposureGamma(g) => g.apply(rgb),
            Self::TonalLevels(g) => g.apply(rgb),
            Self::LuminanceCurve(g) => g.apply(rgb),
            Self::ParametricCurve(g) => g.apply(rgb),
            Self::Channels { matrix, offset } => std::array::from_fn(|i| {
                (0..3).map(|j| matrix[i][j] * rgb[j]).sum::<f32>() + offset[i]
            }),
        }
    }
}
