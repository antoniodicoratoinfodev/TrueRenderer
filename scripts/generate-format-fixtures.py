#!/usr/bin/env python3
"""Generate every format fixture locally; no photographs are downloaded."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FOLDER = ROOT / "var/format-fixtures"


def main():
    if sys.platform != "darwin":
        raise SystemExit("These ImageIO fixtures require a macOS desktop session.")
    webp = shutil.which("cwebp")
    if not webp:
        raise SystemExit("The fixture generator requires cwebp (libwebp tools). The app itself does not.")
    FOLDER.mkdir(parents=True, exist_ok=True)
    generator = ROOT / "var/generate-format-fixtures"
    subprocess.run([
        "xcrun", "clang", "-fobjc-arc", "-O2", "-Wall", "-Wextra", "-Werror",
        "-framework", "Foundation", "-framework", "CoreGraphics",
        "-framework", "ImageIO", "-framework", "UniformTypeIdentifiers",
        str(ROOT / "scripts/generate-format-fixtures.m"), "-o", str(generator),
    ], check=True)
    subprocess.run([str(generator), str(FOLDER)], check=True)
    subprocess.run([sys.executable, str(ROOT / "scripts/generate-dng-fixture.py")], check=True)
    subprocess.run([webp, "-quiet", "-lossless", str(FOLDER / "02-png.png"),
                    "-o", str(FOLDER / "08-webp.webp")], check=True)
    manifest_path = FOLDER / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest.append({"file": "08-webp.webp", "width": 48, "height": 32,
                     "source_bits": 8, "orientation": 1,
                     "generator": "own analytic quadrants encoded losslessly with cwebp"})
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Generated {len(manifest)} own fixtures in {FOLDER}")


if __name__ == "__main__":
    main()
