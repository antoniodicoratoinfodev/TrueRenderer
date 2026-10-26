//! Canonical CPU oracle for qualification of a single GPU spectral scale.
//! This does not replace the production multiscale evaluator or its models.
use super::*;
use crate::editing::regional::Rect;

pub struct SpectralReference {
    pub levels: Vec<f32>,
    pub sigma: Vec<Pixel>,
    pub output: LinearImage,
}

/// Diagnostic oracle for isolated timing; evaluates the unchanged production
/// regional filter, including its full-domain models and chroma guides.
pub fn multiscale_region_reference(
    source: &LinearImage,
    scale: f32,
    origin: [u32; 2],
    size: [u32; 2],
    amounts: [f32; 2],
    threshold: f32,
) -> Result<LinearImage> {
    ensure!(
        scale.is_finite() && scale > 0. && scale <= 1.,
        "Invalid reference scale"
    );
    ensure!(
        amounts
            .into_iter()
            .chain([threshold])
            .all(|v| v.is_finite() && (0. ..=1.).contains(&v)),
        "Invalid reference amounts"
    );
    ensure!(
        (0..2).all(|a| size[a] > 0
            && u64::from(origin[a]) + u64::from(size[a])
                <= u64::from([source.width, source.height][a])),
        "Invalid reference region"
    );
    super::regional::apply(
        source,
        scale,
        amounts[0],
        amounts[1],
        threshold,
        Rect { origin, size },
        &mut [0; 4],
        &|| false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn spectral_reference(
    source: &LinearImage,
    block_size: usize,
    origin: [u32; 2],
    size: [u32; 2],
    amounts: [f32; 2],
    threshold: f32,
    coarse: bool,
) -> Result<SpectralReference> {
    ensure!(
        amounts
            .into_iter()
            .chain([threshold])
            .all(|v| v.is_finite() && (0. ..=1.).contains(&v)),
        "Invalid spectral reference parameters"
    );
    ensure!(
        (0..2).all(|a| size[a] > 0
            && u64::from(origin[a]) + u64::from(size[a])
                <= u64::from([source.width, source.height][a])),
        "Invalid spectral reference region"
    );
    let rect = Rect { origin, size };
    let amounts = [amounts[0], amounts[1], amounts[1]];
    match block_size {
        4 => run::<4>(source, rect, amounts, threshold, coarse),
        8 => run::<8>(source, rect, amounts, threshold, coarse),
        16 => run::<16>(source, rect, amounts, threshold, coarse),
        _ => anyhow::bail!("Unsupported spectral block size"),
    }
}

fn run<const N: usize>(
    source: &LinearImage,
    rect: Rect,
    amounts: [f32; 3],
    threshold: f32,
    coarse: bool,
) -> Result<SpectralReference> {
    let transform = denoise::Transform::<N>::new();
    let model = Model::estimate(source, &transform, coarse, &|| false)?;
    let pixels = if model.active(amounts) {
        denoise::filter_region(
            source,
            &transform,
            &|| false,
            &|block, level| {
                model.shrink(block, level, amounts, threshold);
            },
            rect,
        )?
    } else {
        super::super::input::extract(source, rect)?.pixels
    };
    Ok(SpectralReference {
        levels: model.bands.iter().map(|b| b.level).collect(),
        sigma: model
            .bands
            .iter()
            .flat_map(|b| b.sigma.iter().flatten().copied())
            .collect(),
        output: LinearImage::new(rect.size[0], rect.size[1], pixels)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_scale_oracle_matches_full_luminance_and_regional_lattice() {
        let source = LinearImage::new(
            133,
            131,
            (0..133 * 131)
                .map(|i| {
                    let v = 0.2 + (i % 113) as f32 * 0.0002;
                    [v, v * 0.8, v * 0.6, 1.]
                })
                .collect(),
        )
        .unwrap();
        for (n, scale) in [(4, 0.25), (8, 0.5), (16, 1.)] {
            let mut full = source.clone();
            super::super::apply(&mut full, scale, 0.7, 0., 0.25, &|| false).unwrap();
            let reference =
                spectral_reference(&source, n, [0, 0], [133, 131], [0.7, 0.], 0.25, false).unwrap();
            assert_eq!(full.pixels, reference.output.pixels);
            let regional =
                spectral_reference(&source, n, [3, 5], [129, 125], [0.7, 0.], 0.25, false).unwrap();
            for y in 0..125 {
                assert_eq!(
                    &regional.output.pixels[y * 129..(y + 1) * 129],
                    &full.pixels[(y + 5) * 133 + 3..(y + 5) * 133 + 132]
                );
            }
        }
    }
}
