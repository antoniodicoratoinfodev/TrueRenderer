//! CPU-order DCT butterflies distributed over all patch invocations. Every
//! stage writes shared memory before its consumers, retaining f32 rounding
//! without the serial row ownership of the earlier rejected experiment.
use std::fmt::Write;

pub(super) fn shader() -> String {
    let mut code = String::new();
    for inverse in [false, true] {
        let direction = if inverse { "inverse" } else { "forward" };
        for column in [false, true] {
            let axis = if column { "column" } else { "row" };
            for n in [4_usize, 8, 16] {
                let norm = [(1. / n as f32).sqrt(), (2. / n as f32).sqrt()];
                writeln!(code, "fn {direction}_{axis}{n}(local: vec3<u32>) {{").unwrap();
                let (coordinate, line, stride) = if column {
                    ("local.y", "local.x", 16)
                } else {
                    ("local.x", "local.y * 16u", 1)
                };
                writeln!(code, "let lane = {coordinate};\nlet line = {line};\nlet slot = local.y * 16u + local.x;").unwrap();
                let levels = n.ilog2();
                let split: Vec<_> = (1..=levels)
                    .rev()
                    .filter(|level| !inverse || *level > 1)
                    .map(|level| (1 << level, true))
                    .collect();
                let stages: Vec<_> = split
                    .into_iter()
                    .chain((if inverse { 1 } else { 2 }..=levels).map(|l| (1 << l, false)))
                    .collect();
                for (stage, (size, split)) in stages.iter().copied().enumerate() {
                    let half = size / 2;
                    let first = (stage % 2 == 0) != column;
                    let (source, target) = if first {
                        ("first", "second")
                    } else {
                        ("second", "first")
                    };
                    writeln!(code, "{{\nvar value = vec3(0.);\nif local.x < {n}u && local.y < {n}u {{\nlet offset = lane / {size}u * {size}u;\nlet position = lane % {size}u;").unwrap();
                    if inverse && !split {
                        writeln!(code, "let k = min(position, {size}u - 1u - position);").unwrap();
                    } else if !inverse && !split {
                        code.push_str("let k = position / 2u;\n");
                    } else {
                        writeln!(code, "let k = position % {half}u;").unwrap();
                    }
                    let read = |index: &str| format!("{source}[line + {stride}u * ({index})].xyz");
                    if inverse && split {
                        let scale = |index: &str| {
                            let value = read(index);
                            if stage == 0 {
                                format!(
                                    "rounded3({value} * select({:?}, {:?}, ({index}) != 0u))",
                                    norm[0], norm[1]
                                )
                            } else {
                                value
                            }
                        };
                        writeln!(code, "if position < {half}u {{ value = {}; }} else {{\nvalue = {};\nif k > 0u {{ value += {}; }}\n}}", scale("offset + 2u * k"), scale("offset + 2u * k + 1u"), scale("offset + 2u * k - 1u")).unwrap();
                    } else if !inverse && !split {
                        writeln!(code, "if position % 2u == 0u {{ value = {}; }} else {{\nvalue = {};\nif k + 1u < {half}u {{ value += {}; }}\n}}", read("offset + k"), read(&format!("offset + {half}u + k")), read(&format!("offset + {half}u + k + 1u"))).unwrap();
                    } else {
                        write!(code, "let factors = array<f32, {half}>(").unwrap();
                        for k in 0..half {
                            let factor = (0.5
                                / (std::f64::consts::PI / size as f64 * (k as f64 + 0.5)).cos())
                                as f32;
                            if k > 0 {
                                code.push(',');
                            }
                            write!(code, "{factor:?}").unwrap();
                        }
                        code.push_str(");\n");
                        if inverse {
                            writeln!(code, "let even = {};\nlet delta = rounded3({} * factors[k]);\nvalue = select(even - delta, even + delta, position < {half}u);", read("offset + k"), read(&format!("offset + {half}u + k"))).unwrap();
                        } else {
                            writeln!(code, "let a = {};\nlet b = {};\nvalue = select(rounded3(a - b) * factors[k], a + b, position < {half}u);", read("offset + k"), read(&format!("offset + {size}u - 1u - k"))).unwrap();
                        }
                    }
                    if !inverse && stage + 1 == stages.len() {
                        writeln!(
                            code,
                            "value = rounded3(value) * select({:?}, {:?}, lane != 0u);",
                            norm[0], norm[1]
                        )
                        .unwrap();
                    }
                    writeln!(
                        code,
                        "}}\n{target}[slot] = vec4(value, 0.);\n}}\nworkgroupBarrier();"
                    )
                    .unwrap();
                }
                code.push_str("}\n");
            }
            writeln!(code, "fn {direction}_{axis}(local: vec3<u32>, n: u32) {{\nswitch n {{\ncase 4u: {{ {direction}_{axis}4(local); }}\ncase 8u: {{ {direction}_{axis}8(local); }}\ndefault: {{ {direction}_{axis}16(local); }}\n}}\n}}").unwrap();
        }
    }
    code.push_str(&super::shader_text(
        "../denoise_probe.wgsl",
        include_str!("../denoise_probe.wgsl"),
    ));
    code
}
