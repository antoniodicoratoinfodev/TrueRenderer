//! Portable layer parameters. No source binding, decoder settings or geometry.
use super::{
    EditRecipe,
    layers::{Id, Layer, Operator},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_LOOK_BYTES: usize = 48 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub version: u32,
    pub layers: Vec<Layer>,
}

fn selection<'a>(layers: &'a [Layer], ids: &[Id]) -> Result<Vec<&'a Layer>> {
    let selected: HashSet<_> = ids.iter().copied().collect();
    ensure!(
        !selected.is_empty() && selected.len() == ids.len(),
        "Selezione look vuota o duplicata"
    );
    let result: Vec<_> = layers.iter().filter(|l| selected.contains(&l.id)).collect();
    ensure!(
        result.len() == ids.len(),
        "Livello del look non disponibile"
    );
    Ok(result)
}

impl Look {
    pub fn capture(recipe: &EditRecipe, ids: &[Id], masks: bool) -> Result<Self> {
        recipe.validate()?;
        let layers = recipe
            .layers
            .as_ref()
            .map_or(&[][..], |s| s.layers.as_slice());
        let layers = selection(layers, ids)?
            .into_iter()
            .map(|l| {
                let mut copy = l.duplicate();
                copy.locked = false;
                if !masks {
                    copy.mask = Default::default();
                }
                for op in &mut copy.operators {
                    if let Operator::TonalLevels(levels) = op {
                        levels.analysis = None;
                    }
                }
                copy
            })
            .collect();
        let look = Self { version: 1, layers };
        look.validate()?;
        Ok(look)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Versione look non supportata");
        ensure!(!self.layers.is_empty(), "Look senza livelli");
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_LOOK_BYTES,
            "Look oltre 48 KiB"
        );
        let mut recipe = EditRecipe::neutral(Default::default());
        recipe.layer_stack().layers.clone_from(&self.layers);
        recipe.validate()?;
        ensure!(
            self.layers.iter().flat_map(|l| &l.operators).all(|op| {
                !matches!(op, Operator::TonalLevels(levels) if levels.analysis.is_some())
            }),
            "Il look contiene riferimenti a una foto"
        );
        Ok(())
    }

    /// Independent append, preserving order and every destination base setting.
    /// The returned recipe is validated as a whole before the caller can commit.
    pub fn append_to(
        &self,
        destination: &EditRecipe,
        ids: &[Id],
        amount: f32,
        masks: bool,
    ) -> Result<EditRecipe> {
        self.validate()?;
        destination.validate()?;
        ensure!(
            amount.is_finite() && (0. ..=1.).contains(&amount),
            "Intensità look non valida"
        );
        let chosen = selection(&self.layers, ids)?;
        if amount == 0. {
            return Ok(destination.clone());
        }
        let mut result = destination.clone();
        for layer in chosen {
            let mut copy = layer.duplicate();
            copy.locked = false;
            copy.opacity *= amount;
            if !masks {
                copy.mask = Default::default();
            }
            result.layer_stack().layers.push(copy);
        }
        result.validate()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::{Advanced, layers::*, masks::Mask};

    fn source() -> EditRecipe {
        let mut r = EditRecipe::neutral(Default::default());
        r.exposure_ev = 2.;
        r.process_version = 4;
        r.advanced = Some(Box::new(Advanced::default()));
        r.advanced.as_mut().unwrap().geometry.crop = [0.1, 0.2, 0.8, 0.9];
        let mut level = LevelAdjustments::default();
        level.channels[0].gamma = 1.2;
        level.analysis = Some(LevelAnalysis {
            action: LevelsAction::AutoComposite,
            channel: 0,
            source_digest: "a".repeat(64),
            input_recipe_hash: "b".repeat(64),
            geometry: Default::default(),
            image_size: [100, 100],
            valid_samples: 100,
            considered_samples: 100,
            sample_xy: None,
            sample_rgb: None,
        });
        let mut layer = Layer::new("Finish", Operator::TonalLevels(Box::new(level)));
        layer.locked = true;
        layer.opacity = 0.8;
        layer
            .mask
            .append(MaskKind::Shape(Mask::default()), Combine::Add);
        layer
            .mask
            .append(MaskKind::Constant(0.2), Combine::Subtract);
        r.layer_stack().layers.push(layer);
        r.layer_stack().layers.push(Layer::new(
            "Curve",
            Operator::ParametricCurve(ParametricCurve {
                amounts: [10., 0., 0., 0.],
                ..Default::default()
            }),
        ));
        r
    }
    #[test]
    fn captures_ordered_independent_parameters_without_photo_analysis_or_base() {
        let r = source();
        let before = r.clone();
        let layers = &r.layers.as_ref().unwrap().layers;
        let look = Look::capture(&r, &[layers[1].id, layers[0].id], true).unwrap();
        assert_eq!(
            look.layers.iter().map(|l| &l.name).collect::<Vec<_>>(),
            vec!["Finish", "Curve"]
        );
        assert_eq!(r, before);
        assert!(!look.layers[0].locked);
        assert_ne!(look.layers[0].id, layers[0].id);
        assert_ne!(look.layers[0].mask.nodes[0].id, layers[0].mask.nodes[0].id);
        let Operator::TonalLevels(g) = &look.layers[0].operators[0] else {
            panic!()
        };
        assert!(g.analysis.is_none());
        assert_eq!(g.channels[0].gamma, 1.2);
        let json = serde_json::to_string(&look).unwrap();
        assert!(
            !json.contains("source_digest")
                && !json.contains("raw_engine")
                && !json.contains("geometry")
        );
        let loaded: Look = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, look);
        loaded.validate().unwrap();
        let without_masks = Look::capture(&r, &[layers[0].id], false).unwrap();
        assert_eq!(without_masks.layers[0].mask, MaskGraph::default());
    }
    #[test]
    fn append_preserves_historical_base_and_allocates_independent_layer_and_mask_ids() {
        let r = source();
        let look = Look::capture(&r, &[r.layers.as_ref().unwrap().layers[0].id], true).unwrap();
        let ids = [look.layers[0].id];
        for process in 1..=4 {
            let mut destination = EditRecipe::neutral(crate::decoder::RawEngine::TrueRenderer);
            destination.process_version = process;
            destination.exposure_ev = 0.25;
            let before = destination.clone();
            let result = look.append_to(&destination, &ids, 0.5, true).unwrap();
            assert_eq!(destination, before);
            let mut base = result.clone();
            base.layers = None;
            base.process_version = process;
            base.schema_version = 1;
            assert_eq!(base, destination);
            assert_eq!(result.base_process(), process);
            let added = &result.layers.as_ref().unwrap().layers[0];
            assert_eq!(added.opacity, 0.4);
            assert_ne!(added.id, look.layers[0].id);
            for rgb in [[-0.1, 0.2, 1.5], [0.2, 0.4, 0.7]] {
                let a = added.apply(rgb, [0.5, 0.4], 1.5);
                let b = look.layers[0].apply(rgb, [0.5, 0.4], 1.5);
                for i in 0..3 {
                    assert!((a[i] - (rgb[i] + 0.5 * (b[i] - rgb[i]))).abs() < 1e-6);
                }
            }
            let twice = look.append_to(&result, &ids, 1., true).unwrap();
            let stack = twice.layers.unwrap();
            stack.validate().unwrap();
            assert_ne!(stack.layers[0].id, stack.layers[1].id);
            assert_ne!(stack.layers[0].mask.root, stack.layers[1].mask.root);
            assert_eq!(
                look.append_to(&destination, &ids, 0., true).unwrap(),
                destination
            );
            assert!(
                look.append_to(&destination, &ids, 1., false)
                    .unwrap()
                    .layers
                    .unwrap()
                    .layers[0]
                    .mask
                    .nodes
                    .is_empty()
            );
        }
    }
    #[test]
    fn invalid_look_selection_versions_and_combined_quotas_are_atomic() {
        let mut r = source();
        let id = r.layers.as_ref().unwrap().layers[0].id;
        assert!(Look::capture(&r, &[], false).is_err());
        assert!(Look::capture(&r, &[id, id], false).is_err());
        assert!(Look::capture(&r, &[Id::new_v4()], false).is_err());
        let look = Look::capture(&r, &[id], true).unwrap();
        let ids = [look.layers[0].id];
        for amount in [f32::NAN, -0.1, 1.1] {
            assert!(look.append_to(&r, &ids, amount, true).is_err());
        }
        let mut bad = look.clone();
        bad.version = 2;
        assert!(bad.validate().is_err());
        bad = look.clone();
        bad.layers[0].operators[0] = r.layers.as_ref().unwrap().layers[0].operators[0].clone();
        assert!(
            bad.validate().is_err(),
            "foreign photo provenance is not portable"
        );
        while r.layer_stack().layers.len() < MAX_LAYERS {
            let copy = r.layer_stack().layers[1].duplicate();
            r.layer_stack().layers.push(copy);
        }
        let before = r.clone();
        assert!(look.append_to(&r, &ids, 1., true).is_err());
        assert_eq!(r, before);
        assert!(serde_json::from_str::<Look>("{\"layers\":[]}").is_err());
        assert!(
            serde_json::from_str::<Look>("{\"version\":1,\"layers\":[],\"future\":true}").is_err()
        );
    }
}
