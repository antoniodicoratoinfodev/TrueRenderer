#!/usr/bin/env python3
"""Compare direct Experimental NEF XPC exports with independent Adobe DNGs.

Input manifest: nef, nef_reference (one object per NEF, source/source_sha256,
dng/dng_sha256), corpus (expected refusals), dng, convert_d750 (empty), provenance.
All photographic assets and reports stay under var/. Requires numpy/tifffile
only in the offline test environment. The SDK is never used by the product.
"""
import argparse
import importlib
import json
from pathlib import Path
import subprocess

import numpy as np
import tifffile

dng = importlib.import_module("test-experimental-dng")


def rationals(value):
    return np.array(value[::2], dtype=float) / np.array(value[1::2], dtype=float)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--root", required=True, type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    dng.check(root.is_relative_to(dng.ROOT / "var") and not root.exists(),
              "Use a new campaign directory inside var/")
    manifest = json.loads(args.manifest.read_text())
    pairs = manifest["nef_reference"]
    dng.check(manifest["nef"] and manifest["provenance"] and not manifest["convert_d750"],
              "Independent references and explicit provenance required")
    dng.check([p["source"] for p in pairs] == manifest["nef"], "Reference pairs differ")
    for pair in pairs:
        for key, hash_key in [("source", "source_sha256"), ("dng", "dng_sha256")]:
            dng.check(dng.digest(Path(pair[key])) == pair[hash_key], "Input hash mismatch")
    root.mkdir(parents=True)
    reference_dir = root / "reference"
    reference_dir.mkdir()
    bundle = args.bundle.resolve()
    executables = [bundle / "Contents/MacOS/TrueRenderer"] + [
        bundle / f"Contents/XPCServices/Decoder{s}.xpc/Contents/MacOS/Decoder" for s in (0, 1)]
    hashes = {str(p): dng.digest(p) for p in executables}
    summary = {"passed": False, "complete": False, "cases": [],
               "binary_sha256": hashes, "sdk_sha256": dng.digest(args.reference),
               "provenance": manifest["provenance"],
               "scope": "Direct NEF codes and sensor calibration versus independent Adobe conversions. Stage 2 is the SDK's linearization, not a measured colour or renderer-accuracy result. Compatibility D65 profile differs from Adobe's dual-illuminant profile."}
    report = root / "nef-reference.json"
    report.write_text(json.dumps(summary, indent=2) + "\n")
    with (root / "xpc.log").open("w") as log:
        subprocess.run([str(executables[0]), "--root", str(root), "--verify-experimental",
                        str(args.manifest.resolve())], check=True, stdout=log, stderr=subprocess.STDOUT)
    campaign = json.loads((root / "reports/experimental-private.json").read_text())
    dng.check(campaign["passed"] and campaign["complete"], "XPC campaign incomplete")
    dng.check(len(campaign["nef"]) == len(pairs), "NEF case count differs")
    for i, (pair, result) in enumerate(zip(pairs, campaign["nef"])):
        print(f"NEF reference {i+1}/{len(pairs)}", flush=True)
        source, reference = Path(pair["source"]), Path(pair["dng"])
        dng.check(result["source"] == str(source) and result["sha256"] == pair["source_sha256"],
                  "NEF source identity differs")
        expected_meta = dng.metadata(reference)
        stages, _ = dng.validate(args.reference, reference, reference_dir / f"{i}-adobe", [])
        case = {"source_sha256": pair["source_sha256"], "reference_sha256": pair["dng_sha256"], "slots": []}
        for slot in result["cases"]:
            copy = Path(slot["repacked_path"])
            dng.check(dng.digest(copy) == slot["repacked_sha256"], "Export hash changed")
            meta = dng.metadata(copy)
            for tag in [256, 257, 274, 33421, 33422, 50717, 50829]:
                dng.check(meta[tag] == expected_meta[tag], f"Sensor metadata differs: {tag}")
            # Adobe stores these values as rationals, Experimental uses exact
            # integer tags. Compare meaning, not representation or type.
            with tifffile.TiffFile(reference) as tiff:
                raw = next(p for p in tiff.pages[0].pages if p.photometric == 32803)
                for tag in [50714, 50719, 50720]:
                    expected = expected_meta[tag]
                    if raw.tags[tag].dtype == 5:
                        expected = rationals(expected)
                    dng.check(np.array_equal(meta[tag], expected), f"Calibration differs: {tag}")
            neutral_error = float(np.max(np.abs(rationals(meta[50728]) - rationals(expected_meta[50728]))))
            dng.check(neutral_error <= 0.00000051, "As-shot neutral differs beyond Adobe 1e-6 rounding")
            actual_stages, _ = dng.validate(args.reference, copy, reference_dir / f"{i}-slot{slot['slot']}", [])
            errors = [dng.compare(a, b) for a, b in zip(stages, actual_stages)]
            case["slots"].append({"slot": slot["slot"], "stages": errors,
                                  "neutral_max_error": neutral_error, "calibration_equal": True})
        summary["cases"].append(case)
        report.write_text(json.dumps(summary, indent=2) + "\n")
    for pair in pairs:
        dng.check(dng.digest(Path(pair["source"])) == pair["source_sha256"], "Original changed")
        dng.check(dng.digest(Path(pair["dng"])) == pair["dng_sha256"], "Reference changed")
    dng.check(all(dng.digest(Path(p)) == h for p, h in hashes.items()), "Bundle changed during campaign")
    summary.update(passed=True, complete=True, originals_unchanged=True)
    report.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"NEF differential campaign passed: {report}")


if __name__ == "__main__":
    main()
