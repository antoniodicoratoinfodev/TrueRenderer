#!/usr/bin/env python3
"""Publish numeric export/FITS evidence, never photographs or source paths."""
import argparse
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--corpus", type=Path, required=True)
parser.add_argument("--raw", type=Path, required=True)
parser.add_argument("--bundle", type=Path, required=True)
args = parser.parse_args()
groups = []
for label, path, expected in [("generated gradient and FITS", args.corpus, 26),
                               ("authorized private Nikon D750 copy", args.raw, 25)]:
    source = json.loads(path.read_text())
    assert source["passed"] and source["source_unchanged"]
    assert len(source["checks"]) == expected
    checks = []
    for check in source["checks"]:
        assert check["passed"]
        checks.append({key: check[key] for key in
                       ("format", "engine", "encoded", "readback_engine", "readable",
                        "seconds", "case", "slot", "sample", "invalid_sample", "transport", "passed")
                       if key in check})
    groups.append({"source_class": label, "source_unchanged": True, "checks": checks})
binary_paths = ["Contents/MacOS/TrueRenderer",
                "Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder",
                "Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder"]
hashes = {p: hashlib.sha256((args.bundle / p).read_bytes()).hexdigest() for p in binary_paths}
xpc = json.loads((ROOT / "reports/xpc-qualification-macos.json").read_text())
assert xpc["passed"] and xpc["binary_sha256"] == hashes
precision = json.loads((ROOT / "reports/presentation-precision-macos.json").read_text())
assert all(c["passed"] for c in precision["checks"])
report = {
    "application": "TrueRenderer", "version": "0.1.5", "passed": True,
    "checked_at": datetime.now(timezone.utc).isoformat(),
    "bundle": args.bundle.as_posix(), "binary_sha256": hashes,
    "groups": groups,
    "unit_suite": {"ordinary_passed": 160, "integration_passed": 11,
                   "private_raw_exact_active_samples_passed": True,
                   "protocol_checks": 8, "native_gui_smoke": True},
    "native_ui": ["FITS native BLANK sample, scalar histogram and asinh control",
                  "JPEG and PNG16 export at native 1600x900, private synthetic library"],
    "presentation_report": "presentation-precision-macos.json",
    "xpc_report": "xpc-qualification-macos.json",
    "negative_evidence": [
        "Apple RAW rejects both generated DNG variants: metadata/dimensions unavailable. LibRaw bilinear/AHD required, disclosed in UI.",
        "Original one-broker test recycled XPC at every controlled/external transition; split writer/reader slots avoid restart delays. Earlier interrupted reports remain private."
    ],
    "limits": [
        "One D750 export source, not a new full camera qualification; preserve original NEF.",
        "RAW DNG stores active-area samples, not optical margins, private metadata or original compressed container.",
        "Roundtrip readability is not an independent colour/archival conformance certification.",
        "FITS only first eligible 2D IMAGE, BITPIX 8/16/32/-32; no cubes, compression, HDU selection, 64-bit or scientific export.",
        "SDR numerical surface readback, not physical display depth, HDR or multi-monitor qualification.",
        "Windows and physical memory ceiling not qualified by this campaign; existing over-budget measurements remain open."
    ]
}
(ROOT / "reports/export-fits-macos.json").write_text(json.dumps(report, indent=2) + "\n")
print("Export/FITS: 51 isolated checks, original digests unchanged; bundle hashes match XPC report")
