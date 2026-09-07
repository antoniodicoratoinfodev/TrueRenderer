#!/usr/bin/env python3
"""Qualify the installed bundle with native tests, Finder and an independent copy.

Run in a normal macOS desktop session, after build-macos.sh and
generate-format-fixtures.py. Uses separate test databases under var/.
"""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "dist/TrueRenderer.app"
REPORTS = ROOT / "reports"


def run(*args, timeout=150):
    subprocess.run([str(a) for a in args], cwd=ROOT, check=True, timeout=timeout)


RUN_STARTED = time.time()


def report(path, version):
    assert path.stat().st_mtime >= RUN_STARTED, f"Stale report: {path}"
    value = json.loads(path.read_text())
    assert value["passed"] and value["version"] == version, path
    return value


def main():
    if sys.platform != "darwin":
        raise SystemExit("This suite requires a macOS desktop session.")
    assert (ROOT / "var/format-fixtures/manifest.json").is_file(), "Generate format fixtures first"
    version = plistlib.loads((BUNDLE / "Contents/Info.plist").read_bytes())["CFBundleShortVersionString"]
    executable = BUNDLE / "Contents/MacOS/TrueRenderer"
    run(executable, "--verify-resampling")
    run(executable, "--verify-cache")
    report(REPORTS / "cache-macos.json", version)
    run(executable, "--verify-formats")
    report(REPORTS / "formats-macos.json", version)
    run(sys.executable, ROOT / "scripts/test-xpc-integration.py")

    # LaunchServices starts the installed app; its smoke library is separate.
    run("/usr/bin/open", "-n", "-W", BUNDLE, "--args", "--sampling-smoke")
    report(REPORTS / "smoke-macos.json", version)
    shutil.copy2(REPORTS / "smoke-macos.json", REPORTS / "finder-macos.json")
    run(executable, "--verify-sampling-screenshots")
    run("/usr/bin/open", "-n", "-W", BUNDLE, "--args", "--formats-smoke")
    report(REPORTS / "formats-smoke-macos.json", version)

    run("/usr/bin/open", "-n", "-W", BUNDLE, "--args", "--settings-smoke")
    report(REPORTS / "cache-settings-macos.json", version)

    relocated = Path(tempfile.mkdtemp(prefix=f"relocation-{version}-", dir=ROOT / "var"))
    copied = relocated / "dist/TrueRenderer.app"
    copied.parent.mkdir()
    shutil.copytree(BUNDLE, copied)
    shutil.copytree(ROOT / "corpus", relocated / "corpus", ignore=shutil.ignore_patterns(".truerenderer-cache"))
    run("/usr/bin/open", "-n", "-W", copied, "--args", "--smoke-test")
    report(relocated / "reports/smoke-macos.json", version)
    shutil.copy2(relocated / "reports/smoke-macos.json", REPORTS / "relocation-macos.json")
    for name in ["library.sqlite", "index.sqlite"]:
        database = relocated / "var/smoke" / name
        assert database.is_file() and not database.with_name(name + "-wal").exists()
        with sqlite3.connect(database.as_uri() + "?mode=ro&immutable=1", uri=True) as connection:
            assert connection.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
    for path in ["Contents/MacOS/TrueRenderer", "Contents/MacOS/tr-worker",
                 "Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder",
                 "Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder"]:
        assert hashlib.sha256((BUNDLE / path).read_bytes()).digest() == hashlib.sha256((copied / path).read_bytes()).digest()
    (REPORTS / "relocation-check.json").write_text(json.dumps({
        "application": "TrueRenderer", "version": version, "passed": True,
        "root": str(relocated.relative_to(ROOT)), "stage": "passed",
        "completed_at": datetime.now(timezone.utc).isoformat(),
        "runtime_payload": "independent copies of bundle and corpus, no symlink",
        "scope": "runtime root discovery and embedded XPC; not toolchain relocation",
        "smoke_report": "relocation-macos.json",
        "database_created_under_relocated_root": True, "database_integrity": "ok",
        "database_integrity_tool": sqlite3.sqlite_version,
        "database_integrity_scope": "closed checkpointed files, no WAL; read as immutable",
        "copied_binaries_match": True,
    }, indent=2) + "\n")
    run(sys.executable, ROOT / "scripts/verify-package-macos.py")
    print(f"Installed TrueRenderer {version}: all native checks passed", flush=True)


if __name__ == "__main__":
    main()
