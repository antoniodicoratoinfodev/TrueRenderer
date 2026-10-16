#!/usr/bin/env python3
"""Clipped remosaic of hashed photographic RGB surrogates, not sensor truth.

Uses the central tile of every photograph in a frozen photo-remosaic manifest.
The input triples are treated as linear Rec.2020 signal values for this model;
the original photographs' transfer, camera reconstruction and gamut are not
calibrated away. Source photographs are reused, exposures/noise are new cases.
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


def generate(manifest, output, seed):
    config_bytes = manifest.read_bytes()
    config = json.loads(config_bytes)
    assert config["protocol"] == "TRExp-photo-remosaic-v1"
    tiles = config["tiles"]
    assert len(tiles) == len(config["sources"]) * 9
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    sources, descriptions = [], []
    for source in config["sources"]:
        own = [t for t in tiles if t["source_id"] == source["id"]]
        assert len(own) == 9
        tile = sorted(own, key=lambda t: (t["y"], t["x"]))[4]
        data = (manifest.parent / tile["file"]).read_bytes()
        assert hashlib.sha256(data).hexdigest() == tile["sha256"]
        w, h = tile["width"], tile["height"]
        assert len(data) == w * h * 12 and 32 < min(w, h) <= max(w, h) <= 512
        rgb = list(struct.iter_unpack("<fff", data))
        assert all(math.isfinite(v) and v >= 0 for p in rgb for v in p)
        model = "D750" if source["id"] % 2 == 0 else "D800"
        cm = duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", model + "_MATRIX")
        neutral = duel.color.transform(cm, duel.D65)
        forward = duel.wb.multiply(cm, duel.RGB_TO_XYZ)
        forward = [[v / neutral[1] for v in row] for row in forward]
        for exposure_index, exposure in enumerate([0.9, 2.1]):
            truth = [[v * exposure for v in p] for p in rgb]
            camera = [duel.color.transform(forward, p) for p in truth]
            reference = b"".join(struct.pack("<fff", *p) for p in truth)
            for noise_index, sigma in enumerate([0., .001]):
                for phase, pattern in enumerate(duel.PATTERNS):
                    rng = random.Random(seed + 100003 * source["id"] + 997 * exposure_index
                                        + 37 * noise_index + phase)
                    cfa = [p[pattern[(i // w % 2) * 2 + i % w % 2]] for i, p in enumerate(camera)]
                    observed = [v + rng.gauss(0., sigma * math.sqrt(max(v, 0.)) + sigma * .3)
                                if sigma else v for v in cfa]
                    codes = [min(duel.WHITE, max(0, round(duel.BLACK + (duel.WHITE-duel.BLACK) * v)))
                             for v in observed]
                    ident = f"photo{source['id']}-e{exposure_index}-n{noise_index}-p{phase}-s{seed}"
                    raw = duel.dng(w, h, pattern, cm, neutral, codes)
                    rawpath, refpath = output / (ident + ".dng"), output / (ident + ".f32")
                    rawpath.write_bytes(raw)
                    refpath.write_bytes(reference)
                    rawpath.with_suffix(".cfa").write_bytes(b"".join(struct.pack("<f", v) for v in cfa))
                    sources.append(dict(id=ident, source=str(rawpath), sha256=hashlib.sha256(raw).hexdigest(),
                                        truth=str(refpath), truth_sha256=hashlib.sha256(reference).hexdigest(),
                                        camera="Photographic surrogate " + model,
                                        patches=[[0, 0, w-1, h-1], [16, 16, w-32, h-32]], repetitions=1))
                    descriptions.append(dict(id=ident, tile=tile, exposure=exposure, noise_sigma=sigma,
                                             source=source, phase=phase, size=[w, h],
                                             saturated_sites=codes.count(duel.WHITE)))
    (output / "manifest.json").write_text(json.dumps(dict(schema=1, sources=sources), indent=2) + "\n")
    (output / "description.json").write_text(json.dumps(dict(seed=seed, scope=__doc__,
        source_manifest_sha256=hashlib.sha256(config_bytes).hexdigest(), sources=descriptions), indent=2) + "\n")
    print(f"Generated {len(sources)} cases from {len(config['sources'])} reused photographic sources")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--seed", type=int, required=True)
    args = parser.parse_args()
    generate(args.manifest, args.output, args.seed)
