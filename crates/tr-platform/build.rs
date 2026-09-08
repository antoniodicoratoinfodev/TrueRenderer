use std::{env, path::PathBuf, process::Command};

fn checked(command: &mut Command) {
    assert!(
        command.status().expect("native compiler").success(),
        "native build failed"
    );
}
fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../native/macos");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut objects = vec![];
    for source in ["signing.m", "xpc_bridge.m", "resources.m"] {
        let input = root.join(source);
        println!("cargo:rerun-if-changed={}", input.display());
        let object = out.join(format!("{source}.o"));
        checked(
            Command::new("xcrun")
                .args([
                    "clang",
                    "-c",
                    "-fobjc-arc",
                    "-fblocks",
                    "-O2",
                    "-Wall",
                    "-Wextra",
                    "-Werror",
                    "-mmacosx-version-min=13.0",
                ])
                .arg(&input)
                .arg("-o")
                .arg(&object),
        );
        objects.push(object);
    }
    for header in ["signing.h", "xpc_bridge.h"] {
        println!("cargo:rerun-if-changed={}", root.join(header).display());
    }
    checked(
        Command::new("ar")
            .arg("crs")
            .arg(out.join("libtr_xpc.a"))
            .args(objects),
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=tr_xpc");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Security");
}
