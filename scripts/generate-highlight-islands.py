#!/usr/bin/env python3
"""Seeded, coloured highlight islands with known scene truth, not camera truth.

Quadratics exercise the proposed model. Gaussian/quartic peaks, texture, steps,
noise and hidden detail test model mismatch; they are not training targets.
All phases are correlated. Freeze the implementation before a fresh seed.
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
    sources, descriptions = [], []
    models = [
        ("d750", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D750_MATRIX"), [0, 1, 1, 2]),
        ("d800", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D800_MATRIX"), [0, 1, 1, 2]),
        ("xt5", duel.matrix("third_party/fuji-codec/xt5.rs", "MATRIX"), XTRANS),
    ]
    kinds = ["quadratic", "partial", "gaussian", "quartic", "texture", "noise",
             "edge", "border", "hidden-colour", "hidden-detail", "hidden-flat",
             "gaussian-background", "smooth-texture", "unclipped"]
    for mi, (name, cm, base) in enumerate(models):
        period = math.isqrt(len(base))
        for phase in range(period * period):
            rng = random.Random(seed + mi * 100003 + phase * 997)
            dx, dy = phase % period, phase // period
            pattern = [base[((i // period + dy) % period) * period + (i % period + dx) % period]
                       for i in range(period * period)]
            w, h = rng.randrange(85, 98), rng.randrange(73, 86)
            white = [rng.uniform(.88, 1.12), 1., rng.uniform(.8, 1.2)]
            neutral = duel.color.transform(cm, white)
            forward = duel.wb.multiply(cm, duel.wb.multiply(duel.adapt(white), duel.RGB_TO_XYZ))
            forward = [[v / neutral[1] for v in row] for row in forward]
            inverse = duel.wb.inverse(forward)
            cx, cy = w * .5 + rng.uniform(-3, 3), h * .5 + rng.uniform(-3, 3)
            peaks = [rng.uniform(1.012, 1.07) for _ in range(3)]
            curves = [(rng.uniform(.0007, .0015), rng.uniform(.0007, .0015), rng.uniform(-.0004, .0004))
                      for _ in range(3)]
            shifts = [(rng.uniform(-.7, .7), rng.uniform(-.7, .7)) for _ in range(3)]
            for kind in kinds:
                truth, ideal, observed = [], [], []
                for y in range(h):
                    for x in range(w):
                        camera = []
                        for c in range(3):
                            xx = x - (2. if kind == "border" else cx) - shifts[c][0]
                            yy = y - cy - shifts[c][1]
                            a, b, cross = curves[c]
                            q = a * xx * xx + b * yy * yy + cross * xx * yy
                            peak = peaks[c] - (.22 if kind == "partial" and c == phase % 3 else 0.)
                            v = peak - q
                            if kind == "gaussian":
                                v = peak * math.exp(-q / peak)
                            elif kind == "gaussian-background":
                                v = .025 + (peak - .025) * math.exp(-q / (peak - .025))
                            elif kind == "quartic":
                                v += .12 * q * q
                            elif kind == "texture":
                                v += .025 * math.sin(.53 * x + .31 * y + .87 * c)
                            elif kind == "smooth-texture":
                                v += .004 * math.sin(.12 * x + .07 * y + .87 * c)
                            elif kind == "edge" and x < cx + .33 * (y - cy):
                                v *= .55 + .05 * c
                            elif kind == "unclipped":
                                v -= .1
                            # Identical observed CFA to the quadratic scene;
                            # the hidden truth differs only above its threshold.
                            if v > 1. and kind == "hidden-colour":
                                v += (v - 1.) * (.4 + .5 * c)
                            elif v > 1. and kind == "hidden-detail":
                                v += (v - 1.) * (.7 + .5 * math.sin(1.17 * x - .63 * y + c))
                            elif kind == "hidden-flat":
                                v = min(1., v)
                            camera.append(max(.02, v))
                        truth.append(duel.color.transform(inverse, camera))
                        c = pattern[y % period * period + x % period]
                        ideal.append(camera[c])
                        noise = rng.gauss(0., .0015) if kind == "noise" else 0.
                        observed.append(camera[c] + noise)
                codes = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE - duel.BLACK) * v))) for v in observed]
                ident = f"i{seed}-{name}-p{phase}-{kind}"
                raw = duel.dng(w, h, pattern, cm, neutral, codes)
                ref = b"".join(struct.pack("<fff", *v) for v in truth)
                path, refpath = out / (ident + ".dng"), out / (ident + ".f32")
                path.write_bytes(raw)
                refpath.write_bytes(ref)
                path.with_suffix(".cfa").write_bytes(b"".join(struct.pack("<f", v) for v in ideal))
                sources.append(dict(id=ident, source=str(path), sha256=hashlib.sha256(raw).hexdigest(),
                                    truth=str(refpath), truth_sha256=hashlib.sha256(ref).hexdigest(),
                                    camera="Analytical " + name, repetitions=1,
                                    patches=[[0, 0, w-1, h-1], [int(cx)-16, int(cy)-16, 32, 32]]))
                descriptions.append(dict(id=ident, kind=kind, size=[w, h], pattern=pattern,
                                         peaks=peaks, curves=curves, shifts=shifts, white=white,
                                         saturated_sites=codes.count(duel.WHITE),
                                         shared_RAW_with_quadratic=kind.startswith("hidden-")))
    for name, records in [("manifest", sources), ("description", descriptions)]:
        (out / (name + ".json")).write_text(json.dumps(dict(schema=1, seed=seed, sources=records), indent=2) + "\n")
    print(f"Generated {len(sources)} highlight islands, seed {seed}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seed", type=int, required=True)
    args = parser.parse_args()
    generate(args.output, args.seed)
