//! Native decoders. macOS compiles the Objective-C adapter over ImageIO and
//! CIRAWFilter; Windows compiles LibRaw and the TrueRenderer shim over it.
//!
//! LibRaw is vendored unmodified under `third_party/libraw`; the fixes MSVC
//! needs live here, outside the covered files, so nothing upstream is touched.
//! ADR 0008 records the licence election, why the link is static, and why no
//! binding generator runs at build time.
use std::{env, path::PathBuf, process::Command};

const LIBRAW_SOURCES: [&str; 79] = [
    "src/decoders/canon_600.cpp",
    "src/decoders/crx.cpp",
    "src/decoders/decoders_dcraw.cpp",
    "src/decoders/decoders_libraw.cpp",
    "src/decoders/decoders_libraw_dcrdefs.cpp",
    "src/decoders/dng.cpp",
    "src/decoders/fp_dng.cpp",
    "src/decoders/fuji_compressed.cpp",
    "src/decoders/generic.cpp",
    "src/decoders/kodak_decoders.cpp",
    "src/decoders/load_mfbacks.cpp",
    "src/decoders/olympus14.cpp",
    "src/decoders/pana8.cpp",
    "src/decoders/smal.cpp",
    "src/decoders/sonycc.cpp",
    "src/decoders/unpack.cpp",
    "src/decoders/unpack_thumb.cpp",
    "src/decompressors/losslessjpeg.cpp",
    "src/demosaic/aahd_demosaic.cpp",
    "src/demosaic/ahd_demosaic.cpp",
    "src/demosaic/dcb_demosaic.cpp",
    "src/demosaic/dht_demosaic.cpp",
    "src/demosaic/misc_demosaic.cpp",
    "src/demosaic/xtrans_demosaic.cpp",
    "src/integration/dngsdk_glue.cpp",
    "src/integration/rawspeed_glue.cpp",
    "src/libraw_c_api.cpp",
    "src/libraw_datastream.cpp",
    "src/metadata/adobepano.cpp",
    "src/metadata/canon.cpp",
    "src/metadata/ciff.cpp",
    "src/metadata/cr3_parser.cpp",
    "src/metadata/epson.cpp",
    "src/metadata/exif_gps.cpp",
    "src/metadata/fuji.cpp",
    "src/metadata/hasselblad_model.cpp",
    "src/metadata/identify.cpp",
    "src/metadata/identify_tools.cpp",
    "src/metadata/kodak.cpp",
    "src/metadata/leica.cpp",
    "src/metadata/makernotes.cpp",
    "src/metadata/mediumformat.cpp",
    "src/metadata/minolta.cpp",
    "src/metadata/misc_parsers.cpp",
    "src/metadata/nikon.cpp",
    "src/metadata/normalize_model.cpp",
    "src/metadata/olympus.cpp",
    "src/metadata/p1.cpp",
    "src/metadata/pentax.cpp",
    "src/metadata/samsung.cpp",
    "src/metadata/sony.cpp",
    "src/metadata/tiff.cpp",
    "src/postprocessing/aspect_ratio.cpp",
    "src/postprocessing/dcraw_process.cpp",
    "src/postprocessing/mem_image.cpp",
    "src/postprocessing/postprocessing_aux.cpp",
    "src/postprocessing/postprocessing_utils.cpp",
    "src/postprocessing/postprocessing_utils_dcrdefs.cpp",
    "src/preprocessing/ext_preprocess.cpp",
    "src/preprocessing/raw2image.cpp",
    "src/preprocessing/subtract_black.cpp",
    "src/tables/cameralist.cpp",
    "src/tables/colorconst.cpp",
    "src/tables/colordata.cpp",
    "src/tables/wblists.cpp",
    "src/utils/curves.cpp",
    "src/utils/decoder_info.cpp",
    "src/utils/init_close_utils.cpp",
    "src/utils/open.cpp",
    "src/utils/phaseone_processing.cpp",
    "src/utils/read_utils.cpp",
    "src/utils/thumb_utils.cpp",
    "src/utils/utils_dcraw.cpp",
    "src/utils/utils_libraw.cpp",
    "src/write/apply_profile.cpp",
    "src/write/file_write.cpp",
    "src/write/tiff_writer.cpp",
    "src/x3f/x3f_parse_process.cpp",
    "src/x3f/x3f_utils_patched.cpp",
];

fn main() {
    let target = env::var("CARGO_CFG_TARGET_OS");
    match target.as_deref() {
        Ok("macos") => {
            apple();
            libraw();
        }
        Ok("windows") => libraw(),
        _ => {}
    }
}

/// Apple adapter: one Objective-C file with a narrow C header, linked against
/// the frameworks it uses. Unchanged from before LibRaw existed.
fn apple() {
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

/// Native adapter: LibRaw plus the shim that holds the recipe.
fn libraw() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let shim = root.join("native/libraw");
    for name in ["tr_libraw.cpp", "tr_libraw.h"] {
        println!("cargo:rerun-if-changed={}", shim.join(name).display());
    }
    let vendored = root.join("third_party/libraw");
    println!("cargo:rerun-if-changed={}", vendored.display());
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .include(&vendored)
        .include(&shim)
        .warnings(false)
        .extra_warnings(false);
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Match the Objective-C adapter and bundle minimum, not the build SDK.
        build.flag("-mmacosx-version-min=13.0");
    }
    // Static linking. Without this the headers declare every symbol
    // `__declspec(dllimport)` and each definition clashes with its own
    // declaration, which MSVC reports as C4273.
    build.define("LIBRAW_NODLL", None);
    // Optional integrations that would pull in libraries this build does not
    // ship. Stated rather than left to a default that could change.
    build.define("NO_JASPER", None);
    build.define("NO_JPEG", None);
    build.define("NO_LCMS", None);
    build.define("LIBRAW_CALLOC_RAWSTORE", None);
    for source in LIBRAW_SOURCES {
        build.file(vendored.join(source));
    }
    build.file(shim.join("tr_libraw.cpp"));
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // One response file avoids progressive in-place archive merges. MSVC
        // 14.51 on this Windows host rejects overwriting the input archive with
        // LNK1114; it also keeps the command below Windows' length limit.
        let objects = build.compile_intermediates();
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        let library = out.join("tr_libraw_engines.lib");
        let response = out.join("tr-libraw-objects.rsp");
        let mut args = format!("/NOLOGO\n/OUT:\"{}\"\n", library.display());
        for object in objects {
            args.push_str(&format!("\"{}\"\n", object.display()));
        }
        std::fs::write(&response, args).unwrap();
        assert!(
            build
                .get_archiver()
                .arg(format!("@{}", response.display()))
                .status()
                .unwrap()
                .success(),
            "MSVC LibRaw archive failed"
        );
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-lib=static=tr_libraw_engines");
    } else {
        build.compile("tr_libraw");
    }
}
