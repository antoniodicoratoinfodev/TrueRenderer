use super::*;

#[test]
#[ignore = "local causal diagnostics"]
fn research_campaign_highlight_cells() {
    let path = std::env::var("TR_HL_SOURCE").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let raw = match crate::Raw::parse(&bytes).unwrap() {
        crate::Raw::Dng(d) => d.mosaic().unwrap(),
        crate::Raw::Nef(d) => d.mosaic().unwrap(),
        crate::Raw::Raf(d) => d.mosaic().unwrap(),
    };
    let (columns, rows) = (raw.active_width.div_ceil(SIDE), raw.height.div_ceil(SIDE));
    let mut grid = Grid {
        data: vec![Cell::default(); columns * rows],
        values: &raw.values,
        state: &raw.saturation,
        stride: raw.width,
        width: raw.active_width,
        height: raw.height,
        columns,
        cfa: raw.cfa,
        designs: (0..raw.cfa.period * raw.cfa.period)
            .map(|p| models(raw.cfa.shifted(p % raw.cfa.period, p / raw.cfa.period)))
            .collect(),
    };
    for y in 0..raw.height {
        for x in 0..raw.active_width {
            if raw.saturation[y * raw.width + x] == SATURATED {
                grid.data[y / SIDE * columns + x / SIDE].clipped |= 1 << raw.cfa.color(x, y);
            }
        }
    }
    for x in 0..columns {
        let c = grid.get(x, rows / 2);
        println!(
            "x={} valid={} clipped={} split={} mean={:?} jump={:?} estimate={:?}",
            x * SIDE,
            c.valid,
            c.clipped,
            c.split,
            c.plane.map(|p| p[0]),
            c.jump,
            estimate(&mut grid, x, rows / 2)
        );
    }
    let estimates: Vec<_> = (0..columns * rows)
        .map(|i| estimate(&mut grid, i % columns, i / columns))
        .collect();
    for i in 0..estimates.len() {
        if grid.data[i].clipped != 0 && estimates[i].is_none() {
            println!(
                "MISSING {},{} {:?}",
                i % columns * SIDE,
                i / columns * SIDE,
                occlusion::estimate(&grid, &estimates, i % columns, i / columns)
            );
        }
    }
    let mut recovered = raw.values.clone();
    println!(
        "Evidence {:?}",
        recover(
            &mut recovered,
            &raw.saturation,
            raw.width,
            raw.active_width,
            raw.height,
            raw.cfa
        )
    );
    let truth_path = std::path::Path::new(&path).with_extension("cfa");
    if truth_path.exists() {
        let truth: Vec<_> = std::fs::read(truth_path)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        for y in 0..raw.height {
            for x in 0..raw.active_width {
                let i = y * raw.width + x;
                if raw.saturation[i] == SATURATED
                    && (recovered[i] - truth[i]).abs() > 0.005
                    && recovered[i] != raw.values[i]
                    && y < 8
                {
                    println!(
                        "MISMATCH {x},{y}: was {} now {} truth {}",
                        raw.values[i], recovered[i], truth[i]
                    );
                }
            }
        }
    }
}
