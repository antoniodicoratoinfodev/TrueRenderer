//! Independently owned mip tails. No reference to the original full pyramid.
use crate::{
    budget::Lease,
    color::LinearImage,
    preview::PreviewRequest,
    resample::{Pyramid, Region},
};
use anyhow::{Result, ensure};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// This first adapter develops a bounded full frame; it does not claim regional
/// RAW decoding, native reduced RAW qualification, streaming or gigapixel input.
#[derive(Clone, Copy, Debug)]
pub struct ProviderCapabilities {
    pub full_frame_decode: bool,
    pub regional_decode: bool,
    pub reduced_raw: bool,
}
pub const CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    full_frame_decode: true,
    regional_decode: false,
    reduced_raw: false,
};

pub struct ImageLevels {
    id: u64,
    source_size: [u32; 2],
    base: u32,
    opaque: bool,
    levels: Vec<LinearImage>,
    // The owner of the samples also owns the credits, through renderer/writer jobs.
    lease: Option<Arc<Lease>>,
}
impl ImageLevels {
    /// Generate the same reference graph, dropping expensive ancestors as soon
    /// as the next level is complete. No reduced decode is claimed by this adapter.
    pub fn from_source(mut source: LinearImage, request: PreviewRequest) -> Result<Self> {
        request.validate()?;
        let source_size = [source.width, source.height];
        let opaque = source.pixels.iter().all(|p| p[3] == 1.);
        let mut base = 0;
        while source.width.max(source.height) > request.maximum_level_edge() {
            let size = [source.width.div_ceil(2), source.height.div_ceil(2)];
            source = crate::resample::filter(
                &source,
                Region::fitted([source.width, source.height], size),
                opaque,
            )?;
            base += 1;
        }
        let mut levels = vec![source];
        while levels.last().is_some_and(|l| l.width > 1 || l.height > 1) {
            let source = levels.last().unwrap();
            let size = [source.width.div_ceil(2), source.height.div_ceil(2)];
            levels.push(crate::resample::filter(
                source,
                Region::fitted([source.width, source.height], size),
                opaque,
            )?);
        }
        Self::restore(source_size, base, opaque, levels)
    }
    pub fn from_pyramid(pyramid: Pyramid, request: PreviewRequest) -> Result<Self> {
        request.validate()?;
        let source = [pyramid.source().width, pyramid.source().height];
        let opaque = pyramid.source().pixels.iter().all(|p| p[3] == 1.);
        let mut levels = pyramid.into_levels();
        let base = levels
            .iter()
            .position(|l| l.width.max(l.height) <= request.maximum_level_edge())
            .unwrap_or(0);
        levels.drain(..base);
        Self::restore(source, base as u32, opaque, levels)
    }
    pub fn restore(
        source_size: [u32; 2],
        base: u32,
        opaque: bool,
        levels: Vec<LinearImage>,
    ) -> Result<Self> {
        ensure!(
            source_size[0] > 0
                && source_size[1] > 0
                && source_size[0] as u64 * source_size[1] as u64 <= crate::color::MAX_PIXELS as u64
                && base < 32,
            "Géometria del derivato non valida"
        );
        let mut size = source_size;
        for _ in 0..base {
            ensure!(size != [1, 1], "Livello derivato ridondante");
            size = [size[0].div_ceil(2), size[1].div_ceil(2)];
        }
        ensure!(
            levels.first().is_some_and(|l| [l.width, l.height] == size),
            "Origine mip incoerente"
        );
        // The existing complete-chain validator still applies to the owned tail.
        let pyramid = Pyramid::from_levels(levels)?;
        ensure!(
            !opaque
                || pyramid
                    .levels()
                    .iter()
                    .all(|l| l.pixels.iter().all(|p| p[3] == 1.)),
            "Alpha opaca incoerente"
        );
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            source_size,
            base,
            opaque,
            levels: pyramid.into_levels(),
            lease: None,
        })
    }
    pub fn attach_lease(&mut self, lease: Lease) {
        assert!(self.lease.is_none() && lease.bytes() >= self.byte_len() as u64);
        self.lease = Some(Arc::new(lease));
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn source_size(&self) -> [u32; 2] {
        self.source_size
    }
    pub fn base_level(&self) -> u32 {
        self.base
    }
    pub fn opaque(&self) -> bool {
        self.opaque
    }
    pub fn source(&self) -> &LinearImage {
        &self.levels[0]
    }
    pub fn levels(&self) -> &[LinearImage] {
        &self.levels
    }
    pub fn byte_len(&self) -> usize {
        self.levels.iter().map(|l| l.pixels.len() * 16).sum()
    }
    pub fn sufficient_for(&self, request: PreviewRequest) -> bool {
        let mut size = self.source_size;
        let mut required = 0;
        while size[0].max(size[1]) > request.maximum_level_edge() {
            size = [size[0].div_ceil(2), size[1].div_ceil(2)];
            required += 1;
        }
        self.base <= required
    }
    pub fn requested_base(&self, request: PreviewRequest) -> usize {
        self.levels
            .iter()
            .position(|l| l.width.max(l.height) <= request.maximum_level_edge())
            .unwrap_or(0)
    }
    pub fn detach(&self, request: PreviewRequest, lease: Lease) -> Result<Self> {
        request.validate()?;
        let start = self.requested_base(request);
        ensure!(self.sufficient_for(request), "Dettaglio richiesto assente");
        ensure!(
            lease.bytes()
                >= self.levels[start..]
                    .iter()
                    .map(|l| l.pixels.len() as u64 * 16)
                    .sum(),
            "Derivato senza crediti"
        );
        let mut image = Self::restore(
            self.source_size,
            self.base + start as u32,
            self.opaque,
            self.levels[start..].to_vec(),
        )?;
        image.attach_lease(lease);
        Ok(image)
    }
    pub fn supports(&self, region: Region) -> bool {
        let level = self.source();
        region.step[0] * level.width as f64 / self.source_size[0] as f64 >= 0.5
            && region.step[1] * level.height as f64 / self.source_size[1] as f64 >= 0.5
    }
    pub fn render_input(&self, region: Region) -> Result<(&LinearImage, Region, bool)> {
        ensure!(
            region.size[0] as u64 * region.size[1] as u64
                <= crate::color::MAX_PRESENTATION_PIXELS as u64,
            "Viewport oltre quota"
        );
        let mut index = 0;
        while index + 1 < self.levels.len() {
            let current = &self.levels[index];
            if region.step[0] * current.width as f64 / self.source_size[0] as f64 <= 2.
                && region.step[1] * current.height as f64 / self.source_size[1] as f64 <= 2.
            {
                break;
            }
            index += 1;
        }
        let level = &self.levels[index];
        let ratio = [
            level.width as f64 / self.source_size[0] as f64,
            level.height as f64 / self.source_size[1] as f64,
        ];
        Ok((
            level,
            Region {
                size: region.size,
                origin: [region.origin[0] * ratio[0], region.origin[1] * ratio[1]],
                step: [region.step[0] * ratio[0], region.step[1] * ratio[1]],
            },
            self.opaque,
        ))
    }
    pub fn render(&self, region: Region) -> Result<LinearImage> {
        let (level, region, opaque) = self.render_input(region)?;
        crate::resample::filter(level, region, opaque)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::PreviewQuality;
    #[test]
    fn detached_full_thumbnail_matches_reference_after_source_is_dropped() {
        let source = LinearImage::new(
            513,
            257,
            (0..513 * 257)
                .map(|i| [i as f32 / 10000. - 1., 0.2, 0.8, 1.])
                .collect(),
        )
        .unwrap();
        let reference = Pyramid::new(source).unwrap();
        let region = Region::fitted([513, 257], [81, 41]);
        let expected = reference.render(region).unwrap();
        let original_bytes = reference.byte_len();
        let artifact = ImageLevels::from_pyramid(
            reference,
            PreviewRequest {
                quality: PreviewQuality::Full,
                edge: 81,
            },
        )
        .unwrap();
        assert!(artifact.base_level() > 0);
        assert!(!artifact.sufficient_for(PreviewRequest::full()));
        let larger_view = PreviewRequest {
            quality: PreviewQuality::Full,
            edge: 257,
        };
        assert!(!artifact.sufficient_for(larger_view));
        let memory = crate::budget::MemoryBudget::new(artifact.byte_len() as u64);
        assert!(
            artifact
                .detach(
                    larger_view,
                    memory.try_reserve(artifact.byte_len() as u64).unwrap()
                )
                .is_err()
        );
        assert!(artifact.byte_len() < original_bytes / 4);
        assert_eq!(artifact.render(region).unwrap().pixels, expected.pixels);
    }
    #[test]
    fn geometry_and_quality_do_not_promote_reduced_samples_to_native() {
        assert!(
            ImageLevels::restore(
                [10, 10],
                1,
                true,
                vec![LinearImage::new(1, 1, vec![[1.; 4]]).unwrap()]
            )
            .is_err()
        );
        assert_eq!(PreviewRequest::full().maximum_level_edge(), u32::MAX);
        assert_eq!(
            PreviewRequest {
                quality: PreviewQuality::Standard,
                edge: 0
            }
            .maximum_level_edge(),
            2048
        );
    }
}
