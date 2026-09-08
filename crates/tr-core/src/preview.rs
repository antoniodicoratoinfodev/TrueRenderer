//! Preview quality is independent of pipeline assurance and source resolution.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ImageCompute {
    #[default]
    Automatic,
    Cpu,
    Gpu,
}
impl ImageCompute {
    pub fn code(self) -> u8 {
        match self {
            Self::Automatic => 0,
            Self::Cpu => 1,
            Self::Gpu => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PreviewQuality {
    #[default]
    Standard,
    Full,
}
impl PreviewQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Anteprima standard",
            Self::Full => "Anteprima a qualità piena",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completeness {
    Refining,
    Complete,
}

/// `edge == 0` requests native detail; nonzero edges describe the physical cell.
/// A full-quality thumbnail preserves the reference mip graph, not LOD 0 residency.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub quality: PreviewQuality,
    pub edge: u32,
}
impl PreviewRequest {
    pub fn full() -> Self {
        Self {
            quality: PreviewQuality::Full,
            edge: 0,
        }
    }
    pub fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(self.edge <= 4096, "Dimensione anteprima fuori quota");
        Ok(())
    }
    pub fn maximum_level_edge(self) -> u32 {
        match (self.quality, self.edge) {
            (PreviewQuality::Full, 0) => u32::MAX,
            (PreviewQuality::Full, edge) => edge.saturating_mul(2),
            (PreviewQuality::Standard, 0) => 2048,
            (PreviewQuality::Standard, edge) => edge.saturating_mul(2).min(2048),
        }
    }
}

/// Work priority, separate from image identity and quality. Equal priorities
/// retain FIFO order. Nearby regions are used only by capable providers.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PreviewPriority {
    Immediate,
    Refinement,
    AheadRegion,
    SecondaryVisible,
    AdjacentRows,
    NeighborPreview,
    Background,
}
impl PreviewPriority {
    pub fn visible(self) -> bool {
        matches!(
            self,
            Self::Immediate | Self::Refinement | Self::SecondaryVisible
        )
    }
}
