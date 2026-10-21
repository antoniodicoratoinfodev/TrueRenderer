//! Offline denoiser probe on a trusted, previously decoded linear RGBA dump.
//! No original-photo decoder or product fallback. Output stays in the chosen directory.
use anyhow::{Context, Result, ensure};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Read, Write},
    path::PathBuf,
    time::Instant,
};
use tr_core::{
    color::LinearImage,
    editing::detail::{Denoise, Detail},
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 6 || args.len() == 10,
        "INPUT_DIR NEW_OUTPUT_PREFIX LUMA CHROMA legacy|spectral THREADS [X Y W H]"
    );
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(input.join("input.json"))?)?;
    ensure!(
        cfg!(target_endian = "little"),
        "Probe requires little-endian floats"
    );
    let w = u32::try_from(meta["size"][0].as_u64().context("width")?)?;
    let h = u32::try_from(meta["size"][1].as_u64().context("height")?)?;
    ensure!(
        w > 0 && h > 0 && u64::from(w) * u64::from(h) <= tr_core::color::MAX_PIXELS as u64,
        "image too large"
    );
    let mut pixels = vec![[0_f32; 4]; w as usize * h as usize];
    let mut file = std::fs::File::open(input.join("input.rgba32f"))?;
    ensure!(
        file.metadata()?.len() == u64::from(w) * u64::from(h) * 16,
        "Input length differs"
    );
    file.read_exact(bytemuck::cast_slice_mut(&mut pixels))?;
    let mut image = LinearImage::new(w, h, pixels)?;
    if args.len() == 10 {
        let mut rect = [0_u32; 4];
        for (v, s) in rect.iter_mut().zip(&args[6..]) {
            *v = s.parse()?;
        }
        let [x, y, cw, ch] = rect;
        ensure!(
            cw > 0
                && ch > 0
                && x.checked_add(cw).is_some_and(|v| v <= w)
                && y.checked_add(ch).is_some_and(|v| v <= h),
            "crop outside input"
        );
        let mut pixels = Vec::with_capacity((cw * ch) as usize);
        for yy in y..y + ch {
            pixels.extend_from_slice(
                &image.pixels[(yy * w + x) as usize..(yy * w + x + cw) as usize],
            );
        }
        image = LinearImage::new(cw, ch, pixels)?;
    }
    tr_core::compute::configure(args[5].parse()?)?;
    let detail = Detail {
        luminance_noise: args[2].parse()?,
        chroma_noise: args[3].parse()?,
        denoise: match args[4].as_str() {
            "legacy" => Denoise::Legacy,
            "spectral" => Denoise::SpectralV1,
            _ => anyhow::bail!("unknown method"),
        },
        ..Default::default()
    };
    detail.validate()?;
    let start = Instant::now();
    let size = [image.width, image.height];
    detail.apply(&mut image, size);
    let seconds = start.elapsed().as_secs_f64();
    let mut f = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.with_extension("rgba32f"))?,
    );
    f.write_all(bytemuck::cast_slice(&image.pixels))?;
    f.flush()?;
    let mut f = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.with_extension("ppm"))?,
    );
    writeln!(f, "P6\n{} {}\n255", image.width, image.height)?;
    for p in image.to_display().as_chunks::<4>().0 {
        f.write_all(&p[..3])?;
    }
    f.flush()?;
    let report = serde_json::json!({"seconds":seconds,"size":size,"detail":detail});
    std::fs::write(
        output.with_extension("json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
