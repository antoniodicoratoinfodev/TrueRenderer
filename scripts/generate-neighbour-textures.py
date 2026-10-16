#!/usr/bin/env python3
"""Seeded colour textures, perturbations and explicitly non-identifiable pairs.

Analytical camera-space truth only. The censored-flat member shares the exact
RAW with phase-texture but a different clipped green truth: neither a model nor
agreement with another renderer can resolve that ambiguity from this input.
"""
import argparse
import hashlib
import importlib
import json
import math
from pathlib import Path
import random
import struct

duel = importlib.import_module("generate-renderer-duel")


def generate(out, seed):
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    models = [("linear", [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]),
              ("d750", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D750_MATRIX")),
              ("d800", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D800_MATRIX"))]
    kinds = ["phase-texture", "two-frequencies", "chirp", "envelope", "independent-green",
             "support-error", "noise", "multi-clipped", "censored-flat"]
    sources, descriptions = [], []
    for mi, (model, cm) in enumerate(models):
        for phase, pattern in enumerate(duel.PATTERNS):
            rng = random.Random(seed + mi * 100003 + phase * 997)
            w, h = rng.randrange(93, 114), rng.randrange(77, 94)
            white = [rng.uniform(.86, 1.14), 1., rng.uniform(.82, 1.18)]
            neutral = duel.color.transform(cm, white)
            forward = duel.wb.multiply(cm, duel.wb.multiply(duel.adapt(white), duel.RGB_TO_XYZ))
            inverse = duel.wb.inverse([[v / neutral[1] for v in row] for row in forward])
            fx, fy = rng.uniform(.14, .82), rng.uniform(.13, .69)
            angle = rng.uniform(-math.pi, math.pi)
            shift = rng.uniform(.22, 1.15)
            separation = rng.uniform(.025, .18)
            amplitude = rng.uniform(.025, .07)
            crossing = rng.uniform(.42, .66) * h
            slope = rng.uniform(.0035, .0058)
            point = [w // 3, h // 4]
            if pattern[point[1] % 2 * 2 + point[0] % 2] == 1:
                point[0] += 1
            for kind in kinds:
                truth, cfa, observed = [], [], []
                noise_rng = random.Random(seed + mi * 70001 + phase * 511)
                for y in range(h):
                    for x in range(w):
                        theta = fx * x + fy * y + angle
                        if kind == "chirp":
                            theta += .0014 * x * x
                        envelope = .7 + .3 * math.cos(.023 * x) if kind == "envelope" else 1.
                        light = 1. + slope * (y - crossing)
                        red = .49 * light * (1. + amplitude * envelope * math.sin(theta + shift))
                        blue = .46 * light * (1. + amplitude * envelope * math.sin(theta + shift + separation))
                        green = light * (1. + amplitude * envelope * math.sin(theta))
                        camera = [red, green, blue]
                        if kind == "two-frequencies":
                            camera = [v * (1. + .018 * math.sin(.39 * x - .28 * y + c * .81))
                                      for c, v in enumerate(camera)]
                        elif kind == "independent-green":
                            camera[1] += .021 * math.cos(.29 * x - .43 * y)
                        elif kind == "multi-clipped":
                            camera[0] *= 2.5
                        elif kind == "censored-flat":
                            camera[1] = min(camera[1], 1.)
                        truth.append(duel.color.transform(inverse, camera))
                        c = pattern[y % 2 * 2 + x % 2]
                        cfa.append(camera[c])
                        perturbation = 0.
                        if kind == "noise":
                            perturbation = noise_rng.gauss(0., .0007 * math.sqrt(camera[c]) + .0003)
                        elif kind == "support-error" and [x, y] == point:
                            perturbation = .018
                        observed.append(camera[c] + perturbation)
                codes = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE - duel.BLACK) * v)))
                         for v in observed]
                ident = f"n{seed}-{model}-p{phase}-{kind}"
                raw = duel.dng(w, h, pattern, cm, neutral, codes)
                reference = b"".join(struct.pack("<fff", *v) for v in truth)
                path, ref = out / (ident + ".dng"), out / (ident + ".f32")
                path.write_bytes(raw)
                ref.write_bytes(reference)
                path.with_suffix(".cfa").write_bytes(b"".join(struct.pack("<f", v) for v in cfa))
                sources.append(dict(id=ident, source=str(path), sha256=hashlib.sha256(raw).hexdigest(),
                                    truth=str(ref), truth_sha256=hashlib.sha256(reference).hexdigest(),
                                    camera="Analytical " + model, repetitions=1,
                                    patches=[[0, 0, w - 1, h - 1], [7, 7, w - 14, h - 14]]))
                descriptions.append(dict(id=ident, kind=kind, size=[w, h], pattern=pattern,
                                         frequency=[fx, fy], phase=angle, chromatic_shift=shift,
                                         support_phase_separation=separation,
                                         amplitude=amplitude, crossing=crossing, slope=slope,
                                         perturbed_site=point, saturated_sites=codes.count(duel.WHITE),
                                         nonidentifiable_pair=kind in ("phase-texture", "censored-flat")))
    for name, records in [("manifest", sources), ("description", descriptions)]:
        (out / (name + ".json")).write_text(json.dumps(dict(schema=1, seed=seed, sources=records), indent=2) + "\n")
    by_id = {s["id"]: s for s in sources}
    for s in sources:
        if s["id"].endswith("censored-flat"):
            paired = by_id[s["id"].replace("censored-flat", "phase-texture")]
            assert s["sha256"] == paired["sha256"]
            assert s["truth_sha256"] != paired["truth_sha256"]
    print(f"Generated {len(sources)} neighbour-texture signals, seed {seed}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seed", type=int, required=True)
    args = parser.parse_args()
    generate(args.output, args.seed)
