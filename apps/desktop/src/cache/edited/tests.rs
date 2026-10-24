use super::*;
use std::{
    sync::atomic::AtomicU64,
    time::{Duration, Instant},
};
use tr_core::{color::LinearImage, decoder::RawEngine, editing::CurvePoint};

fn cache() -> Arc<Manager> {
    Arc::new(Manager::new(crate::cache::Settings {
        memory_mib: 1024,
        diagnostic_fixed_memory: true,
        disk_mib: 64,
        free_mib: 0,
        raw_engine: RawEngine::LibRawBilinear,
        ..Default::default()
    }))
}
fn source(cache: &Manager) -> ImageLevels {
    let pixels = (0..65 * 33)
        .map(|i| {
            let value = (i % 97) as f32 / 60.;
            [value, value * 0.4, value * 0.2, 1.]
        })
        .collect();
    let mut image = ImageLevels::from_source(
        LinearImage::new(65, 33, pixels).unwrap(),
        PreviewRequest::full(),
    )
    .unwrap();
    image.attach_lease(cache.memory.try_reserve(image.byte_len() as u64).unwrap());
    image
}
fn render(
    cache: &Manager,
    source: &ImageLevels,
    index: usize,
    recipe: &EditRecipe,
) -> Arc<ImageLevels> {
    let base = source.base_level() + index as u32;
    let mut raster = source.levels()[index].clone();
    let size = recipe
        .apply_preview(&mut raster, source.source_size(), base)
        .unwrap();
    let edge = size[0].max(size[1]);
    let base = base.min(31 - edge.leading_zeros() + u32::from(!edge.is_power_of_two()));
    let mut image = ImageLevels::from_reference_mip(raster, size, base, true).unwrap();
    image.attach_lease(cache.memory.try_reserve(image.byte_len() as u64).unwrap());
    Arc::new(image)
}
fn wait(cache: &Manager) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while cache.preview_writes_pending() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_pixels_equal(actual: &ImageLevels, expected: &ImageLevels) {
    assert_eq!(actual.levels().len(), expected.levels().len());
    for (a, b) in actual.levels().iter().zip(expected.levels()) {
        assert_eq!((a.width, a.height), (b.width, b.height));
        assert_eq!(a.pixels, b.pixels);
    }
}

#[test]
fn canonical_edits_survive_relaunch_with_exact_pixels_and_complete_identity() {
    let dir = tempfile::tempdir().unwrap();
    let manager = cache();
    let writer = Writer::start(manager.clone(), Arc::new(AtomicU64::new(0)));
    let input = source(&manager);
    let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
    recipe.exposure_ev = 0.75;
    recipe.curve = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 0.4, y: 0.65 },
        CurvePoint { x: 1., y: 1. },
    ];
    let output = render(&manager, &input, 0, &recipe);
    let artifact = Artifact::new(
        manager.clone(),
        dir.path().into(),
        "verified-source",
        PreviewRequest::full(),
        &input,
        0,
        &recipe,
    );
    artifact.store(&writer, output.clone(), 0);
    assert!(manager.preview_writes_pending());
    wait(&manager);
    let reopened = cache();
    let baseline = reopened.memory.usage().reserved;
    let fresh = Artifact::new(
        reopened.clone(),
        dir.path().into(),
        "verified-source",
        PreviewRequest::full(),
        &input,
        0,
        &recipe,
    );
    let hit = fresh.load(&|| false).unwrap();
    assert_pixels_equal(&hit, &output);
    for variant in 0..5 {
        let mut request = PreviewRequest::full();
        let mut changed = recipe.clone();
        let digest = if variant == 0 {
            "different-source"
        } else {
            "verified-source"
        };
        if variant == 1 {
            changed.exposure_ev = -0.75;
        }
        if variant == 2 {
            request.raw_wb.red = 1250;
        }
        if variant == 3 {
            request.quality = tr_core::preview::PreviewQuality::Standard;
        }
        let index = usize::from(variant == 4);
        let miss = Artifact::new(
            reopened.clone(),
            dir.path().into(),
            digest,
            request,
            &input,
            index,
            &changed,
        );
        assert!(miss.load(&|| false).is_none(), "variant {variant}");
    }
    drop(hit);
    assert_eq!(reopened.memory.usage().reserved, baseline);
}

#[test]
fn cropped_reduced_edit_keeps_its_geometry_and_cannot_be_read_as_native() {
    let dir = tempfile::tempdir().unwrap();
    let manager = cache();
    let writer = Writer::start(manager.clone(), Arc::new(AtomicU64::new(0)));
    let input = source(&manager);
    let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
    recipe.process_version = 3;
    recipe.advanced = Some(Default::default());
    recipe.advanced.as_mut().unwrap().geometry.crop = [0.1, 0.2, 0.9, 0.8];
    recipe.advanced.as_mut().unwrap().geometry.quarter_turns = 1;
    let output = render(&manager, &input, 1, &recipe);
    let artifact = Artifact::new(
        manager.clone(),
        dir.path().into(),
        "verified",
        PreviewRequest::full(),
        &input,
        1,
        &recipe,
    );
    artifact.store(&writer, output.clone(), 0);
    wait(&manager);
    let hit = artifact.load(&|| false).unwrap();
    assert_pixels_equal(&hit, &output);
    assert_eq!(hit.source_size(), output.source_size());
    assert_eq!(hit.base_level(), 1);
    let native = Artifact::new(
        manager.clone(),
        dir.path().into(),
        "verified",
        PreviewRequest::full(),
        &input,
        0,
        &recipe,
    );
    assert!(native.load(&|| false).is_none());
}

#[test]
fn edited_cache_obeys_disable_cancel_and_disk_limits() {
    let dir = tempfile::tempdir().unwrap();
    let manager = cache();
    let input = source(&manager);
    let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
    recipe.exposure_ev = 1.;
    let artifact = Artifact::new(
        manager.clone(),
        dir.path().into(),
        "verified",
        PreviewRequest::full(),
        &input,
        0,
        &recipe,
    );
    let output = render(&manager, &input, 0, &recipe);
    let writer = Writer::start(manager.clone(), Arc::new(AtomicU64::new(1)));
    artifact.store(&writer, output.clone(), 0); // revoked domain
    wait(&manager);
    assert!(artifact.load(&|| false).is_none());
    let mut settings = manager.settings();
    settings.disk_mib = 0;
    manager.configure(settings.clone());
    artifact.store(&writer, output.clone(), 1);
    wait(&manager);
    assert!(artifact.load(&|| false).is_none());
    settings.disk_mib = 64;
    settings.enabled = false;
    manager.configure(settings);
    artifact.store(&writer, output, 1);
    assert!(!manager.preview_writes_pending());
    assert!(artifact.load(&|| false).is_none());
}

#[test]
fn a_short_writer_lock_reuses_the_canonical_result_instead_of_recomputing() {
    let dir = tempfile::tempdir().unwrap();
    let manager = cache();
    let input = source(&manager);
    let mut recipe = EditRecipe::neutral(RawEngine::LibRawBilinear);
    recipe.exposure_ev = 0.5;
    let output = render(&manager, &input, 0, &recipe);
    let artifact = Artifact::new(
        manager.clone(),
        dir.path().into(),
        "verified",
        PreviewRequest::full(),
        &input,
        0,
        &recipe,
    );
    let writer = Writer::start(manager.clone(), Arc::new(AtomicU64::new(0)));
    artifact.store(&writer, output.clone(), 0);
    wait(&manager);
    let path = dir.path().to_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    let locked = std::thread::spawn(move || {
        let _lock = crate::cache::Folder::open(&path, false, true).unwrap();
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(60));
    });
    rx.recv().unwrap();
    let hit = artifact.load(&|| false).unwrap();
    assert_pixels_equal(&hit, &output);
    assert!(manager.stats().busy > 0);
    locked.join().unwrap();
}
