#!/usr/bin/env python3
"""Build an isolated native XPC capability lab, without changing the desktop app."""
from pathlib import Path
import argparse
import plistlib
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "experiments/macos-xpc"


def run(*args):
    subprocess.run(args, check=True)


def bundle_info(path, executable, identifier, kind):
    info = {
        "CFBundleExecutable": executable,
        "CFBundleIdentifier": identifier,
        "CFBundleName": executable,
        "CFBundlePackageType": kind,
        "CFBundleVersion": "1",
        "CFBundleShortVersionString": "0.1.0",
        "LSMinimumSystemVersion": "13.0",
    }
    if kind == "XPC!":
        info["XPCService"] = {"ServiceType": "Application", "RunLoopType": "dispatch_main"}
    with (path / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump(info, stream)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--limited", action="store_true", help="apply worker-only RLIMIT_NPROC 0")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise SystemExit("This experiment requires macOS and Command Line Tools.")
    folder = ROOT / "var/sandbox-probe"
    identifier = "it.truerenderer.sandboxprobe"
    definitions = []
    if args.limited:
        folder /= "limited"
        identifier += ".limited"
        definitions = ['-DTR_PROBE_LIMIT_CHILDREN=1', f'-DTR_PROBE_HOST="{identifier}"',
                       f'-DTR_PROBE_SERVICE="{identifier}.worker"']
    BUNDLE = folder / "TrueRendererSandboxProbe.app"
    SERVICE = BUNDLE / "Contents/XPCServices/ProbeWorker.xpc"
    for path in (BUNDLE, SERVICE):
        (path / "Contents/MacOS").mkdir(parents=True, exist_ok=True)
    for source, path, name in (
        ("worker.m", SERVICE, "ProbeWorker"),
        ("host.m", BUNDLE, "TrueRendererSandboxProbe"),
    ):
        run("xcrun", "clang", "-fobjc-arc", "-fblocks", "-O2", "-g",
            "-Wall", "-Wextra", "-Werror", "-mmacosx-version-min=13.0",
            "-framework", "Foundation", "-framework", "Security",
            *definitions, str(SOURCE / source), "-o", str(path / "Contents/MacOS" / name))
    bundle_info(BUNDLE, "TrueRendererSandboxProbe", identifier, "APPL")
    bundle_info(SERVICE, "ProbeWorker", identifier + ".worker", "XPC!")
    entitlements = BUNDLE.parent / "worker.entitlements.plist"
    with entitlements.open("wb") as stream:
        plistlib.dump({"com.apple.security.app-sandbox": True}, stream)
    run("/usr/bin/codesign", "--force", "--sign", "-", "--entitlements", str(entitlements), str(SERVICE))
    run("/usr/bin/codesign", "--force", "--sign", "-", str(BUNDLE))
    run("/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", str(BUNDLE))
    print(BUNDLE)


if __name__ == "__main__":
    main()
