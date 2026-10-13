#!/usr/bin/env python3
"""Diagnostic inter-engine color differences, never camera calibration truth."""
import argparse
from collections import Counter, defaultdict
import hashlib
import importlib
import json
import math
from pathlib import Path
import statistics

color = importlib.import_module("calibrate-raw-profile")
wb = importlib.import_module("analyze-raw-wb")
MATRIX = wb.conversion()
ANCHOR = "TrueRendererExperimental"
ENGINES = (ANCHOR, "Apple", "LibRawAhd", "LibRawBilinear", "TrueRenderer")
PROTOCOL = "TRExp-engine-color-v1"
Y_BINS = (.01, .03, .1, .3, .9)


def summary(values):
    values = sorted(color.number(v) for v in values)
    if not values:
        return {"count": 0, "mean": None, "median": None, "p95": None, "minimum": None, "maximum": None}
    return {"count": len(values), "mean": math.fsum(values) / len(values),
            "median": statistics.median(values), "p95": values[math.ceil(.95 * len(values)) - 1],
            "minimum": values[0], "maximum": values[-1]}


def validate_block(block):
    rgb = color.vector(block["rgb"])
    low, high = color.number(block["minimum"]), color.number(block["maximum"])
    mean, sigma = color.number(block["y_mean"]), color.number(block["y_stddev"])
    if low > min(rgb) + 1e-12 or high < max(rgb) - 1e-12 or sigma < 0:
        raise ValueError("inconsistent linear block moments")
    expected_y = math.fsum(x * y for x, y in zip(rgb, (.2627002120112671, .6779980715188708, .0593017164698620)))
    if abs(mean - expected_y) > max(1e-12, abs(expected_y) * 1e-10):
        raise ValueError("block luminance mismatch")
    return rgb


def domain(block, gain=1.):
    rgb = validate_block(block)
    xyz = color.transform(MATRIX, [v * gain for v in rgb])
    if not (0 < block["minimum"] * gain and block["maximum"] * gain < .98):
        return "sample_range", xyz
    if min(xyz) <= 0 or not .01 <= xyz[1] <= .9:
        return "xyz_domain", xyz
    if block["y_mean"] <= 0 or block["y_stddev"] / block["y_mean"] > .15:
        return "texture_or_noise", xyz
    return None, xyz


def differences(a, b):
    la, lb = color.lab(a), color.lab(b)
    xa, xb = [v / sum(a) for v in a[:2]], [v / sum(b) for v in b[:2]]
    return {"de00": color.delta_e(la, lb), "d_lab": [y - x for x, y in zip(la, lb)],
            "d_xy": [y - x for x, y in zip(xa, xb)], "y_ratio": b[1] / a[1]}


def metrics(rows):
    return {"delta_e00": summary(r["de00"] for r in rows),
            "delta_lab": [summary(r["d_lab"][c] for r in rows) for c in range(3)],
            "delta_xy": [summary(r["d_xy"][c] for r in rows) for c in range(2)],
            "y_ratio": summary(r["y_ratio"] for r in rows)}


def analyze_pair(zones):
    if len(zones) != 9:
        raise ValueError("nine comparison zones required")
    eligible, excluded, geometry, zone_counts = [], Counter(), [], []
    for zi, zone in enumerate(zones):
        reg = zone["registration"]
        dx, dy = reg["dx"], reg["dy"]
        if any(type(v) is not int or abs(v) > 24 for v in (dx, dy)):
            raise ValueError("invalid registration offset")
        if "gradient_cosine" in reg:
            cosine = color.number(reg["gradient_cosine"])
            runner = color.number(reg["distant_runner_up_cosine"])
            gap = color.number(reg["peak_gap"])
            if max(abs(cosine), abs(runner)) > 1.000001 or abs(gap - (cosine - runner)) > 1e-12:
                raise ValueError("inconsistent registration scores")
            boundary = max(abs(dx), abs(dy)) == 24
            supported = cosine >= .85 and gap >= .002 and not boundary
            if reg.get("at_search_boundary") is not boundary:
                raise ValueError("inconsistent registration boundary")
        else:
            supported = False
            if reg.get("reason") != "insufficient gradient energy":
                raise ValueError("missing registration evidence")
        if reg.get("supported") is not supported:
            raise ValueError("inconsistent registration eligibility")
        geometry.append(supported)
        rows = zone["blocks"]
        expected_xy = {(x, y) for y in range(32, 288, 16) for x in range(32, 288, 16)}
        if len(rows) != 256 or {tuple(r["xy"]) for r in rows} != expected_xy:
            raise ValueError("missing or duplicate linear blocks")
        count = 0
        for row in rows:
            # Validate even excluded data: NaN must not silently become an exclusion.
            da, a = domain(row["anchor"])
            db, b = domain(row["comparison"])
            reason = "weak_geometry" if not supported else da or db
            if reason:
                excluded[reason] += 1
                continue
            eligible.append({"zone": zi, "xy": row["xy"], "a": a, "b": b,
                             "block": row["comparison"], "raw": differences(a, b)})
            count += 1
        zone_counts.append(count)
    populated = sum(n > 0 for n in zone_counts)
    sufficient = len(eligible) >= 12 and populated >= 3
    gain = statistics.median(r["a"][1] / r["b"][1] for r in eligible) if sufficient else None
    sufficient = sufficient and 1 / 16 <= gain <= 16
    matched, normalized, low_chroma, by_y = [], [], [], [[] for _ in range(4)]
    after_exclusions = Counter()
    worst = []
    per_zone = [[] for _ in range(9)]
    if sufficient:
        for row in eligible:
            reason, b = domain(row["block"], gain)
            if reason:
                after_exclusions[reason] += 1
                continue
            diff = differences(row["a"], b)
            matched.append(row["raw"])
            normalized.append(diff)
            per_zone[row["zone"]].append(diff)
            la = color.lab(row["a"])
            if math.hypot(la[1], la[2]) <= 8:
                low_chroma.append(diff)
            for i in range(4):
                if Y_BINS[i] <= row["a"][1] < Y_BINS[i + 1] or i == 3 and row["a"][1] == .9:
                    by_y[i].append(diff)
            worst.append({"zone": row["zone"], "xy": row["xy"], **diff})
    return {"sufficient_for_exposure_estimate": sufficient,
            "registered_zones": sum(geometry), "eligible_zones": populated,
            "eligible_blocks_by_zone": zone_counts, "excluded_blocks": dict(excluded),
            "raw": metrics([r["raw"] for r in eligible]),
            "comparison_exposure_gain": gain, "comparison_exposure_ev": math.log2(gain) if gain else None,
            "after_scale_excluded_blocks": dict(after_exclusions),
            "raw_on_normalized_subset": metrics(matched), "exposure_normalized": metrics(normalized),
            "normalized_zones": [metrics(rows) for rows in per_zone],
            "low_chroma_anchor_cab_le_8": metrics(low_chroma),
            "anchor_y_bins": [{"range": list(Y_BINS[i:i + 2]), **metrics(rows)} for i, rows in enumerate(by_y)],
            "largest_normalized_differences": sorted(worst, key=lambda r: r["de00"], reverse=True)[:10]}


def read_json(path):
    return json.loads(path.read_bytes(), parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def local_file(root, name):
    if not isinstance(name, str) or Path(name).name != name or name in (".", ".."):
        raise ValueError("private artifact must be a local filename")
    path = root / name
    if not path.is_file() or path.is_symlink():
        raise ValueError("missing or linked comparison artifact")
    return path


def analyze(root):
    manifest = read_json(root / "manifest.json")
    if manifest.get("protocol") != PROTOCOL or manifest.get("complete") is not True or manifest.get("passed") is not True:
        raise ValueError("complete successful comparison campaign required")
    if [manifest.get(k) for k in ("edge", "margin", "registration_radius", "registration_stride")] != [320, 32, 24, 8]:
        raise ValueError("unexpected comparison geometry")
    entries = manifest["sources"]
    if not 1 <= len(entries) <= 128 or len({r["id"] for r in entries}) != len(entries):
        raise ValueError("empty or duplicate campaign")
    files, all_sources, groups, refusals = [], {}, defaultdict(list), []
    accepted = 0
    for entry in entries:
        path = local_file(root, entry["report"])
        if digest(path) != color.sha256(entry["sha256"]):
            raise ValueError("comparison report hash mismatch")
        source = read_json(path)
        if source["id"] != entry["id"] or source.get("original_unchanged") is not True:
            raise ValueError("unbound or changed comparison source")
        source_hash = color.sha256(source["source_sha256"])
        if source_hash in all_sources or source["kind"] not in ("original", "repacked-control"):
            raise ValueError("duplicate or invalid source")
        if [e["engine"] for e in source["engines"]] != list(ENGINES):
            raise ValueError("incomplete or reordered engines")
        anchor = source["engines"][0]
        if anchor.get("accepted") is not True or anchor["zones"]:
            raise ValueError("missing anchor")
        pairs = []
        for engine in source["engines"]:
            if engine.get("accepted") is not True:
                if engine.get("expected_refusal") is not True:
                    raise ValueError("unexpected decoder failure")
                refusals.append({"id": source["id"], "camera": source["camera"], "kind": source["kind"],
                                 "engine": engine["engine"], "error": engine["error"]})
                continue
            accepted += 1
            color.integers(engine["size"], 2, 320)
            for key in ("minimum", "maximum"):
                color.number(engine["range"][key])
            overview = engine["overview"]
            w, h = color.integers(overview["size"], 2, 1)
            if local_file(root, overview["linear"]).stat().st_size != w * h * 12:
                raise ValueError("overview float byte length mismatch")
            local_file(root, overview["png"])
            if len(engine["crop_pngs"]) != 9:
                raise ValueError("missing private crops")
            for name in engine["crop_pngs"]:
                local_file(root, name)
            if engine["engine"] == ANCHOR:
                continue
            pair = {"engine": engine["engine"], "recipe": engine["recipe"], "size": engine["size"],
                    "input_color": engine["info"]["input_color"], "range": engine["range"],
                    **analyze_pair(engine["zones"])}
            pairs.append(pair)
            groups[source["camera"], source["kind"], engine["engine"]].append(pair)
        result = {k: source[k] for k in ("id", "camera", "kind", "source_sha256", "original_sha256")}
        result.update({"anchor_recipe": anchor["recipe"], "anchor_size": anchor["size"], "pairs": pairs})
        files.append(result)
        all_sources[source_hash] = source
    aggregates = []
    for (camera, kind, engine), pairs in sorted(groups.items()):
        valid = [p for p in pairs if p["exposure_normalized"]["delta_e00"]["count"]]
        aggregates.append({"camera": camera, "kind": kind, "engine": engine, "files": len(pairs),
                           "files_with_normalized_comparison": len(valid),
                           "registered_zones": sum(p["registered_zones"] for p in pairs),
                           "raw_file_medians_delta_e00": summary(p["raw"]["delta_e00"]["median"] for p in pairs if p["raw"]["delta_e00"]["count"]),
                           "matched_raw_file_medians_delta_e00": summary(p["raw_on_normalized_subset"]["delta_e00"]["median"] for p in valid),
                           "normalized_file_medians_delta_e00": summary(p["exposure_normalized"]["delta_e00"]["median"] for p in valid),
                           "exposure_ev": summary(p["comparison_exposure_ev"] for p in valid),
                           "normalized_file_medians_delta_lab": [summary(p["exposure_normalized"]["delta_lab"][c]["median"] for p in valid) for c in range(3)],
                           "low_chroma_file_medians_delta_e00": summary(p["low_chroma_anchor_cab_le_8"]["delta_e00"]["median"] for p in valid if p["low_chroma_anchor_cab_le_8"]["delta_e00"]["count"])})
    controls = []
    for source in all_sources.values():
        if source["kind"] != "repacked-control":
            if source["original_sha256"] is not None:
                raise ValueError("original has repack binding")
            continue
        original = all_sources.get(source["original_sha256"])
        if not original or original["kind"] != "original" or original["camera"] != source["camera"]:
            raise ValueError("unbound repack control")
        a, b = original["engines"][0]["overview"], source["engines"][0]["overview"]
        controls.append({"id": source["id"], "original_id": original["id"], "camera": source["camera"],
                         "anchor_overview_bit_exact": a["size"] == b["size"] and digest(root / a["linear"]) == digest(root / b["linear"])})
    return {"schema": 1, "protocol": PROTOCOL, "campaign_manifest_sha256": digest(root / "manifest.json"),
            "analyzer_sha256": digest(Path(__file__)), "color_accuracy_qualified": False,
            "sources": len(files), "accepted_renders": accepted, "expected_refusals": refusals,
            "pair_count": sum(len(f["pairs"]) for f in files), "aggregates": aggregates,
            "repacked_controls": controls, "files": files,
            "limits": "Differences from Experimental, not errors against scene truth. One common exposure scale; no WB/profile fit. Output-domain masks do not prove unsaturated sensor sites. Files and blocks are correlated. Quantiles describe only this corpus; no population confidence claim."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("campaign", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = analyze(args.campaign)
    with args.output.open("x") as stream:
        stream.write(json.dumps(result, indent=2, allow_nan=False) + "\n")
    print(json.dumps({k: result[k] for k in ("sources", "accepted_renders", "pair_count", "repacked_controls")}, indent=2))


if __name__ == "__main__":
    main()
