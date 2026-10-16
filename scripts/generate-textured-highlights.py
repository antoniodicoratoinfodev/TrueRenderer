#!/usr/bin/env python3
"""Known-model coloured textures and counterexamples, not calibrated photos.

Different frequency mixtures, envelopes, source colours, shot/read noise and
partial clipping. Camera matrices are compatibility data also used for truth.
Freeze implementation/generator before a separate verification seed.
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
    sources, descriptions = [], []
    models = [("linear", [[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]]),
              ("d750", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D750_MATRIX")),
              ("d800", duel.matrix("third_party/nikon-reference-data/nikon_d750.rs", "D800_MATRIX"))]
    kinds = ["mixed-waves", "envelope", "fine-texture", "independent-green", "two-sources",
             "noise", "multi-clipped", "unclipped", "point-error"]
    for mi, (name, cm) in enumerate(models):
        for phase, pattern in enumerate(duel.PATTERNS):
            rng = random.Random(seed + mi*100003 + phase*997)
            w, h = rng.randrange(93,113), rng.randrange(75,91)
            white = [rng.uniform(.87,1.13),1.,rng.uniform(.84,1.17)]
            neutral = duel.color.transform(cm,white)
            forward = duel.wb.multiply(cm,duel.wb.multiply(duel.adapt(white),duel.RGB_TO_XYZ))
            inverse = duel.wb.inverse([[v/neutral[1] for v in row] for row in forward])
            frequencies = [[rng.uniform(.06,.21)*rng.choice([-1,1]) for _ in range(2)] for _ in range(3)]
            phases = [rng.uniform(-math.pi,math.pi) for _ in range(3)]
            a, b = rng.uniform(.65,.95), rng.uniform(1.1,1.5)
            point_x, point_y = w//3, h//3
            if pattern[point_y%2*2+point_x%2] != 1:
                point_x += 1
            for kind in kinds:
                truth, cfa, observed = [], [], []
                for y in range(h):
                    for x in range(w):
                        rate = 3.7 if kind == "fine-texture" else 1.
                        waves = [math.sin(rate*(fx*x+fy*y)+p) for (fx,fy),p in zip(frequencies,phases)]
                        envelope = .65+.35*math.exp(-((x-.6*w)/(.35*w))**2) if kind == "envelope" else 1.
                        red = .36 + .025*envelope*(waves[0]+.6*waves[2]) + .0008*y
                        blue = .30 + .035*envelope*(waves[1]-.4*waves[2]) + .0005*x
                        green = a*red+b*blue+.12+.0044*y
                        if kind == "independent-green":
                            green += .035*math.sin(.31*x-.23*y+.3)
                        elif kind == "two-sources" and x>.4*w+.14*y:
                            green += .13*red-.29*blue
                        elif kind == "multi-clipped":
                            red *= 2.8
                        elif kind == "unclipped":
                            green *= .6
                        camera = [red,green,blue]
                        truth.append(duel.color.transform(inverse,camera))
                        c = pattern[y%2*2+x%2]
                        cfa.append(camera[c])
                        noise = rng.gauss(0.,.001*math.sqrt(camera[c])+.0005) if kind == "noise" else 0.
                        if kind == "point-error" and x==point_x and y==point_y:
                            noise += .035
                        observed.append(camera[c]+noise)
                codes = [min(duel.WHITE,max(0,round(duel.BLACK+(duel.WHITE-duel.BLACK)*v))) for v in observed]
                ident = f"t{seed}-{name}-p{phase}-{kind}"
                raw = duel.dng(w,h,pattern,cm,neutral,codes)
                reference = b"".join(struct.pack("<fff",*v) for v in truth)
                path, ref = out/(ident+".dng"),out/(ident+".f32")
                path.write_bytes(raw);ref.write_bytes(reference)
                path.with_suffix(".cfa").write_bytes(b"".join(struct.pack("<f",v) for v in cfa))
                sources.append(dict(id=ident,source=str(path),sha256=hashlib.sha256(raw).hexdigest(),
                                    truth=str(ref),truth_sha256=hashlib.sha256(reference).hexdigest(),
                                    camera="Analytical "+name,repetitions=1,patches=[[0,0,w-1,h-1],[7,7,w-14,h-14]]))
                descriptions.append(dict(id=ident,kind=kind,size=[w,h],pattern=pattern,frequencies=frequencies,
                                         phases=phases,colour_relation=[a,b],white=white,perturbed_site=[point_x,point_y],
                                         saturated_sites=codes.count(duel.WHITE)))
    for name, records in [("manifest",sources),("description",descriptions)]:
        (out/(name+".json")).write_text(json.dumps(dict(schema=1,seed=seed,sources=records),indent=2)+"\n")
    print(f"Generated {len(sources)} textured highlights, seed {seed}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output",type=Path)
    parser.add_argument("--seed",type=int,required=True)
    args = parser.parse_args()
    generate(args.output,args.seed)
