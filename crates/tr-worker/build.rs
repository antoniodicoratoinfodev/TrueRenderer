use std::{env, path::PathBuf, process::Command};
fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../native/macos");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in ["image_decoder.m", "image_decoder.h"] {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
    }
    assert!(
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
                "-mmacosx-version-min=13.0"
            ])
            .arg(root.join("image_decoder.m"))
            .arg("-o")
            .arg(out.join("image_decoder.o"))
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("ar")
            .arg("crs")
            .arg(out.join("libtr_image.a"))
            .arg(out.join("image_decoder.o"))
            .status()
            .unwrap()
            .success()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=tr_image");
    for framework in [
        "Foundation",
        "CoreImage",
        "Metal",
        "CoreGraphics",
        "ImageIO",
        "UniformTypeIdentifiers",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
