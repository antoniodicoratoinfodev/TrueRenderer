//! Reuse the spatial basis while only texture/clarity/sharpness amount changes.
use super::*;

pub(super) struct Checkpoint {
    identity: Identity,
    prefix: Vec<Stage>,
    key: Detail,
    basis: Vec<Pixel>,
    pub(super) lease: Lease,
}

fn parameters(d: &Detail) -> Option<(Detail, f32)> {
    if d.luminance_noise != 0. || d.chroma_noise != 0. || d.dehaze != 0. {
        return None;
    }
    if [d.texture, d.clarity, d.sharpen]
        .iter()
        .filter(|v| **v != 0.)
        .count()
        != 1
    {
        return None;
    }
    let mut key = d.clone();
    let value = if d.texture != 0. {
        &mut key.texture
    } else if d.clarity != 0. {
        &mut key.clarity
    } else {
        &mut key.sharpen
    };
    let amount = *value / 100.;
    *value = 1.;
    Some((key, amount))
}
pub(super) fn supported(stage: &Stage) -> bool {
    matches!(stage, Stage::Detail(d) if parameters(d).is_some())
}

impl Processor {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_spatial(
        &mut self,
        detail: &Detail,
        prefix: &[Stage],
        identity: Identity,
        image: &mut LinearImage,
        full: [u32; 2],
        native: [u32; 2],
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<()> {
        let (key, amount) = parameters(detail).expect("single spatial node");
        let hit = self
            .spatial
            .as_ref()
            .is_some_and(|s| s.identity == identity && s.prefix == prefix && s.key == key);
        if !hit {
            self.spatial = None;
            let bytes = image.pixels.len() as u64 * 16;
            if let Some(lease) = self.reserve_cache(
                bytes,
                (self.memory.usage().limit / 32).min(128 * 1024 * 1024),
            ) {
                let scale = full[0].max(full[1]) as f32 / native[0].max(native[1]) as f32;
                let mut basis = if key.sharpen != 0. {
                    detail::gaussian(image, (key.radius * scale).max(0.15))
                } else {
                    let radius = if key.texture != 0. {
                        (2. * scale).round().max(1.) as u32
                    } else {
                        (native[0].max(native[1]) as f32 * 0.008 * scale)
                            .round()
                            .clamp(1., 512.) as u32
                    };
                    detail::blur(image, radius)
                };
                visit_mut(&mut basis, cancelled, |i, b| {
                    let p = image.pixels[i];
                    if p[3] <= 0. || b[3] <= 0. {
                        *b = [0.; 4];
                        return;
                    }
                    let y = detail::luma(detail::straight(p));
                    let low = detail::luma(detail::straight(*b));
                    let difference = y - low;
                    let threshold = key.threshold / 100. * 0.02;
                    let detail = difference.signum() * (difference.abs() - threshold).max(0.);
                    let protection = if key.clarity != 0. {
                        (4. * y.clamp(0., 1.) * (1. - y.clamp(0., 1.))).sqrt()
                    } else {
                        1.
                    };
                    *b = [detail, protection, 0.05 + y.abs() * 0.25, 1.];
                })?;
                self.spatial = Some(Checkpoint {
                    identity,
                    prefix: prefix.to_vec(),
                    key,
                    basis,
                    lease,
                });
            }
        } else {
            self.statistics.spatial_reuses += 1;
        }
        if let Some(s) = &self.spatial {
            visit_mut(&mut image.pixels, cancelled, |i, p| {
                let b = s.basis[i];
                if p[3] <= 0. || b[3] <= 0. {
                    return;
                }
                let rgb = detail::straight(*p);
                // Preserve multiplication order: protection is not folded into
                // the cached detail, and limiting remains dependent on amount.
                let delta = (amount * b[0] * b[1]).clamp(-b[2], b[2]);
                for c in 0..3 {
                    p[c] = (rgb[c] + delta) * p[3];
                }
            })?;
            Ok(())
        } else {
            detail.apply_domain(image, native, full, cancelled)
        }
    }
}
