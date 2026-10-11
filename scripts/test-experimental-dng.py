#!/usr/bin/env python3
"""Compare Experimental XPC exports with an offline Adobe dng_validate.

Requires numpy/tifffile in the test environment, never in the application.
Use the existing --verify-experimental JSON manifest with supplied dng paths
and explicit provenance; convert_d750 must be empty for this independent test.
All full-source TrueRenderer decoding stays inside the bundle's XPC services.
Reports and SDK stage TIFFs may contain private image data: keep them in var/.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

import numpy as np
import tifffile

ROOT = Path(__file__).resolve().parents[1]
PROFILE_TAGS = [50708, 50721, 50722, 50723, 50724, 50727, 50728, 50729,
                50778, 50779, 50931, 50932, 50936, 50941, 50942, 50964, 50965]
SENSOR_TAGS = [33421, 33422, 50710, 50711, 50712, 50713, 50714, 50715,
               50716, 50717, 50718, 50719, 50720, 50829]


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def check(condition, message):
    if not condition:
        raise ValueError(message)


def metadata(path):
    with tifffile.TiffFile(path) as tiff:
        root = tiff.pages[0]
        pending, raw = list(tiff.pages), []
        while pending:
            page = pending.pop()
            if page.pages is not None:
                pending.extend(page.pages)
            if (page.tags.valueof(254, 0) == 0
                    and page.tags.valueof(262) == 32803):
                raw.append(page)
        check(len(raw) == 1, "Expected one primary CFA image")
        values = {tag: root.tags[tag].value for tag in PROFILE_TAGS if tag in root.tags}
        values.update({tag: raw[0].tags[tag].value for tag in SENSOR_TAGS if tag in raw[0].tags})
        values[274] = root.tags.valueof(274, 1)
        values[256] = raw[0].imagewidth
        values[257] = raw[0].imagelength
        return values


def validate(reference, source, prefix, expected_warnings):
    log = prefix.with_suffix(".log")
    command = [str(reference), "-v", "-1", str(prefix) + "-stage1",
               "-2", str(prefix) + "-stage2", str(source)]
    with log.open("w") as output:
        # SDK verbose stdout and stderr can interleave inside a hex-dump line,
        # concealing a diagnostic from a line-prefix filter. Keep them separate.
        result = subprocess.run(command, stdout=output, stderr=subprocess.PIPE,
                                text=True, timeout=180)
        output.write("\n" + result.stderr)
    text = log.read_text(errors="replace")
    check(result.returncode == 0 and "Validation complete" in text,
          f"DNG SDK failed: {log}")
    diagnostics = re.findall(r"\*\*\* (?:Warning|Error): [^\r\n]*?\*\*\*", text)
    check(all(line.startswith("*** Warning:") for line in expected_warnings),
          "SDK errors cannot be waived")
    check(sorted(diagnostics) == sorted(expected_warnings),
          f"Unexpected DNG SDK diagnostics in {log}: {diagnostics}")
    return [Path(str(prefix) + f"-stage{stage}.tif") for stage in (1, 2)], diagnostics


def compare(expected_path, actual_path):
    expected, actual = tifffile.imread(expected_path), tifffile.imread(actual_path)
    check(expected.shape == actual.shape and expected.dtype == actual.dtype,
          "SDK stage geometry/type differs")
    check(expected.ndim == 2 and expected.dtype == np.dtype("uint16"),
          "This benchmark requires uint16 single-plane SDK stages")
    maximum = 0
    for row in range(0, expected.shape[0], 64):
        difference = (expected[row:row + 64].astype(np.int32)
                      - actual[row:row + 64].astype(np.int32))
        maximum = max(maximum, int(np.abs(difference).max()))
    check(maximum == 0, f"SDK samples differ by {maximum}")
    canonical = np.ascontiguousarray(expected, dtype="<u2")
    return {"width": expected.shape[1], "height": expected.shape[0],
            "samples": expected.size, "maximum_error_u16": maximum,
            "samples_le_u16_sha256": hashlib.sha256(memoryview(canonical).cast("B")).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--root", required=True, type=Path, help="New directory under project var/")
    args = parser.parse_args()
    bundle, reference, manifest_path = [p.resolve(strict=True) for p in
                                        (args.bundle, args.reference, args.manifest)]
    root = args.root.resolve()
    check(ROOT / "var" in root.parents and not root.exists(), "Use a new isolated root under var/")
    manifest = json.loads(manifest_path.read_text())
    check(not manifest["convert_d750"] and manifest["dng"] and manifest.get("provenance"),
          "Independent DNG inputs and provenance are required; legacy conversion must be empty")
    sources = [Path(p).resolve(strict=True) for p in manifest["dng"]]
    check(len(set(sources)) == len(sources), "Duplicate DNG inputs")
    before = {str(p): digest(p) for p in sources}
    root.mkdir(parents=True)
    (root / "reference").mkdir()
    report_path = root / "dng-reference.json"
    report = {"passed": False, "complete": False, "cases": [],
              "reference_sha256": digest(reference), "provenance": manifest["provenance"],
              "limits": "Stored sensor and SDK linearization roundtrip only; not renderer colour accuracy, native NEF decoding, performance or physical memory qualification."}

    def save():
        report_path.write_text(json.dumps(report, indent=2) + "\n")

    save()
    try:
        executable = bundle / "Contents/MacOS/TrueRenderer"
        report["binary_sha256"] = {str(p.relative_to(bundle)): digest(p) for p in
                                  [executable, *(bundle / f"Contents/XPCServices/Decoder{s}.xpc/Contents/MacOS/Decoder" for s in (0, 1))]}
        with (root / "experimental.log").open("w") as output:
            subprocess.run([str(executable), "--root", str(root), "--verify-experimental",
                            str(manifest_path)], stdout=output, stderr=subprocess.STDOUT,
                           timeout=1800, check=True)
        campaign = json.loads((root / "reports/experimental-private.json").read_text())
        check(campaign["passed"] and campaign["complete"], "Incomplete XPC campaign")
        check({r["sha256"] for r in campaign["dng"]} == set(before.values()), "Campaign inputs differ")
        for i, record in enumerate(campaign["dng"]):
            source = Path(record["source"]).resolve()
            warnings = manifest.get("reference_expected_warnings", {}).get(str(source), [])
            original, diagnostics = validate(reference, source, root / "reference" / f"{i}-source", warnings)
            original_metadata = metadata(source)
            case = {"source": str(source), "sha256": record["sha256"],
                    "reference_warnings": diagnostics, "slots": []}
            check({v["slot"] for v in record["cases"]} == {0, 1}, "Both XPC services are required")
            for slot in record["cases"]:
                repacked = Path(slot["repacked_path"]).resolve(strict=True)
                check(root in repacked.parents, "Export outside isolated root")
                check(digest(repacked) == slot["repacked_sha256"], "Export digest differs")
                exported_metadata = metadata(repacked)
                check(all(exported_metadata.get(k) == v for k, v in original_metadata.items()),
                      "Export changed supported sensor/profile metadata")
                stages, diagnostics = validate(reference, repacked, root / "reference" / f"{i}-slot{slot['slot']}", warnings)
                comparisons = [compare(a, b) for a, b in zip(original, stages)]
                case["slots"].append({"slot": slot["slot"], "stage1": comparisons[0],
                                      "stage2": comparisons[1], "metadata_preserved": True,
                                      "reference_warnings": diagnostics})
            report["cases"].append(case)
            save()
            print(f"DNG {i + 1}/{len(campaign['dng'])}: two slots, exact SDK stages", flush=True)
        check(all(digest(Path(p)) == sha for p, sha in before.items()), "Input changed")
        report.update(passed=True, complete=True, originals_unchanged=True)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        save()


if __name__ == "__main__":
    main()
