#!/usr/bin/env python3
from pathlib import Path
import plistlib
import subprocess

root = Path(__file__).resolve().parents[1]
bundle = root / "var/control-probe/ControlProbe.app"
service = bundle / "Contents/XPCServices/ControlWorker.xpc"
for path, source, name, identifier, kind in (
    (service, "control-worker.m", "ControlWorker", "it.truerenderer.controlprobe.worker", "XPC!"),
    (bundle, "control-host.m", "ControlProbe", "it.truerenderer.controlprobe", "APPL"),
):
    (path / "Contents/MacOS").mkdir(parents=True, exist_ok=True)
    subprocess.run(["xcrun", "clang", "-fobjc-arc", "-fblocks", "-O2", "-Wall", "-Wextra", "-Werror",
                    "-mmacosx-version-min=13.0", "-framework", "Foundation", "-framework", "Security",
                    str(root / "experiments/macos-xpc" / source), str(root / "native/macos/signing.m"),
                    "-o", str(path / "Contents/MacOS" / name)], check=True)
    info = {"CFBundleExecutable": name, "CFBundleIdentifier": identifier, "CFBundlePackageType": kind,
            "CFBundleVersion": "1"}
    if kind == "XPC!":
        info["XPCService"] = {"ServiceType": "Application", "RunLoopType": "dispatch_main"}
    (path / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
entitlements = bundle.parent / "worker.entitlements.plist"
entitlements.write_bytes(plistlib.dumps({"com.apple.security.app-sandbox": True}))
subprocess.run(["codesign", "--force", "--sign", "-", "--entitlements", str(entitlements), str(service)], check=True)
subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], check=True)
subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
print(bundle)
