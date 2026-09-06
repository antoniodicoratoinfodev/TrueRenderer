#!/usr/bin/env python3
"""Record the installed runtime after successful Finder and relocation smoke tests."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import plistlib
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def command(*args):
    return subprocess.run(args, capture_output=True, check=True).stdout


def main():
    bundle = ROOT / "dist/TrueRenderer.app"
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    version = info["CFBundleShortVersionString"]
    command("/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle))
    for report_name in ["finder-macos.json", "relocation-macos.json", "xpc-qualification-macos.json", "resampling-macos.json", "sampling-presentation-macos.json"]:
        report = json.loads((ROOT / "reports" / report_name).read_text())
        assert report["passed"] and report["version"] == version, report_name
        if "worker_transports" in report:
            assert report["worker_transports"] == ["XPC / App Sandbox · R0"]
            assert len(report["worker_pids"]) == 2
    paths = ["Contents/MacOS/TrueRenderer", "Contents/MacOS/tr-worker"]
    services = []
    for slot in range(2):
        relative = f"Contents/XPCServices/Decoder{slot}.xpc"
        service = bundle / relative
        service_info = plistlib.loads((service / "Contents/Info.plist").read_bytes())
        assert service_info["CFBundleShortVersionString"] == version
        assert service_info["CFBundleIdentifier"] == f"it.truerenderer.prototype.decoder.{slot}"
        entitlements = plistlib.loads(command("/usr/bin/codesign", "-d", "--entitlements", ":-", str(service)))
        assert entitlements == {"com.apple.security.app-sandbox": True}, entitlements
        services.append({"bundle": relative, "identifier": service_info["CFBundleIdentifier"], "entitlements": entitlements})
        paths.append(relative + "/Contents/MacOS/Decoder")
    binaries = {}
    for relative in paths:
        path = bundle / relative
        architecture = command("/usr/bin/lipo", "-archs", str(path)).decode().strip()
        assert architecture == "arm64", (relative, architecture)
        data = path.read_bytes()
        binaries[relative] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "architecture": architecture}
    qualification = json.loads((ROOT / "reports/xpc-qualification-macos.json").read_text())
    for relative, digest in qualification["binary_sha256"].items():
        assert binaries[relative]["sha256"] == digest, "Installed executable differs from the qualified bundle"
    result = {
        "application": "TrueRenderer", "version": version, "passed": True,
        "checked_at": datetime.now(timezone.utc).isoformat(),
        "format": "macOS arm64 app bundle", "bundle": "dist/TrueRenderer.app",
        "signature": "ad hoc; verified with codesign --deep --strict",
        "finder_smoke": "passed via LaunchServices", "finder_smoke_report": "finder-macos.json",
        "relocation_smoke": "passed with independent bundle and corpus copies", "relocation_smoke_report": "relocation-macos.json",
        "embedded_services": services, "binaries": binaries,
        "scope": "internal macOS R0 prototype; distribution signer, supported OS range and full sandbox gate remain open",
    }
    (ROOT / "reports/package-macos.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
