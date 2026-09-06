//! Independent signal tests and unchanged corpus renders, using the UI's sampler.
use anyhow::{Result, ensure};
use std::{f64::consts::TAU, path::Path};
use tr_core::{
    color::LinearImage,
    resample::{Pyramid, Region, VERSION},
};

fn save(path: &Path, raster: &LinearImage) -> Result<()> {
    image::save_buffer(
        path,
        &raster.to_display(),
        raster.width,
        raster.height,
        image::ColorType::Rgba8,
    )?;
    Ok(())
}

pub fn run(root: &Path, worker: &Path) -> Result<()> {
    let output = root.join("reports/resampling");
    std::fs::create_dir_all(&output)?;
    let mut signals = Vec::new();
    let mut passed = true;
    // Reference is the analytic sinusoid, not a second copy of the filter.
    // Exclude the transition band and a 12-output-pixel boundary from acceptance.
    for frequency in [0.017, 0.043, 0.113, 0.183, 0.317, 0.437] {
        let source = LinearImage::new(
            768,
            128,
            (0..768 * 128)
                .map(|i| {
                    let v =
                        (0.5 + 0.5 * (TAU * frequency * (f64::from(i % 768) + 0.5)).sin()) as f32;
                    [v, v, v, 1.]
                })
                .collect(),
        )?;
        let pyramid = Pyramid::new(source)?;
        for width in [96, 192, 320, 600] {
            let size = [width, ((f64::from(width) / 6.).round() as u32).max(1)];
            let image = pyramid.render(Region::fitted([768, 128], size))?;
            let values: Vec<_> = (12..width - 12)
                .map(|x| f64::from(image.pixels[(size[1] / 2 * width + x) as usize][0]) - 0.5)
                .collect();
            let rms = (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt();
            let expected_rms = ((12..width - 12)
                .map(|x| {
                    let v = 0.5
                        * (TAU * frequency * ((f64::from(x) + 0.5) * 768. / f64::from(width)))
                            .sin();
                    v * v
                })
                .sum::<f64>()
                / values.len() as f64)
                .sqrt();
            let nyquist = f64::from(width) / 768. / 2.;
            let check = if frequency >= nyquist * 1.5 {
                "stopband"
            } else if frequency <= nyquist * 0.25 {
                "passband"
            } else {
                "transition_observed_only"
            };
            let accepted = match check {
                "stopband" => rms <= 0.02,
                "passband" => (0.95..=1.05).contains(&(rms / expected_rms)),
                _ => true,
            };
            passed &= accepted;
            signals.push(serde_json::json!({"frequency_cycles_per_source_pixel":frequency,"output":size,"output_nyquist_in_source_units":nyquist,"linear_rms":rms,"analytic_unfiltered_rms":expected_rms,"check":check,"passed":accepted}));
        }
    }
    let path = root.join("corpus/04_Frequenze_radiali.png");
    let (_, digest) = tr_platform::snapshot(&path)?;
    let mut broker = tr_platform::Broker::with_slot(worker.into(), 0);
    let decoded = broker.decode(&path, &digest, 0)?;
    let pyramid = Pyramid::new(decoded.raster)?;
    let source = pyramid.source();
    let exact = pyramid.render(Region::fitted(
        [source.width, source.height],
        [source.width, source.height],
    ))?;
    let exact_1to1 = exact.pixels == source.pixels;
    passed &= exact_1to1;
    save(&output.join("radial-1to1.png"), &exact)?;
    let mut renders = Vec::new();
    for size in [[160, 107], [320, 213], [444, 296], [860, 573], [1440, 960]] {
        let current = pyramid.render(Region::fitted([source.width, source.height], size))?;
        save(
            &output.join(format!("radial-{}x{}.png", size[0], size[1])),
            &current,
        )?;
        // Model the old Adatta path: nearest sample selection at arbitrary scale.
        let mut pixels = Vec::new();
        for y in 0..size[1] {
            for x in 0..size[0] {
                let sx = ((f64::from(x) + 0.5) * f64::from(source.width) / f64::from(size[0]))
                    .floor() as u32;
                let sy = ((f64::from(y) + 0.5) * f64::from(source.height) / f64::from(size[1]))
                    .floor() as u32;
                pixels.push(
                    source.pixels[(sy.min(source.height - 1) * source.width
                        + sx.min(source.width - 1)) as usize],
                );
            }
        }
        save(
            &output.join(format!("legacy-nearest-{}x{}.png", size[0], size[1])),
            &LinearImage::new(size[0], size[1], pixels)?,
        )?;
        renders.push(size);
    }
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"sampling":VERSION,"passed":passed,"exact_1to1":exact_1to1,"source_digest":digest,"source_dimensions":[source.width,source.height],"transport":broker.transport(),"signal_cases":signals,"radial_renders":renders,"thresholds":{"stopband_rms_max":0.02,"passband_contrast_ratio":[0.95,1.05],"boundary_exclusion_output_pixels":12},"scope":"CPU subset: sine rejection/preservation, exact source pixels, unchanged radial corpus. Not a full orientation, tile, isotropy, temporal LOD, display or ICC qualification. Legacy PNGs model nearest Adatta; they are not captured UI screenshots."});
    std::fs::write(
        root.join("reports/resampling-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Sampling: passed={passed}; 24 signal cases; exact 1:1={exact_1to1}");
    ensure!(
        passed,
        "Qualifica campionamento fallita: vedere reports/resampling-macos.json"
    );
    Ok(())
}

pub fn screenshots(root: &Path, worker: &Path) -> Result<()> {
    let path = root.join("corpus/04_Frequenze_radiali.png");
    let (_, digest) = tr_platform::snapshot(&path)?;
    let mut broker = tr_platform::Broker::with_slot(worker.into(), 0);
    let pyramid = Pyramid::new(broker.decode(&path, &digest, 0)?.raster)?;
    let mut checks = Vec::new();
    let mut passed = true;
    for name in [
        "01-grid",
        "04-radial-1to1",
        "05-radial-fit",
        "06-radial-37percent",
        "07-grid-small",
        "08-grid-large",
    ] {
        let records: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(
            root.join("reports").join(format!("{name}-sampling.json")),
        )?)?;
        ensure!(
            !records.is_empty(),
            "Nessun raster radiale catturato in {name}"
        );
        // These are our own smoke-test artifacts, never arbitrary photo input.
        let screen = image::open(root.join("reports").join(format!("{name}.png")))?.to_rgba8();
        for record in records {
            let size: [u32; 2] = serde_json::from_value(record["size"].clone())?;
            let origin = serde_json::from_value(record["origin"].clone())?;
            let step = serde_json::from_value(record["step"].clone())?;
            let rect: [f64; 4] = serde_json::from_value(record["rect_physical"].clone())?;
            let aligned = rect
                .iter()
                .all(|v| v.is_finite() && *v >= 0. && (*v - v.round()).abs() < 0.001)
                && (rect[2] - rect[0] - f64::from(size[0])).abs() < 0.001
                && (rect[3] - rect[1] - f64::from(size[1])).abs() < 0.001;
            ensure!(
                aligned
                    && rect[2] <= f64::from(screen.width())
                    && rect[3] <= f64::from(screen.height()),
                "Geometria fisica invalida: {record}"
            );
            let expected = pyramid.render(Region { size, origin, step })?.to_display();
            let mut maximum = 0u8;
            let mut differing_channels = 0u64;
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let pixel = screen.get_pixel(rect[0] as u32 + x, rect[1] as u32 + y).0;
                    for c in 0..4 {
                        let difference =
                            pixel[c].abs_diff(expected[((y * size[0] + x) * 4) as usize + c]);
                        maximum = maximum.max(difference);
                        differing_channels += u64::from(difference != 0);
                    }
                }
            }
            let accepted = maximum <= 1;
            passed &= accepted;
            checks.push(serde_json::json!({"screenshot":name,"rect_physical":rect,"size":size,"origin":origin,"step":step,"max_channel_error_u8":maximum,"differing_channels":differing_channels,"passed":accepted}));
        }
    }
    let report = serde_json::json!({"application":"TrueRenderer","version":env!("CARGO_PKG_VERSION"),"sampling":VERSION,"passed":passed,"channel_error_threshold_u8":1,"checks":checks,"scope":"Actual egui/wgpu surface screenshot pixels vs CPU raster, six views at the captured backing scale. Not compositor, monitor profile or physical display certification."});
    std::fs::write(
        root.join("reports/sampling-presentation-macos.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Native surface sampling: passed={passed}");
    ensure!(
        passed,
        "Pixel della superficie diversi dal raster: vedere sampling-presentation-macos.json"
    );
    Ok(())
}
