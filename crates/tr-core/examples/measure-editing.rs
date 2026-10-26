//! CPU-only diagnostic on deterministic 18 MP data. This isolates graph costs;
//! it does not measure UI latency. Run in release, with no concurrent workload:
//! cargo run --release -p tr-core --example measure-editing -- exposure 8
use anyhow::{Result, ensure};
use std::{hint::black_box, time::Instant};
use tr_core::{
    budget::MemoryBudget,
    color::LinearImage,
    editing::{Advanced, EditRecipe, regional::Processor},
    preview::PreviewRequest,
    provider::ImageLevels,
    resample::Region,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let control = args.get(1).map_or("exposure", String::as_str);
    ensure!(
        ["exposure", "texture"].contains(&control),
        "exposure or texture"
    );
    let threads = args.get(2).map_or(Ok(8), |s| s.parse::<usize>())?;
    tr_core::compute::configure(threads)?;
    let (width, height) = (5184, 3456);
    let mut state = 48271_u32;
    let source = LinearImage::new(
        width,
        height,
        (0..width * height)
            .map(|i| {
                let base = 0.03 + 0.6 * (i % width) as f32 / width as f32;
                let rgb = std::array::from_fn::<_, 3, _>(|c| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    base * [1.1, 0.8, 0.6][c] + (state as f32 / u32::MAX as f32 - 0.5) * 0.03
                });
                [rgb[0], rgb[1], rgb[2], 1.]
            })
            .collect(),
    )?;
    let source = ImageLevels::from_source(source, PreviewRequest::full())?;
    let mut recipe = EditRecipe::neutral(Default::default());
    recipe.process_version = 3;
    recipe.exposure_ev = 0.25;
    let mut advanced = Advanced::default();
    if args.get(3).is_some_and(|s| s == "vignette") {
        advanced.geometry.vignette = 15.;
    }
    advanced.detail.luminance_noise = 25.;
    advanced.detail.chroma_noise = 35.;
    advanced.detail.texture = 15.;
    advanced.detail.clarity = 12.;
    advanced.detail.sharpen = 35.;
    advanced.detail.radius = 1.4;
    recipe.advanced = Some(Box::new(advanced));
    let region = Region {
        size: [855, 596],
        origin: [2112., 1429.],
        step: [1.; 2],
    };
    let mut processor = Processor::new(MemoryBudget::new(4 * 1024 * 1024 * 1024));
    for frame in 0..7 {
        if frame > 0 {
            if control == "exposure" {
                recipe.exposure_ev = 0.25 + frame as f32 * 0.1;
            } else {
                recipe.advanced.as_mut().unwrap().detail.texture = (frame as f32 - 3.) * 10.;
            }
        }
        processor.statistics = Default::default();
        let started = Instant::now();
        black_box(processor.render(&source, &recipe, [1.; 3], region)?);
        println!(
            "{}",
            serde_json::json!({"frame":frame,"control":control,"threads":threads,"elapsed_us":started.elapsed().as_micros(),"statistics":processor.statistics})
        );
    }
    Ok(())
}
