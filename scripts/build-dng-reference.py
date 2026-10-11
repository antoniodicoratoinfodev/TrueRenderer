#!/usr/bin/env python3
"""Build an offline macOS DNG SDK 1.7.1/2724 reference, never linked to the app.

Supply the official extracted SDK, CMake and a libjpeg prefix (e.g. Homebrew's
jpeg-turbo). No downloads or system installation. XMP is disabled; two missing
upstream XMP guards are applied to a private copy of dng_jxl.cpp. The TIFF,
lossless JPEG, sensor and colour algorithms are unchanged. All outputs stay in
project var/. The Adobe SDK and its dependencies retain their original notices.
"""
import argparse
import difflib
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--cmake", default="cmake")
    parser.add_argument("--jpeg-prefix", required=True, type=Path)
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    sdk, output, jpeg = args.sdk.resolve(strict=True), args.output.resolve(), args.jpeg_prefix.resolve(strict=True)
    if ROOT / "var" not in output.parents or args.jobs < 1:
        raise ValueError("Use an output directory under project var/ and positive jobs")
    output.mkdir(parents=True, exist_ok=True)
    original = (sdk / "dng_sdk/source/dng_jxl.cpp").read_text()
    start = "\t\tif (includeXMP && metadata && metadata->GetXMP ())"
    end = "\t\t\t} // xmp"
    if original.count(start) != 1 or original.count(end) != 1:
        raise ValueError("Unsupported SDK source; check the XMP-disabled build adaptation")
    patched = original.replace(start, "\t\t#if qDNGUseXMP\n" + start).replace(end, end + "\n\t\t#endif")
    a = patched.index("void dng_jxl_decoder::ProcessXMPBox")
    b = patched.index("void dng_jxl_decoder::ProcessBox", a)
    part = patched[a:b].replace("\tif (fInfo)\n", "\t#if qDNGUseXMP\n\tif (fInfo)\n")
    close = part.rfind("\n\t}\n")
    if close < 0:
        raise ValueError("Unsupported SDK XMP function")
    part = part[:close] + "\n\t#endif\n" + part[close:]
    patched = patched[:a] + part + patched[b:]
    (output / "dng_jxl.cpp").write_text(patched)
    (output / "xmp-disabled.patch").write_text("".join(difflib.unified_diff(
        original.splitlines(True), patched.splitlines(True), fromfile="dng_jxl.cpp", tofile="dng_jxl.cpp")))
    (output / "CMakeLists.txt").write_text(r'''cmake_minimum_required(VERSION 3.16)
project(TRDngReference LANGUAGES C CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_BUILD_TYPE Release)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(BUILD_TESTING OFF CACHE BOOL "" FORCE)
foreach(flag TOOLS DEVTOOLS FUZZERS DOXYGEN MANPAGES BENCHMARK EXAMPLES JPEGLI SJPEG)
  set(JPEGXL_ENABLE_${flag} OFF CACHE BOOL "" FORCE)
endforeach()
add_subdirectory("${TR_DNG_SDK}/libjxl/libjxl" jxl)
file(GLOB DNG_SOURCES "${TR_DNG_SDK}/dng_sdk/source/*.cpp")
list(FILTER DNG_SOURCES EXCLUDE REGEX "/dng_(jxl|xmp_sdk|update_meta)\.cpp$")
add_executable(dng_validate ${DNG_SOURCES} dng_jxl.cpp)
target_compile_definitions(dng_validate PRIVATE qMacOS=1 qWinOS=0 qDNGUseStdInt=1 qDNGValidate=1 qDNGValidateTarget=1 qDNGUseXMP=0 qDNGXMPFiles=0 qDNGXMPDocOps=0 qDNGUseLibJPEG=1)
target_include_directories(dng_validate PRIVATE "${TR_DNG_SDK}/dng_sdk/source" "${TR_JPEG_PREFIX}/include")
target_link_libraries(dng_validate PRIVATE jxl jxl_threads "${TR_JPEG_PREFIX}/lib/libjpeg.dylib" z "-framework CoreServices" "-framework Accelerate")
''')
    subprocess.run([args.cmake, "-S", str(output), "-B", str(output / "out"),
                    "-DCMAKE_POLICY_VERSION_MINIMUM=3.5", f"-DTR_DNG_SDK={sdk}",
                    f"-DTR_JPEG_PREFIX={jpeg}"], check=True)
    subprocess.run([args.cmake, "--build", str(output / "out"), "--target", "dng_validate",
                    "-j", str(args.jobs)], check=True)
    reference = output / "out/dng_validate"
    (output / "build.json").write_text(json.dumps({
        "reference": str(reference), "sha256": hashlib.sha256(reference.read_bytes()).hexdigest(),
        "sdk_license_sha256": hashlib.sha256((sdk / "LICENSE.txt").read_bytes()).hexdigest(),
        "xmp_disabled": True, "adaptation": "xmp-disabled.patch",
        "linked_to_truerenderer": False,
    }, indent=2) + "\n")
    print(reference)


if __name__ == "__main__":
    main()
