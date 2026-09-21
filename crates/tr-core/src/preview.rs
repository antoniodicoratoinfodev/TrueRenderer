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

/// Bounded physical-pixel buckets shared by thumbnail demand and disk lookup.
/// Power-of-two rounding followed by the quality's 2x margin can retain an
/// unnecessary ancestor (four times the pixels) in a large grid cell.
pub const THUMBNAIL_EDGE_STEP: u32 = 128;
pub fn thumbnail_edge(physical_edge: u32) -> u32 {
    physical_edge.clamp(1, 4096).div_ceil(THUMBNAIL_EDGE_STEP) * THUMBNAIL_EDGE_STEP
}

/// `edge == 0` requests native detail; nonzero edges describe the physical cell.
/// A full-quality thumbnail preserves the reference mip graph, not LOD 0 residency.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    #[serde(default)]
    pub raw_engine: crate::decoder::RawEngine,
    pub quality: PreviewQuality,
    pub edge: u32,
}
impl PreviewRequest {
    pub fn full() -> Self {
        Self {
            raw_engine: crate::decoder::RawEngine::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_buckets_bound_oversampling_without_losing_physical_detail() {
        assert_eq!(thumbnail_edge(536), 640);
        assert_eq!(thumbnail_edge(224), 256);
        assert_eq!(thumbnail_edge(u32::MAX), 4096);
        for physical in 1..=4096 {
            let edge = thumbnail_edge(physical);
            assert!(edge >= physical && edge - physical < THUMBNAIL_EDGE_STEP);
            assert_eq!(edge % THUMBNAIL_EDGE_STEP, 0);
        }
        // D750: a 536px cell needs the 752px reference level, not 1504px.
        let request = PreviewRequest {
            edge: thumbnail_edge(536),
            ..PreviewRequest::full()
        };
        assert_eq!(
            crate::protocol::mip_geometry([6016, 4016], request.maximum_level_edge()),
            ([752, 502], 3)
        );
    }
}
