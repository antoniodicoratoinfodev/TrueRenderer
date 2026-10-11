#!/usr/bin/env python3
"""Direct NEF/RAF XPC campaign versus independent Adobe DNG sensor planes.

Manifest: nef, raf, raw_reference [{source, source_sha256, dng, dng_sha256,
reference_window:[left,top,width,height]}], dng, corpus, convert_d750:[], provenance.
Adobe may omit sensor margins: the explicit window is compared at stage 1;
stage 2 compares the complete active planes. No colour-fidelity claim follows.
"""
import argparse
import hashlib
import importlib
import json
from pathlib import Path
import subprocess
import struct

import numpy as np
import tifffile

dng = importlib.import_module("test-experimental-dng")


def values(page, tag, default=None):
    field = page.tags.get(tag)
    if field is None:
        return np.atleast_1d(default)
    v = np.atleast_1d(field.value)
    return v[::2] / v[1::2] if field.dtype in (5, 10) else v


def sensor_metadata(path):
    with tifffile.TiffFile(path) as f:
        root = f.pages[0]
        pending, raw = list(f.pages), []
        while pending:
            page = pending.pop()
            if page.pages is not None:
                pending.extend(page.pages)
            if page.tags.valueof(254, 0) == 0 and page.photometric == 32803:
                raw.append(page)
        dng.check(len(raw) == 1, "Ambiguous raw image")
        page = raw[0]
        area = values(page, 50829, [0, 0, page.imagelength, page.imagewidth])
        return {"width": page.imagewidth, "height": page.imagelength,
                "orientation": values(root, 274, 1),
                "active_size": area[2:] - area[:2],
                "cfa_repeat": values(page, 33421),
                "cfa": bytes(page.tags[33422].value),
                "black": values(page, 50714, 0).reshape(tuple(values(page, 50713, [1, 1]).astype(int))),
                "white": values(page, 50717),
                "crop_origin": values(page, 50719), "crop_size": values(page, 50720),
                "neutral": values(root, 50728)}


def compare_window(expected_path, actual_path, window):
    expected = tifffile.imread(expected_path)
    actual = tifffile.imread(actual_path)
    original_size = actual.size
    if window is not None:
        x, y, w, h = window
        dng.check(x >= 0 and y >= 0 and w > 0 and h > 0
                  and x+w <= actual.shape[1] and y+h <= actual.shape[0], "Bad reference window")
        actual = actual[y:y+h, x:x+w]
    dng.check(expected.shape == actual.shape and expected.dtype == actual.dtype == np.dtype("uint16")
              and expected.ndim == 2, "Reference plane geometry/type differs")
    maximum = 0
    for y in range(0, len(expected), 64):
        maximum = max(maximum, int(np.abs(expected[y:y+64].astype(np.int32)
                                         - actual[y:y+64].astype(np.int32)).max()))
    dng.check(maximum == 0, f"Sensor plane differs by {maximum}")
    return {"width": expected.shape[1], "height": expected.shape[0], "samples": expected.size,
            "maximum_error_u16": maximum, "excluded_margin_samples": original_size-actual.size,
            "samples_le_u16_sha256": hashlib.sha256(np.ascontiguousarray(expected, dtype="<u2").tobytes()).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["bundle", "reference", "manifest", "root"]:
        parser.add_argument("--"+name, required=True, type=Path)
    parser.add_argument("--campaign-report", type=Path, help="Reuse an existing private XPC report for reference-only diagnostics")
    parser.add_argument("--discard-intermediates", action="store_true", help="Remove only this run's generated stage TIFFs after a successful case")
    args = parser.parse_args()
    root = args.root.resolve()
    dng.check(root.is_relative_to(dng.ROOT/"var") and not root.exists(), "Use a new root under var/")
    manifest = json.loads(args.manifest.read_text())
    pairs = manifest["raw_reference"]
    paths = manifest.get("nef", []) + manifest.get("raf", [])
    dng.check(pairs and manifest["provenance"] and not manifest["convert_d750"]
              and [p["source"] for p in pairs] == paths, "Invalid reference manifest")
    for pair in pairs:
        for key in ["source", "dng"]:
            dng.check(dng.digest(Path(pair[key])) == pair[key+"_sha256"], "Input hash mismatch")
    root.mkdir(parents=True)
    references = root/"reference"
    references.mkdir()
    bundle = args.bundle.resolve()
    binaries = [bundle/"Contents/MacOS/TrueRenderer"] + [bundle/f"Contents/XPCServices/Decoder{s}.xpc/Contents/MacOS/Decoder" for s in (0, 1)]
    hashes = {str(p): dng.digest(p) for p in binaries}
    summary = {"passed": False, "complete": False, "cases": [], "binary_sha256": hashes,
               "sdk_sha256": dng.digest(args.reference), "provenance": manifest["provenance"],
               "reused_campaign": str(args.campaign_report) if args.campaign_report else None,
               "limits": "Adobe retained sensor window only at stage 1. Full active SDK linearization at stage 2. Reconstructed lossy codes remain lossy. D65 compatibility profiles differ from Adobe; no measured colour/demosaic qualification."}
    report = root/"raw-reference.json"
    report.write_text(json.dumps(summary, indent=2)+"\n")
    if args.campaign_report:
        campaign_path = args.campaign_report
    else:
        with (root/"xpc.log").open("w") as log:
            subprocess.run([str(binaries[0]), "--root", str(root), "--verify-experimental", str(args.manifest.resolve())],
                           check=True, stdout=log, stderr=subprocess.STDOUT, timeout=3600)
        campaign_path = root/"reports/experimental-private.json"
    campaign = json.loads(campaign_path.read_text())
    dng.check(campaign["passed"] and campaign["complete"], "Incomplete XPC campaign")
    cases = campaign.get("nef", []) + campaign.get("raf", [])
    dng.check(len(cases) == len(pairs), "Case count differs")
    for i, (pair, result) in enumerate(zip(pairs, cases)):
        print(f"RAW reference {i+1}/{len(pairs)}", flush=True)
        dng.check(result["source"] == pair["source"] and result["sha256"] == pair["source_sha256"], "Source identity differs")
        expected = sensor_metadata(pair["dng"])
        stages, reference_warnings = dng.validate(args.reference, pair["dng"], references/f"{i}-adobe", pair.get("reference_warnings", []))
        intermediates = list(stages)
        case = {"source_sha256": pair["source_sha256"], "reference_sha256": pair["dng_sha256"], "reference_warnings": reference_warnings, "slots": []}
        if "linearization_reference" in pair:
            derived = pair["linearization_reference"]
            path = Path(derived["path"])
            dng.check(dng.digest(path) == derived["sha256"], "Derived reference changed")
            with tifffile.TiffFile(pair["dng"]) as original, tifffile.TiffFile(path) as copy:
                old = next(p for p in original.pages[0].pages if p.photometric == 32803)
                new = next(p for p in copy.pages[0].pages if p.photometric == 32803)
                for tag, identifiers in derived["omitted_optional_opcodes"].items():
                    tag = int(tag)
                    dng.check(tag in (51009, 51022), "Only post-unpack optional lists may be omitted")
                    payload = old.tags[tag].value
                    count, identifier, version, flags, length = struct.unpack_from(">5I", payload)
                    dng.check(count == 1 and identifiers == [identifier] and flags == 1
                              and identifier == (3 if tag == 51009 else 1)
                              and length + 20 == len(payload) and new.tags[tag].value == bytes(4),
                              "Unexpected/mandatory opcode removal")
                for tag in old.tags:
                    if str(tag.code) not in derived["omitted_optional_opcodes"]:
                        other = new.tags[tag.code]
                        dng.check(tag.dtype == other.dtype and tag.count == other.count
                                  and tag.value == other.value, "Unrelated reference metadata changed")
            baseline, baseline_warnings = dng.validate(args.reference, path, references/f"{i}-calibration", pair.get("reference_warnings", []))
            intermediates.extend(baseline)
            dng.compare(stages[0], baseline[0])
            stages[1] = baseline[1]
            case["linearization_reference"] = derived
            case["linearization_warnings"] = baseline_warnings
        for slot in result["cases"]:
            copy = Path(slot["repacked_path"])
            dng.check(dng.digest(copy) == slot["repacked_sha256"], "Export changed")
            actual = sensor_metadata(copy)
            for key in ["orientation", "active_size", "cfa_repeat", "cfa", "white", "crop_origin", "crop_size"]:
                dng.check(np.array_equal(actual[key], expected[key]), f"Metadata differs: {key}")
            # Repeat dimensions may be represented differently for uniform black.
            rows, cols = np.lcm(actual["black"].shape, expected["black"].shape)
            dng.check(np.array_equal(np.tile(actual["black"], (rows//actual["black"].shape[0], cols//actual["black"].shape[1])),
                                    np.tile(expected["black"], (rows//expected["black"].shape[0], cols//expected["black"].shape[1]))), "Black levels differ")
            neutral_error = float(np.abs(actual["neutral"]-expected["neutral"]).max())
            dng.check(neutral_error <= 0.00000051, "WB differs beyond Adobe rounding")
            outputs, warnings = dng.validate(args.reference, copy, references/f"{i}-slot{slot['slot']}", pair.get("export_warnings", []))
            intermediates.extend(outputs)
            errors = [compare_window(stages[0], outputs[0], pair["reference_window"]), compare_window(stages[1], outputs[1], None)]
            case["slots"].append({"slot": slot["slot"], "stages": errors, "neutral_max_error": neutral_error, "calibration_equal": True, "warnings": warnings})
        summary["cases"].append(case)
        report.write_text(json.dumps(summary, indent=2)+"\n")
        if args.discard_intermediates:
            case["removed_stage_tiffs"] = [{"file": p.name, "sha256": dng.digest(p), "bytes": p.stat().st_size} for p in intermediates]
            for p in intermediates:
                dng.check(p.parent == references and p.suffix == ".tif", "Unsafe intermediate cleanup")
                p.unlink()
            report.write_text(json.dumps(summary, indent=2)+"\n")
    for pair in pairs:
        for key in ["source", "dng"]:
            dng.check(dng.digest(Path(pair[key])) == pair[key+"_sha256"], "Original/reference changed")
    dng.check(all(dng.digest(Path(p)) == h for p, h in hashes.items()), "Bundle changed")
    summary.update(passed=True, complete=True, originals_unchanged=True)
    report.write_text(json.dumps(summary, indent=2)+"\n")
    print(f"RAW differential campaign passed: {report}")


if __name__ == "__main__":
    main()
