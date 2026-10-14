#!/usr/bin/env python3
"""Reproduce the frozen HL3 review signals; these are not calibrated photographs.

All Bayer/X-Trans phases, odd dimensions, moving coloured steps, curved colour
boundaries and signal-dependent noise. The two regions per image overlap.
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


def generate(out):
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    cm = duel.XYZ_TO_RGB
    neutral = duel.color.transform(cm, duel.D65)
    forward = duel.wb.multiply(cm, duel.RGB_TO_XYZ)
    forward = [[v / neutral[1] for v in row] for row in forward]
    w, h = 137, 89
    sources, descriptions = [], []
    for period, base in [(2, [0, 1, 1, 2]), (6, XTRANS)]:
        for phase in range(period * period):
            dx, dy = phase % period, phase // period
            pattern = [base[((i // period + dy) % period) * period
                            + (i % period + dx) % period]
                       for i in range(period * period)]
            for kind in ["plane", "step", "curved-colour", "signal-noise"]:
                rng = random.Random(174619 + period * 997 + phase * 31)
                hue = ([.29, 1., .63] if phase % 3 == 0 else
                       [1., .37, .72] if phase % 3 == 1 else [.43, .71, 1.])
                truth = []
                for y in range(h):
                    for x in range(w):
                        light = .38 + 1.14 * x / (w - 1) + .023 * y / (h - 1)
                        if kind == "step":
                            p, n = (x, w) if phase % 2 == 0 else (y, h)
                            a, b = n // 3 + phase % 5, 2 * n // 3 + (phase % 7) - 3
                            light = .57 if p < a else 1.11 if p < b else 1.59
                        rgb = [v * light for v in hue]
                        if kind == "curved-colour" and x < .64 * y + 27 + 8 * math.sin(y * .073):
                            rgb = [.14, .19, .07]
                        truth.append(rgb)
                camera = [duel.color.transform(forward, p) for p in truth]
                cfa = [camera[y * w + x][pattern[(y % period) * period + x % period]]
                       for y in range(h) for x in range(w)]
                observed = [v + (rng.gauss(0, .0013 * math.sqrt(v) + .0004)
                                 if kind == "signal-noise" else 0.) for v in cfa]
                samples = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE - duel.BLACK) * v)))
                           for v in observed]
                name = f"cfa{period}-p{phase}-{kind}"
                raw = duel.dng(w, h, pattern, cm, neutral, samples)
                rawpath, refpath = out / (name + ".dng"), out / (name + ".f32")
                rawpath.write_bytes(raw)
                reference = b"".join(struct.pack("<fff", *v) for v in truth)
                refpath.write_bytes(reference)
                (out / (name + ".cfa")).write_bytes(b"".join(struct.pack("<f", v) for v in cfa))
                patches = [[0, 0, w - 1, h - 1], [3, 3, w - 6, h - 6]]
                sources.append(dict(id=name, source=str(rawpath), sha256=hashlib.sha256(raw).hexdigest(),
                                    camera="Analytical RGB model", truth=str(refpath),
                                    truth_sha256=hashlib.sha256(reference).hexdigest(),
                                    patches=patches, repetitions=1))
                descriptions.append(dict(
                    id=name, kind=kind, cfa_period=period, phase=phase,
                    patch_sensor_range=[[
                        min(camera[y * w + x][c] for y in range(y0, y0 + ph)
                            for x in range(x0, x0 + pw) for c in range(3)),
                        max(camera[y * w + x][c] for y in range(y0, y0 + ph)
                            for x in range(x0, x0 + pw) for c in range(3))]
                        for x0, y0, pw, ph in patches]))
    for name, items in [("manifest", sources),
                        ("bayer", [s for s in sources if s["id"].startswith("cfa2-")]),
                        ("description", descriptions)]:
        (out / (name + ".json")).write_text(json.dumps({"schema": 1, "sources": items}, indent=2) + "\n")
    print(f"Generated {len(sources)} analytical cases in {out}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory for generated data")
    generate(parser.parse_args().output)
