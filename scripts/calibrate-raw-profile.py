#!/usr/bin/env python3
"""Offline measured-profile candidate; standard library only, no app installation."""
import argparse
import functools
import hashlib
import json
import math
from pathlib import Path

D50 = (0.96422, 1.0, 0.82521)
CFA_DOMAIN = "black/white calibrated CFA, before WB; sensor coordinates before crop/orientation"


def identifier(value):
    if not isinstance(value, str) or not value.strip() or value != value.strip():
        raise ValueError("nonempty identifier without surrounding whitespace required")
    return value


def positive(value, maximum=1e6):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or not 0 < value <= maximum:
        raise ValueError("finite positive scale required")
    return float(value)


def integers(value, length, minimum=0):
    if not isinstance(value, list) or len(value) != length or any(type(v) is not int or v < minimum for v in value):
        raise ValueError("invalid measurement counts or geometry")
    return value


def sha256(value):
    if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
        raise ValueError("lowercase SHA-256 required")
    return value


@functools.cache
def xtrans_patterns():
    # Same 6x6 sensor layout documented in tr-raw/cfa.rs; no rendered RGB inference.
    base = [1, 1, 0, 1, 1, 2, 1, 1, 2, 1, 1, 0, 2, 0, 1, 0, 2, 1,
            1, 1, 2, 1, 1, 0, 1, 1, 0, 1, 1, 2, 0, 2, 1, 2, 0, 1]
    patterns = set()
    for transform in range(8):
        for shift in range(36):
            values = []
            for i in range(36):
                x, y = i % 6, i // 6
                if transform & 1:
                    x, y = y, x
                x = 5 - x if transform & 2 else x
                y = 5 - y if transform & 4 else y
                values.append(base[((y + shift // 6) % 6) * 6 + (x + shift % 6) % 6])
            patterns.add(tuple(values))
    return frozenset(patterns)


def cfa_layout(report):
    period = report.get("cfa_period")
    if type(period) is not int or period not in (2, 6):
        raise ValueError("unsupported CFA period")
    pattern = integers(report.get("cfa_pattern"), period * period)
    expected = [1, 2, 1] if period == 2 else [8, 20, 8]
    if any(v > 2 for v in pattern) or [pattern.count(c) for c in range(3)] != expected:
        raise ValueError("invalid RGB CFA pattern")
    if period == 2 and (pattern.index(0) ^ pattern.index(2)) != 3:
        raise ValueError("invalid Bayer diagonal")
    if period == 6 and tuple(pattern) not in xtrans_patterns():
        raise ValueError("invalid X-Trans layout")
    return period, pattern


def phase_evidence(patch, period, pattern):
    """Recompose pooled moments independently; keep Gr/Gb and X-Trans sites distinct."""
    x, y, w, h = patch["rect_sensor"]
    rows = patch.get("phase_statistics")
    if not isinstance(rows, list) or len(rows) != period * period:
        raise ValueError("complete CFA phase statistics required")
    by_phase = {}
    close = lambda a, b: abs(a - b) <= max(1e-12, 1e-10 * abs(b))
    for row in rows:
        if any(k not in row for k in ("mean", "minimum", "maximum", "sample_variance")):
            raise ValueError("missing CFA phase moments")
        px, py = integers(row.get("phase_xy"), 2)
        if px >= period or py >= period or (px, py) in by_phase:
            raise ValueError("invalid or duplicate CFA phase")
        color = row.get("color")
        if type(color) is not int or color != pattern[py * period + px]:
            raise ValueError("CFA phase color mismatch")
        count, negative, saturated = integers([row.get(k) for k in
                                              ("sample_count", "negative_sites", "saturated_sites")], 3)
        nx = max(0, (w - 1 - (px - x) % period) // period + 1)
        ny = max(0, (h - 1 - (py - y) % period) // period + 1)
        if count != nx * ny or max(negative, saturated) > count:
            raise ValueError("CFA phase geometry/count mismatch")
        if not count:
            if any(row.get(k) is not None for k in ("mean", "minimum", "maximum", "sample_variance")):
                raise ValueError("empty CFA phase has invented moments")
        else:
            low, mean, high = vector([row.get(k) for k in ("minimum", "mean", "maximum")])
            if not low <= mean <= high:
                raise ValueError("inconsistent CFA phase range")
            if (negative == 0) != (low >= 0) or (negative == count) != (high < 0):
                raise ValueError("inconsistent negative CFA phase sites")
            if (saturated == 0) != (high < .985) or (saturated == count) != (low >= .985):
                raise ValueError("inconsistent saturated CFA phase sites")
            if count == 1:
                if row.get("sample_variance") is not None or low != mean or mean != high:
                    raise ValueError("single CFA site has invented variance or range")
            elif number(row.get("sample_variance")) < 0:
                raise ValueError("negative CFA phase variance")
        by_phase[px, py] = row
    for color in range(3):
        channel = [r for r in rows if r["color"] == color and r["sample_count"]]
        count = sum(r["sample_count"] for r in channel)
        if count != patch["sample_count"][color]:
            raise ValueError("pooled CFA count mismatch")
        mean = math.fsum(r["mean"] * r["sample_count"] for r in channel) / count
        variance = math.fsum((r["sample_variance"] or 0.) * (r["sample_count"] - 1)
                            + r["sample_count"] * (r["mean"] - mean) ** 2 for r in channel) / (count - 1)
        if not close(mean, patch["camera_rgb"][color]) or not close(variance, patch["sample_variance"][color]):
            raise ValueError("pooled CFA moments do not match phases")
        for key, combine in (("minimum", min), ("maximum", max),
                             ("negative_sites", sum), ("saturated_sites", sum)):
            if combine(r[key] for r in channel) != patch[key][color]:
                raise ValueError("pooled CFA extrema or counters do not match phases")
    comparison = patch.get("bayer_green_comparison")
    if period == 2:
        green = {pattern[py * 2 + (px ^ 1)]: (px, py)
                 for (px, py), row in by_phase.items() if row["color"] == 1}
        gr, gb = (by_phase[green[c]]["mean"] for c in (0, 2))
        difference = (gr - gb) / ((gr + gb) * .5) if gr is not None and gb is not None and min(gr, gb) > 0 else None
        if not isinstance(comparison, dict) or any(comparison.get(key) != list(green[c]) for key, c in
                                                   (("green_red_row", 0), ("green_blue_row", 2))):
            raise ValueError("Bayer green phase labels mismatch")
        actual = comparison.get("relative_mean_difference")
        mismatch = actual is not None if difference is None else actual is None or not close(number(actual), difference)
        if mismatch:
            raise ValueError("Bayer green difference mismatch")
    elif comparison is not None:
        raise ValueError("X-Trans cannot be reduced to two Bayer greens")
    return {"phase_statistics": rows, "bayer_green_comparison": comparison}


def measured_config(config, root):
    """Bind target references to hash-pinned CFA statistics, with one scale per capture."""
    if config.get("schema") != 2 or root is None:
        raise ValueError("measured calibration requires schema 2 and a statistics directory")
    if "camera_white" in config or "white_capture" in config:
        raise ValueError("measured white must refer to a CFA patch")
    captures = config.get("captures")
    if not isinstance(captures, list) or not 2 <= len(captures) <= 256:
        raise ValueError("separate fit and validation captures required")
    by_id, sources, evidence = {}, set(), []
    recipe = None
    for capture in captures:
        capture_id = identifier(capture.get("id"))
        split = capture.get("split")
        if capture_id in by_id or split not in ("fit", "validation"):
            raise ValueError("duplicate capture or invalid split")
        scale = positive(capture.get("exposure_scale"))
        path = Path(root) / identifier(capture.get("statistics_file"))
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != sha256(capture.get("statistics_sha256")):
            raise ValueError("CFA statistics hash mismatch")
        report = json.loads(data)
        source = sha256(report.get("source_sha256"))
        if source in sources:
            raise ValueError("RAW acquisition reused under multiple capture identifiers")
        sources.add(source)
        if report.get("schema") != 2 or report.get("domain") != CFA_DOMAIN or report.get("camera") != config.get("camera"):
            raise ValueError("CFA statistics camera or domain mismatch")
        period, pattern = cfa_layout(report)
        current_recipe = identifier(report.get("recipe"))
        if recipe is not None and recipe != current_recipe:
            raise ValueError("mixed CFA calibration recipes")
        recipe = current_recipe
        width, height = integers(report.get("sensor_size"), 2, 1)
        patches = {}
        for patch in report["patches"]:
            patch_id = identifier(patch.get("id"))
            if patch_id in patches:
                raise ValueError("duplicate CFA patch")
            patches[patch_id] = patch
        by_id[capture_id] = (split, scale, width, height, period, pattern, patches)
        evidence.append({"id": capture_id, "split": split, "exposure_scale": scale,
                         "source_sha256": source, "statistics_sha256": digest,
                         "recipe": recipe, "sensor_size": [width, height],
                         "cfa_period": period, "cfa_pattern": pattern})

    def measurement(capture_id, patch_id):
        if capture_id not in by_id:
            raise ValueError("unknown capture")
        split, scale, width, height, period, pattern, patches = by_id[capture_id]
        if patch_id not in patches:
            raise ValueError("unknown CFA patch")
        patch = patches[patch_id]
        x, y, w, h = integers(patch.get("rect_sensor"), 4)
        counts = integers(patch.get("sample_count"), 3, 3)
        saturated = integers(patch.get("saturated_sites"), 3)
        negative = integers(patch.get("negative_sites"), 3)
        if not w or not h or x + w > width or y + h > height or sum(counts) != w * h:
            raise ValueError("CFA patch geometry/count mismatch")
        mean, low, high, variance = (vector(patch[k]) for k in ("camera_rgb", "minimum", "maximum", "sample_variance"))
        if any(n > count for n, count in zip(negative, counts)) or min(variance) < 0 or any(not a <= b <= d for a, b, d in zip(low, mean, high)):
            raise ValueError("inconsistent CFA statistics")
        if any(saturated) or max(high) >= .985 or patch.get("fit_eligible_no_saturation") is not True:
            raise ValueError("saturated CFA sites cannot enter the fit or validation")
        phases = phase_evidence(patch, period, pattern)
        trace = {"capture": capture_id, "id": patch_id, "rect_sensor": [x, y, w, h],
                 "unscaled_camera_rgb": mean, "maximum": high, "sample_count": counts,
                 "sample_variance": variance, "negative_sites": negative, "saturated_sites": saturated,
                 **phases}
        return split, [v * scale for v in mean], trace

    patches, traces = [], []
    for reference in config["patches"]:
        if any(k in reference for k in ("camera_rgb", "split", "exposure_scale")):
            raise ValueError("patch values, split and exposure scale must come from their capture")
        capture_id, patch_id = (identifier(reference.get(k)) for k in ("capture", "id"))
        split, camera, trace = measurement(capture_id, patch_id)
        patches.append({**reference, "split": split, "camera_rgb": camera})
        traces.append(trace)
    white = config["white"]
    white_capture = identifier(white.get("capture"))
    split, white_camera, white_trace = measurement(white_capture, identifier(white.get("patch")))
    if split != "fit":
        raise ValueError("white acquisition must belong to fit")
    reflectance = positive(white.get("reference_y"), 1.)
    white_camera = [v / reflectance for v in white_camera]
    prepared = {**config, "schema": 1, "patches": patches,
                "camera_white": white_camera, "white_capture": white_capture}
    return prepared, {"captures": evidence, "patches": traces,
                      "white": {**white_trace, "reference_y": reflectance}}


def number(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or abs(value) > 1e6:
        raise ValueError("nonfinite or out-of-domain component")
    return float(value)


def vector(value):
    if not isinstance(value, (list, tuple)) or len(value) != 3:
        raise ValueError("expected three finite numbers")
    return tuple(number(x) for x in value)


def dot(a, b):
    return math.fsum(x * y for x, y in zip(a, b))


def transform(matrix, rgb):
    return tuple(dot(row, rgb) for row in matrix)


def lab(xyz):
    delta = 6 / 29
    def f(t):
        return t ** (1 / 3) if t > delta ** 3 else t / (3 * delta ** 2) + 4 / 29
    x, y, z = (f(v / w) for v, w in zip(xyz, D50))
    return (116 * y - 16, 500 * (x - y), 200 * (y - z))


def delta_e(a, b):
    """CIEDE2000 equations 2–22, Sharma/Wu/Dalal 2005; kL=kC=kH=1."""
    l1, a1, b1 = vector(a)
    l2, a2, b2 = vector(b)
    mean_c = (math.hypot(a1, b1) + math.hypot(a2, b2)) / 2
    g = (1 - math.sqrt(mean_c ** 7 / (mean_c ** 7 + 25 ** 7))) / 2
    ap1, ap2 = (1 + g) * a1, (1 + g) * a2
    c1, c2 = math.hypot(ap1, b1), math.hypot(ap2, b2)
    h1 = math.degrees(math.atan2(b1, ap1)) % 360 if c1 else 0
    h2 = math.degrees(math.atan2(b2, ap2)) % 360 if c2 else 0
    dh = h2 - h1
    if c1 * c2 == 0:
        dh = 0
    elif dh > 180:
        dh -= 360
    elif dh < -180:
        dh += 360
    dl, dc = l2 - l1, c2 - c1
    dh_term = 2 * math.sqrt(c1 * c2) * math.sin(math.radians(dh / 2))
    ml, mc = (l1 + l2) / 2, (c1 + c2) / 2
    mh = (h1 + h2) / 2
    if c1 * c2 == 0:
        mh = h1 + h2
    elif abs(h1 - h2) > 180:
        mh += 180 if h1 + h2 < 360 else -180
    cos = lambda angle: math.cos(math.radians(angle))
    t = 1 - .17 * cos(mh - 30) + .24 * cos(2 * mh) + .32 * cos(3 * mh + 6) - .20 * cos(4 * mh - 63)
    sl = 1 + .015 * (ml - 50) ** 2 / math.sqrt(20 + (ml - 50) ** 2)
    sc, sh = 1 + .045 * mc, 1 + .015 * mc * t
    rt = -2 * math.sqrt(mc ** 7 / (mc ** 7 + 25 ** 7)) * math.sin(math.radians(60 * math.exp(-((mh - 275) / 25) ** 2)))
    x, y, z = dl / sl, dc / sc, dh_term / sh
    return math.sqrt(max(0, x * x + y * y + z * z + rt * y * z))


def fit(patches, white):
    pivot = max(range(3), key=lambda c: white[c])
    free = [c for c in range(3) if c != pivot]
    scales = [math.sqrt(p["weight"]) for p in patches]
    columns = [[s * (p["camera_rgb"][c] - p["camera_rgb"][pivot] * white[c] / white[pivot])
                for p, s in zip(patches, scales)] for c in free]
    r00 = math.sqrt(dot(columns[0], columns[0]))
    if r00 <= 1e-15:
        raise ValueError("rank-deficient fit")
    q0 = [v / r00 for v in columns[0]]
    r01 = dot(q0, columns[1])
    residual = [v - r01 * q for v, q in zip(columns[1], q0)]
    correction = dot(q0, residual)
    r01 += correction
    residual = [v - correction * q for v, q in zip(residual, q0)]
    r11 = math.sqrt(dot(residual, residual))
    if r11 <= 1e-15:
        raise ValueError("rank-deficient fit")
    # Infinity norm condition number of the triangular factor.
    condition = max(abs(r00) + abs(r01), r11) * max(1 / r00 + abs(r01 / (r00 * r11)), 1 / r11)
    if not math.isfinite(condition) or condition > 10000:
        raise ValueError("ill-conditioned fit")
    q1 = [v / r11 for v in residual]
    matrix = []
    for c in range(3):
        rhs = [s * (p["xyz"][c] - p["camera_rgb"][pivot] * D50[c] / white[pivot])
               for p, s in zip(patches, scales)]
        second = dot(q1, rhs) / r11
        first = (dot(q0, rhs) - r01 * second) / r00
        row = [0., 0., 0.]
        row[free[0]], row[free[1]] = first, second
        row[pivot] = (D50[c] - first * white[free[0]] - second * white[free[1]]) / white[pivot]
        matrix.append(row)
    return matrix, condition


def summarize(matrix, patches):
    rows = []
    for p in patches:
        actual = transform(matrix, p["camera_rgb"])
        rows.append({"id": p["id"], "capture": p["capture"], "neutral": p["neutral"],
                     "predicted_xyz": actual, "xyz_error": [x - y for x, y in zip(actual, p["xyz"])],
                     "delta_e00": delta_e(lab(actual), lab(p["xyz"]))})
    values = sorted(p["delta_e00"] for p in rows)
    neutrals = [p["delta_e00"] for p in rows if p["neutral"]]
    metrics = {"mean": math.fsum(values) / len(values), "p95": values[math.ceil(.95 * len(values)) - 1],
               "maximum": max(values), "neutral_maximum": max(neutrals) if neutrals else None}
    passed = all(metrics[k] is not None and metrics[k] <= v
                 for k, v in (("mean", 1), ("p95", 2), ("maximum", 4), ("neutral_maximum", 1)))
    return {"count": len(rows), "metrics": metrics, "numerical_thresholds_passed": passed, "patches": rows}


def calibrate(config, root=None):
    evidence = None
    if config.get("kind") == "measured":
        config, evidence = measured_config(config, root)
    elif config.get("schema") != 1 or config.get("kind") != "synthetic":
        raise ValueError("unsupported schema or data kind")
    fields = ("camera", "mode", "illuminant", "reference_provenance", "uncertainty", "white_capture")
    if any(not isinstance(config.get(k), str) or not config[k].strip() for k in fields):
        raise ValueError("missing acquisition provenance")
    if config.get("observer") != "CIE 1931 2 degree" or config.get("reference_space") != "XYZ D50 Y=1":
        raise ValueError("explicit observer and XYZ D50 scale required")
    white = vector(config["camera_white"])
    if min(white) <= 0 or max(white) >= .985:
        raise ValueError("white must be positive and unsaturated before WB")
    groups = {"fit": [], "validation": []}
    keys = set()
    for original in config["patches"]:
        p = dict(original)
        for key in ("id", "capture"):
            identifier(p.get(key))
        key = (p["capture"], p["id"])
        if key in keys or p.get("split") not in groups or not isinstance(p.get("neutral"), bool):
            raise ValueError("duplicate patch or invalid split/neutral flag")
        keys.add(key)
        p["camera_rgb"], p["xyz"] = vector(p["camera_rgb"]), vector(p["xyz"])
        if max(p["camera_rgb"]) >= .985 or min(p["xyz"]) < 0:
            raise ValueError("saturated camera sample or invalid reference XYZ")
        weight = p.get("weight", 1.)
        if isinstance(weight, bool) or not isinstance(weight, (float, int)) or not math.isfinite(weight) or not 0 < weight <= 1e6:
            raise ValueError("invalid patch weight")
        p["weight"] = weight
        groups[p["split"]].append(p)
    fit_captures = {p["capture"] for p in groups["fit"]}
    validation_captures = {p["capture"] for p in groups["validation"]}
    if fit_captures & validation_captures or config["white_capture"] not in fit_captures:
        raise ValueError("fit, white and validation acquisitions must be separated")
    if any(len(p) < 6 for p in groups.values()) or not any(p["neutral"] for p in groups["validation"]):
        raise ValueError("six patches per split and a validation neutral required")
    matrix, condition = fit(groups["fit"], white)
    result = {k: config[k] for k in fields + ("kind", "observer", "reference_space")}
    result.update({"schema": 1, "matrix_camera_to_xyz_d50": matrix, "camera_white": white,
                   "white_residual": [x - y for x, y in zip(transform(matrix, white), D50)],
                   "qr_condition_infinity": condition, "qualified": False,
                   "qualification_note": "Offline candidate; measured repeatability, uncertainty and domain review required."})
    result.update({name: summarize(matrix, patches) for name, patches in groups.items()})
    if evidence is not None:
        result.update(schema=2, measurement_evidence=evidence)
    return result


def reference_tests(path):
    errors = []
    for line in Path(path).read_text().splitlines():
        if not line.strip():
            continue
        values = [float(v) for v in line.split()]
        if len(values) != 7:
            raise ValueError("expected seven columns in reference data")
        a, b, expected = values[:3], values[3:6], values[6]
        if not math.isfinite(expected) or expected < 0:
            raise ValueError("expected finite nonnegative reference Delta E")
        errors.extend((abs(delta_e(a, b) - expected), abs(delta_e(b, a) - expected)))
    if not errors or max(errors) > .00005:
        raise ValueError(f"CIEDE2000 reference failed: {max(errors, default=None)}")
    return {"pairs": len(errors) // 2, "maximum_absolute_error": max(errors), "passed": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, nargs="?")
    parser.add_argument("output", type=Path, nargs="?")
    parser.add_argument("--reference-tests", type=Path)
    args = parser.parse_args()
    if args.reference_tests:
        print(json.dumps(reference_tests(args.reference_tests), indent=2))
        return
    if args.input is None or args.output is None:
        parser.error("input and new output JSON required")
    source = args.input.read_bytes()
    report = calibrate(json.loads(source), args.input.resolve().parent)
    report["input_sha256"] = hashlib.sha256(source).hexdigest()
    serialized = json.dumps(report, indent=2, allow_nan=False) + "\n"
    with args.output.open("x") as stream:
        stream.write(serialized)


if __name__ == "__main__":
    main()
