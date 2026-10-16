#!/usr/bin/env python3
"""Seeded analytical clipping-boundary signals, not physical camera calibration.

All CFA phases, coloured ramps/shoulders, noise, texture and rejected boundaries.
Freeze code and seed before running a new verification. Once observed, a run is
regression data. Regions and phases are correlated, not independent photographs.
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
XTRANS = importlib.import_module("generate-highlight-validation").XTRANS


def generate(out, seed):
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    models = [
        ("d750", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D750_MATRIX"), [0, 1, 1, 2]),
        ("d800", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D800_MATRIX"), [0, 1, 1, 2]),
        ("xt5", duel.matrix("third_party/fuji-codec/xt5.rs", "MATRIX"), XTRANS),
    ]
    sources, descriptions = [], []
    kinds = ["ramp", "shoulder", "curved-colour", "step", "noise", "texture",
             "ambiguous-neutral", "ambiguous-colour"]
    for model_index, (name, cm, base) in enumerate(models):
        period = math.isqrt(len(base))
        for phase in range(period * period):
            rng = random.Random(seed + 100003 * model_index + 997 * phase)
            dx, dy = phase % period, phase // period
            pattern = [base[((i // period + dy) % period) * period + (i % period + dx) % period]
                       for i in range(period * period)]
            w, h = rng.randrange(103, 121), rng.randrange(73, 89)
            white = [rng.uniform(.87, 1.08), 1., rng.uniform(.82, 1.19)]
            neutral = duel.color.transform(cm, white)
            forward = duel.wb.multiply(cm, duel.wb.multiply(duel.adapt(white), duel.RGB_TO_XYZ))
            forward = [[v / neutral[1] for v in row] for row in forward]
            hue = [rng.uniform(.32, .88) for _ in range(3)]
            hue[phase % 3] = 1.
            peak = max(duel.color.transform(forward, hue))
            axis = phase % 2
            n = [w, h][axis]
            crossing = rng.randrange(n // 2, 2 * n // 3)
            epsilon = rng.uniform(-1.3e-4, 1.3e-4)
            slope = rng.uniform(.007, .011)
            dark = [rng.uniform(.05, .14) for _ in range(3)]
            for kind in kinds:
                truth = []
                for y in range(h):
                    for x in range(w):
                        p, q = (x, y) if axis == 0 else (y, x)
                        light = 1. + epsilon + slope * (p - crossing)
                        if kind == "shoulder":
                            light = min(light, 1. + epsilon)
                        elif kind == "step" and p > n * .71:
                            light += .17
                        rgb = [v * light / peak for v in hue]
                        if kind == "curved-colour" and p < .39 * q + 23 + 3.1 * math.sin(.13 * q):
                            rgb = dark
                        elif kind == "texture":
                            rgb = [v * (1. + .095 * math.sin(.61 * x + .37 * y + .71 * c))
                                   for c, v in enumerate(rgb)]
                        elif kind == "ambiguous-neutral":
                            rgb = [5., 5., 5.]
                        elif kind == "ambiguous-colour":
                            rgb = [5., 4., 6.]
                        truth.append(rgb)
                camera = [duel.color.transform(forward, rgb) for rgb in truth]
                cfa = [camera[y * w + x][pattern[(y % period) * period + x % period]]
                       for y in range(h) for x in range(w)]
                observed = [v + (rng.gauss(0., .0011 * math.sqrt(max(0., v)) + .0006)
                                 if kind == "noise" else 0.) for v in cfa]
                codes = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE - duel.BLACK) * v)))
                         for v in observed]
                ident = f"s{seed}-{name}-p{phase}-{kind}"
                raw = duel.dng(w, h, pattern, cm, neutral, codes)
                rawpath, refpath = out / (ident + ".dng"), out / (ident + ".f32")
                rawpath.write_bytes(raw)
                reference = b"".join(struct.pack("<fff", *v) for v in truth)
                refpath.write_bytes(reference)
                rawpath.with_suffix(".cfa").write_bytes(b"".join(struct.pack("<f", v) for v in cfa))
                sources.append(dict(id=ident, source=str(rawpath), sha256=hashlib.sha256(raw).hexdigest(),
                                    truth=str(refpath), truth_sha256=hashlib.sha256(reference).hexdigest(),
                                    camera="Analytical " + name,
                                    patches=[[0, 0, w-1, h-1], [7, 7, w-14, h-14]], repetitions=1))
                descriptions.append(dict(id=ident, kind=kind, cfa_period=period, phase=phase,
                                         size=[w, h], axis=axis, crossing=crossing,
                                         threshold_offset=epsilon, white=white, hue=hue,
                                         intentional_identical_raw_pair=kind.startswith("ambiguous-"),
                                         saturated_sites=sum(v == duel.WHITE for v in codes)))
    for name, items in [("manifest", sources), ("description", descriptions),
                        ("bayer", [s for s in sources if "-xt5-" not in s["id"]])]:
        (out / (name + ".json")).write_text(json.dumps(dict(schema=1, seed=seed, sources=items), indent=2) + "\n")
    print(f"Generated {len(sources)} analytical signals with seed {seed}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seed", type=int, required=True)
    args = parser.parse_args()
    generate(args.output, args.seed)
