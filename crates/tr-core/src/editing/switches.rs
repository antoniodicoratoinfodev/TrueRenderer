//! Persisted section bypass. Parked values never enter CPU/GPU evaluation.
use super::EditRecipe;
use anyhow::{Result, ensure};
use serde_json::Value;
use std::collections::BTreeSet;

fn paths(id: &str) -> Option<&'static [&'static str]> {
    Some(match id {
        "raw-wb-panel" => &["/raw_wb"],
        "rgb-wb-panel" => &["/temperature", "/tint"],
        "light-panel" => &[
            "/exposure_ev",
            "/brightness",
            "/contrast",
            "/highlights",
            "/shadows",
            "/whites",
            "/blacks",
        ],
        "curve-panel" => &["/curve"],
        "color-panel" => &["/saturation", "/vibrance", "/protect_warm"],
        "geometry" => &[
            "/advanced/geometry/crop",
            "/advanced/geometry/quarter_turns",
            "/advanced/geometry/flip_horizontal",
            "/advanced/geometry/flip_vertical",
            "/advanced/geometry/angle",
            "/advanced/geometry/perspective",
            "/advanced/geometry/scale",
        ],
        "optics" => &[
            "/advanced/geometry/distortion",
            "/advanced/geometry/moustache",
            "/advanced/geometry/vignette",
            "/advanced/geometry/ca",
            "/advanced/geometry/defringe",
        ],
        "detail" => &["/advanced/detail"],
        "advanced-color" => &["/advanced/color"],
        "local-masks" => &["/advanced/masks"],
        _ => return None,
    })
}

fn document(recipe: &EditRecipe) -> Result<Value> {
    let mut value = serde_json::to_value(recipe)?;
    value["raw_wb"] = serde_json::to_value(recipe.raw_wb)?;
    value["vibrance"] = recipe.vibrance.into();
    value["protect_warm"] = recipe.protect_warm.into();
    value["advanced"] = serde_json::to_value(recipe.advanced.clone().unwrap_or_default())?;
    Ok(value)
}

impl EditRecipe {
    pub fn set_section_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let fields = paths(id).ok_or_else(|| anyhow::anyhow!("Strumento sconosciuto"))?;
        if enabled == !self.suspended.contains_key(id) {
            return Ok(());
        }
        let had_advanced = self.advanced.is_some();
        let mut value = document(self)?;
        if enabled {
            let stored = self.suspended.get(id).unwrap();
            for path in fields {
                *value.pointer_mut(path).unwrap() = stored[*path].clone();
            }
        } else {
            let neutral = document(&EditRecipe::neutral(self.raw_engine))?;
            let mut stored = serde_json::Map::new();
            for path in fields {
                stored.insert((*path).into(), value.pointer(path).unwrap().clone());
                *value.pointer_mut(path).unwrap() = neutral.pointer(path).unwrap().clone();
            }
            value["suspended"][id] = Value::Object(stored);
        }
        if enabled {
            value["suspended"].as_object_mut().unwrap().remove(id);
        }
        if !had_advanced {
            value.as_object_mut().unwrap().remove("advanced");
        }
        let candidate: Self = serde_json::from_value(value)?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn expanded_sections(&self) -> Result<(Self, BTreeSet<String>)> {
        let ids: BTreeSet<_> = self.suspended.keys().cloned().collect();
        let mut expanded = self.clone();
        for id in &ids {
            expanded.set_section_enabled(id, true)?;
        }
        Ok((expanded, ids))
    }

    pub(super) fn validate_switches(&self) -> Result<()> {
        if self.suspended.is_empty() {
            return Ok(());
        }
        ensure!(self.suspended.len() <= 10, "Troppi strumenti disattivati");
        let mut value = document(self)?;
        value.as_object_mut().unwrap().remove("suspended");
        let neutral = document(&Self::neutral(self.raw_engine))?;
        for (id, stored) in &self.suspended {
            let fields =
                paths(id).ok_or_else(|| anyhow::anyhow!("Strumento sospeso sconosciuto"))?;
            ensure!(
                stored.as_object().is_some_and(|o| o.len() == fields.len()),
                "Parametri sospesi non validi"
            );
            for path in fields {
                ensure!(
                    value.pointer(path) == neutral.pointer(path),
                    "Strumento disattivato con parametri eseguibili non neutri"
                );
                let v = stored
                    .get(*path)
                    .ok_or_else(|| anyhow::anyhow!("Parametro sospeso mancante"))?;
                *value.pointer_mut(path).unwrap() = v.clone();
            }
        }
        if self.advanced.is_none() {
            let advanced: super::Advanced = serde_json::from_value(value["advanced"].clone())?;
            advanced.validate()?;
            ensure!(
                advanced.is_neutral(),
                "Parametri avanzati sospesi senza processo avanzato"
            );
            value.as_object_mut().unwrap().remove("advanced");
        }
        serde_json::from_value::<Self>(value)?.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switch_roundtrip_preserves_values_and_render() {
        let mut r = EditRecipe::neutral(crate::decoder::RawEngine::default());
        r.exposure_ev = 1.25;
        let original = r.clone();
        r.set_section_enabled("light-panel", false).unwrap();
        assert_eq!(r.exposure_ev, 0.);
        assert!(r.is_neutral());
        let bytes = serde_json::to_vec(&r).unwrap();
        let mut reopened: EditRecipe = serde_json::from_slice(&bytes).unwrap();
        reopened.validate().unwrap();
        reopened.set_section_enabled("light-panel", true).unwrap();
        assert_eq!(original, reopened);
    }
    #[test]
    fn advanced_sections_restore_in_any_order_without_losing_optics() {
        let mut recipe = EditRecipe::neutral(Default::default());
        recipe.require_process(3);
        let a = recipe.advanced.get_or_insert_default();
        a.geometry.angle = 12.;
        a.geometry.distortion = 7.;
        a.detail.texture = 15.;
        a.color.monochrome = true;
        let original = recipe.clone();
        for id in ["geometry", "optics", "detail", "advanced-color"] {
            recipe.set_section_enabled(id, false).unwrap();
        }
        recipe.validate().unwrap();
        assert!(recipe.is_neutral());
        for id in ["optics", "advanced-color", "geometry", "detail"] {
            recipe.set_section_enabled(id, true).unwrap();
        }
        assert_eq!(recipe, original);
        let mut invalid = EditRecipe::neutral(Default::default());
        invalid
            .suspended
            .insert("unknown".into(), serde_json::json!({}));
        assert!(invalid.validate().is_err());
    }
}
