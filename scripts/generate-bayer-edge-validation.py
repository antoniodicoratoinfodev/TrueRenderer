#!/usr/bin/env python3
"""Frozen-seed analytical Bayer validation; no physical camera truth.

Generate only after freezing the candidate. Once observed, a run is regression
data. Its seed, source hashes, full outcomes and ambiguous pairs must be retained.
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


def generate(output, seed):
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    rng = random.Random(seed)
    models = [
        ("linear", duel.XYZ_TO_RGB),
        ("d750", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D750_MATRIX")),
        ("d800", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D800_MATRIX")),
    ]
    kinds = ["constant-hue", "colour-change", "affine-colour", "noise", "texture", "curved"]
    sources, descriptions = [], []
    for model, cm in models:
        white = [rng.uniform(.94, .99), 1., rng.uniform(.79, 1.09)]
        neutral = duel.color.transform(cm, white)
        transform = duel.wb.multiply(cm, duel.wb.multiply(duel.adapt(white), duel.RGB_TO_XYZ))
        transform = [[v / neutral[1] for v in row] for row in transform]
        for phase, pattern in enumerate(duel.PATTERNS):
            w, h = rng.randrange(43, 56) * 2 + 1, rng.randrange(34, 44) * 2 + 1
            for axis in range(2):
                n = [w, h][axis]
                cuts = [rng.randrange(3, 7), n // 2 + rng.randrange(-4, 5), n - rng.randrange(4, 8)]
                for location, cut in enumerate(cuts):
                    hue = [rng.uniform(.18, .92) for _ in range(3)]
                    hue[rng.randrange(3)] = 1.
                    scale = rng.uniform(.96, 1.84) / max(duel.color.transform(transform, hue))
                    high = [v * scale for v in hue]
                    ratio = rng.uniform(.25, .76)
                    same_hue_low = [v * ratio for v in high]
                    different_low = [rng.uniform(.035, .32) for _ in range(3)]
                    slopes = [[rng.uniform(-.0007, .0007) for _ in range(3)] for _ in range(2)]
                    current_kinds = kinds + (["ambiguous-neutral", "ambiguous-colour"] if axis == location == 0 else [])
                    for kind in current_kinds:
                        truth = []
                        for y in range(h):
                            for x in range(w):
                                p, tangent = ([x, y][axis], [y, x][axis])
                                boundary = cut + (2.3 * math.sin(.19 * tangent + .7) if kind == "curved" else 0.)
                                side = int(p >= boundary)
                                low = same_hue_low if kind in ("constant-hue", "noise") else different_low
                                rgb = list(high if side else low)
                                if kind == "affine-colour":
                                    rgb = [v + slopes[side][c] * (x + .67 * y) for c, v in enumerate(rgb)]
                                if kind == "texture":
                                    rgb = [v * (1 + .06 * math.sin(.73*x+.37*y+c*.61)) for c, v in enumerate(rgb)]
                                if kind == "ambiguous-neutral":
                                    rgb = [8., 8., 8.]
                                if kind == "ambiguous-colour":
                                    rgb = [8., 7., 6.]
                                truth.append(rgb)
                        camera = [duel.color.transform(transform, p) for p in truth]
                        cfa = [camera[y*w+x][pattern[y%2*2+x%2]] for y in range(h) for x in range(w)]
                        observed = [v + (rng.gauss(0., .0013 * math.sqrt(max(0., v)) + .0006)
                                         if kind == "noise" else 0.) for v in cfa]
                        codes = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE-duel.BLACK)*v))) for v in observed]
                        name = f"s{seed}-{model}-p{phase}-a{axis}-k{location}-{kind}"
                        raw = duel.dng(w, h, pattern, cm, neutral, codes)
                        reference = b"".join(struct.pack("<fff", *p) for p in truth)
                        rawpath, refpath = output / (name + ".dng"), output / (name + ".f32")
                        rawpath.write_bytes(raw)
                        refpath.write_bytes(reference)
                        (output / (name + ".cfa")).write_bytes(b"".join(struct.pack("<f", v) for v in cfa))
                        sources.append(dict(id=name, source=str(rawpath), sha256=hashlib.sha256(raw).hexdigest(),
                                            camera="Analytical Bayer model: " + model, truth=str(refpath),
                                            truth_sha256=hashlib.sha256(reference).hexdigest(),
                                            patches=[[0, 0, w-1, h-1], [7, 7, w-14, h-14]], repetitions=1))
                        descriptions.append(dict(id=name, kind=kind, model=model, phase=phase, axis=axis,
                                                 cut=cut, size=[w, h], white=white,
                                                 saturated_sites=codes.count(duel.WHITE),
                                                 intentional_identical_raw_pair=kind.startswith("ambiguous-")))
    (output / "manifest.json").write_text(json.dumps(dict(schema=1, sources=sources), indent=2) + "\n")
    (output / "description.json").write_text(json.dumps(dict(seed=seed, scope=__doc__, sources=descriptions), indent=2) + "\n")
    print(f"Generated {len(sources)} cases; full-clipping pairs intentionally share RAW codes")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seed", required=True, type=int)
    args = parser.parse_args()
    generate(args.output, args.seed)
