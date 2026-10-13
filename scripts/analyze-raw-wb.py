#!/usr/bin/env python3
"""Summarize the offline RAW WB precision campaign in XYZ D50/Lab/Delta E00."""
import argparse
import hashlib
import importlib
import json
import math
from pathlib import Path

color = importlib.import_module("calibrate-raw-profile")


def inverse(m):
    a, b, c = m[0]
    d, e, f = m[1]
    g, h, i = m[2]
    cofactors = [[e*i-f*h, c*h-b*i, b*f-c*e],
                 [f*g-d*i, a*i-c*g, c*d-a*f],
                 [d*h-e*g, b*g-a*h, a*e-b*d]]
    determinant = a*cofactors[0][0]+b*cofactors[1][0]+c*cofactors[2][0]
    if abs(determinant) < 1e-12:
        raise ValueError("singular matrix")
    return [[v/determinant for v in row] for row in cofactors]


def multiply(a, b):
    return [[math.fsum(a[r][k]*b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def conversion():
    # Invert the published production Rec.2020 projection, then Bradford D65->D50.
    xyz_to_2020 = [[1.716651187971268, -.355670783776392, -.253366281373660],
                   [-.666684351832489, 1.616481236634939, .015768545813911],
                   [.017639857445311, -.042770613257809, .942103121235474]]
    bradford = [[.8951, .2664, -.1614], [-.7502, 1.7135, .0367], [.0389, -.0685, 1.0296]]
    source = color.transform(bradford, [.9504559270516716, 1., 1.0890577507598784])
    target = color.transform(bradford, color.D50)
    diagonal = [[target[r]/source[r] if r == c else 0. for c in range(3)] for r in range(3)]
    return multiply(multiply(inverse(bradford), multiply(diagonal, bradford)), inverse(xyz_to_2020))


def summary(values):
    values = sorted(values)
    if not values:
        return {"count": 0, "mean": None, "p95": None, "maximum": None}
    return {"count": len(values), "mean": math.fsum(values)/len(values),
            "p95": values[math.ceil(.95*len(values))-1], "maximum": max(values)}


def analyze(data):
    matrix = conversion()
    white_lab = color.lab(color.transform(matrix, [1., 1., 1.]))
    if max(abs(x-y) for x, y in zip(white_lab, [100., 0., 0.])) > 1e-8:
        raise ValueError("analysis white mismatch")
    cameras = []
    if not data["cameras"]:
        raise ValueError("empty WB campaign")
    for camera in data["cameras"]:
        rejected = camera["rejected_wb_pairs"]
        precise_acceptance = (sum((x["continuous_error"] is None) != (x["precise_error"] is None)
                                  for x in rejected)
                              if data.get("schema") == 2 else None)
        errors, precise_errors, projection, linear, outside = [], [], [], [], 0
        for case in camera["cases"]:
            exact = color.vector(case["continuous_rec2020"])
            quantized = color.vector(case["quantized_rec2020"])
            reference = color.vector(case["f64_projection"])
            projection.extend(abs(x-y) for x, y in zip(exact, reference))
            linear.extend(abs(x-y) for x, y in zip(exact, quantized))
            a, b = color.transform(matrix, exact), color.transform(matrix, quantized)
            precise = color.transform(matrix, color.vector(case["precise_rec2020"])) if "precise_rec2020" in case else None
            compared = [a, b] + ([precise] if precise is not None else [])
            if not case["unsaturated_positive_camera"] or any(min(v) < 0 or v[1] > 1 for v in compared):
                outside += 1
                continue
            errors.append(color.delta_e(color.lab(a), color.lab(b)))
            if precise is not None:
                precise_errors.append(color.delta_e(color.lab(a), color.lab(precise)))
        de = summary(errors)
        fine = summary(precise_errors)
        cameras.append({"camera": camera["camera"], "source_sha256": camera["source_sha256"],
                        "profile_hash": camera["profile_hash"], "evaluated_constant_colors": len(camera["cases"]),
                        "wb_quantization_delta_e00": de, "outside_lab_comparison_domain": outside,
                        "precise_wb_delta_e00": fine,
                        "fp32_projection_absolute_error": summary(projection),
                        "wb_quantization_linear_absolute_error": summary(linear),
                        "diagnostic_delta_e00_budget": .1,
                        "diagnostic_budget_passed": de["maximum"] is not None and de["maximum"] <= .1,
                        "precise_diagnostic_budget_passed": fine["maximum"] is not None and fine["maximum"] <= .1,
                        "rejected_wb_pairs": camera["rejected_wb_pairs"],
                        "precise_wb_acceptance_changed": precise_acceptance,
                        "wb_acceptance_changed": sum((x["continuous_error"] is None) != (x["quantized_error"] is None)
                                                     for x in camera["rejected_wb_pairs"])})
    return {"recipe": data["recipe"], "cameras": cameras, "camera_color_accuracy_qualified": False,
            "limits": data["limits"], "lab_domain": "Positive XYZ, Y<=1 and unsaturated nonnegative camera samples only; extended values retained in linear metrics."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    source = args.input.read_bytes()
    result = analyze(json.loads(source))
    result["input_sha256"] = hashlib.sha256(source).hexdigest()
    with args.output.open("x") as stream:
        stream.write(json.dumps(result, indent=2, allow_nan=False)+"\n")


if __name__ == "__main__":
    main()
