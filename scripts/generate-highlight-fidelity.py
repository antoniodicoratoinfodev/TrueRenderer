#!/usr/bin/env python3
"""New analytical validation for edge interpolation and highlight continuation.

No camera calibration. Both CFA families, every phase, independent colours,
noise/texture, curved boundaries and identifiable/ambiguous full clipping.
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


def generate(out, round_number):
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    cm = duel.XYZ_TO_RGB
    neutral = duel.color.transform(cm, duel.D65)
    forward = duel.wb.multiply(cm, duel.RGB_TO_XYZ)
    forward = [[v / neutral[1] for v in row] for row in forward]
    w, h = (145 + 6 * round_number, 95 + 6 * round_number)
    kinds = ["steps", "curved", "texture", "noise", "clipped-tail",
             "ambiguous-neutral", "ambiguous-colour"]
    sources, descriptions = [], []
    for period, base in [(2, [0, 1, 1, 2]), (6, XTRANS)]:
        for phase in range(period * period):
            dx, dy = phase % period, phase // period
            pattern = [base[((i // period + dy) % period) * period
                            + (i % period + dx) % period] for i in range(period * period)]
            rng = random.Random(823457 + 31013 * round_number + 997 * period + 131 * phase)
            hue = [rng.uniform(.24, .87) for _ in range(3)]
            hue[phase % 3] = 1.
            dark = [rng.uniform(.045, .12) for _ in range(3)]
            for kind in kinds:
                truth = []
                for y in range(h):
                    for x in range(w):
                        p, n = (x, w) if phase % 2 == 0 else (y, h)
                        light = .31 + 1.18 * p / (n - 1) + .017 * (x + y) / (w + h - 2)
                        if kind == "steps":
                            a, b = n // 3 + phase % 9 - 4, 2 * n // 3 + phase % 11 - 5
                            light = .51 if p < a else 1.07 if p < b else 1.67
                        rgb = [v * light for v in hue]
                        if kind == "curved":
                            edge = 21 + .43 * y + 4.9 * math.sin(.091 * y + .31)
                            if x < edge:
                                rgb = [v + .00012 * x + .00009 * y for v in dark]
                        elif kind == "texture":
                            rgb = [v * light * (1 + .13 * math.sin(.77*x+.43*y+c*.59))
                                   for c, v in enumerate(hue)]
                        elif kind == "clipped-tail":
                            tail_hue = [.80, .91, 1.]
                            tail_hue = tail_hue[phase % 3:] + tail_hue[:phase % 3]
                            rgb = [v * (.3 + .972 * p / (n-1)) for v in tail_hue]
                        elif kind == "ambiguous-neutral":
                            rgb = [2.6, 2.6, 2.6]
                        elif kind == "ambiguous-colour":
                            rgb = [2.6, 1.9, 1.4]
                        truth.append(rgb)
                camera = [duel.color.transform(forward, rgb) for rgb in truth]
                cfa = [camera[y*w+x][pattern[(y % period)*period + x % period]]
                       for y in range(h) for x in range(w)]
                observed = [v + (rng.gauss(0, .0017*math.sqrt(max(0, v)) + .0007)
                                 if kind == "noise" else 0.) for v in cfa]
                codes = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE-duel.BLACK)*v)))
                         for v in observed]
                name = f"v{round_number}-cfa{period}-p{phase}-{kind}"
                raw = duel.dng(w, h, pattern, cm, neutral, codes)
                rawpath, refpath = out / (name + ".dng"), out / (name + ".f32")
                rawpath.write_bytes(raw)
                ref = b"".join(struct.pack("<fff", *p) for p in truth)
                refpath.write_bytes(ref)
                (out / (name + ".cfa")).write_bytes(b"".join(struct.pack("<f", v) for v in cfa))
                patches = [[0, 0, w-1, h-1], [7, 7, w-14, h-14]]
                sources.append(dict(id=name, source=str(rawpath), sha256=hashlib.sha256(raw).hexdigest(),
                                    camera="Analytical RGB model", truth=str(refpath),
                                    truth_sha256=hashlib.sha256(ref).hexdigest(), patches=patches, repetitions=1))
                descriptions.append(dict(id=name, kind=kind, cfa_period=period, phase=phase,
                                         intentional_identical_raw_pair=kind.startswith("ambiguous-"),
                                         saturated_sites=sum(v == duel.WHITE for v in codes)))
    for name, items in [("manifest", sources), ("bayer", [s for s in sources if "-cfa2-" in s["id"]]),
                        ("description", descriptions)]:
        (out / (name + ".json")).write_text(json.dumps(dict(schema=1, sources=items), indent=2) + "\n")
    print(f"Generated {len(sources)} analytical cases; ambiguous pairs intentionally share RAW codes")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--validation-round", type=int, required=True, choices=[1, 2, 3])
    args = parser.parse_args()
    generate(args.output, args.validation_round)
