#!/usr/bin/env python3
"""Package prebuilt Rust binaries and two separate sandboxed XPC decoder services."""
from pathlib import Path
import argparse
import plistlib
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def run(*arguments):
    subprocess.run(arguments, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", default="dist/TrueRenderer.app")
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--fault-injection", action="store_true", help="test-only service, never package for users")
    args = parser.parse_args()
    if args.fault_injection and not (ROOT / args.bundle).resolve().is_relative_to(ROOT / "var"):
        raise SystemExit("Fault-injection bundles may only be created under var/.")
    bundle = (ROOT / args.bundle).resolve()
    if bundle.suffix != ".app" or not any(parent in bundle.parents for parent in [ROOT / "dist", ROOT / "var"]):
        raise SystemExit("Bundle must be an .app inside project dist/ or var/.")
    build = ROOT / "target" / args.profile
    contents = bundle / "Contents"
    (contents / "MacOS").mkdir(parents=True, exist_ok=True)
    (contents / "Resources").mkdir(exist_ok=True)
    shutil.copy2(build / "truerenderer", contents / "MacOS/TrueRenderer")
    shutil.copy2(build / "tr-worker", contents / "MacOS/tr-worker")
    shutil.copy2(ROOT / "scripts/Info.plist", contents / "Info.plist")
    host_info = plistlib.loads((contents / "Info.plist").read_bytes())
    staging = ROOT / "var/package-macos"
    staging.mkdir(parents=True, exist_ok=True)
    executable = staging / "Decoder"
    run("xcrun", "clang", "-fobjc-arc", "-fblocks", "-O2", "-Wall", "-Wextra", "-Werror",
        *(["-DTR_XPC_FAULT_INJECTION=1"] if args.fault_injection else []),
        "-mmacosx-version-min=13.0", "-framework", "Foundation", "-framework", "Security",
        str(ROOT / "native/macos/xpc_worker.m"), str(build / "libtr_worker.a"),
        "-framework", "CoreImage", "-framework", "CoreGraphics", "-framework", "ImageIO",
        "-framework", "UniformTypeIdentifiers", "-liconv", "-o", str(executable))
    entitlements = staging / "decoder.entitlements.plist"
    entitlements.write_bytes(plistlib.dumps({"com.apple.security.app-sandbox": True}))
    for slot in range(2):
        service = contents / f"XPCServices/Decoder{slot}.xpc"
        (service / "Contents/MacOS").mkdir(parents=True, exist_ok=True)
        shutil.copy2(executable, service / "Contents/MacOS/Decoder")
        info = {"CFBundleExecutable": "Decoder", "CFBundleIdentifier": f"it.truerenderer.prototype.decoder.{slot}",
                "CFBundlePackageType": "XPC!", "CFBundleVersion": host_info["CFBundleVersion"], "CFBundleShortVersionString": host_info["CFBundleShortVersionString"],
                "XPCService": {"ServiceType": "Application", "RunLoopType": "dispatch_main"}}
        (service / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        run("/usr/bin/codesign", "--force", "--sign", "-", "--entitlements", str(entitlements), str(service))
    run("/usr/bin/codesign", "--force", "--sign", "-", str(contents / "MacOS/tr-worker"))
    run("/usr/bin/codesign", "--force", "--sign", "-", str(bundle))
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle))
    print(bundle)


if __name__ == "__main__":
    main()
