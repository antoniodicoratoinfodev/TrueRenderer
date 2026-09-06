pub mod color;
pub mod corpus;
pub mod protocol;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Label {
    #[default]
    None,
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}
impl Label {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Red,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
    ];
    pub fn text(self) -> &'static str {
        match self {
            Self::None => "Nessuna",
            Self::Red => "Rosso",
            Self::Yellow => "Giallo",
            Self::Green => "Verde",
            Self::Blue => "Blu",
            Self::Purple => "Viola",
        }
    }
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub rating: i8,
    pub label: Label,
    pub keywords: Vec<String>,
}
impl Annotation {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (-1..=5).contains(&self.rating),
            "Valutazione valida: scartato (-1), oppure 0–5"
        );
        ensure!(
            self.keywords.len() <= 64,
            "Massimo 64 parole chiave per immagine"
        );
        ensure!(
            self.keywords
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)),
            "Parola chiave non valida (massimo 128 byte)"
        );
        Ok(())
    }
    pub fn set_keywords(&mut self, text: &str) -> Result<()> {
        let mut keys: Vec<String> = text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        keys.sort_by_key(|s| s.to_lowercase());
        keys.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
        let proposed = Self {
            keywords: keys,
            ..self.clone()
        };
        proposed.validate()?;
        *self = proposed;
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub path: std::path::PathBuf,
    pub name: String,
    pub bytes: u64,
    pub digest: String,
    pub approved: bool,
    pub annotation: Annotation,
    pub revision: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    Grid,
    Preview,
    Compare,
}
#[derive(Debug, Clone, Copy)]
pub struct ViewTransform {
    pub zoom: Option<f32>,
    pub center: [f32; 2],
}
impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            zoom: None,
            center: [0.5, 0.5],
        }
    }
}
impl ViewTransform {
    pub fn scale(&self, image: [f32; 2], viewport: [f32; 2], pixels_per_point: f32) -> f32 {
        self.zoom
            .map(|z| z / pixels_per_point)
            .unwrap_or_else(|| (viewport[0] / image[0]).min(viewport[1] / image[1]))
    }
    pub fn set_zoom(&mut self, zoom: f32) {
        if zoom.is_finite() {
            self.zoom = Some(zoom.clamp(0.01, 32.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_one_to_one_on_retina() {
        let transform = ViewTransform {
            zoom: Some(1.0),
            ..Default::default()
        };
        assert_eq!(transform.scale([1200., 800.], [1000., 800.], 2.), 0.5);
    }
    #[test]
    fn keywords_and_rating_are_bounded() {
        let mut a = Annotation::default();
        a.set_keywords(" Paesaggio, test, paesaggio ,, ").unwrap();
        assert_eq!(a.keywords.len(), 2);
        a.rating = 6;
        assert!(a.validate().is_err());
    }
}
