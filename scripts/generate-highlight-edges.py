#!/usr/bin/env python3
"""Known-model highlight edges and counterexamples; never physical camera truth."""
import argparse
import hashlib
import importlib
import json
import math
from pathlib import Path
import random
import struct

duel = importlib.import_module('generate-renderer-duel')
XTRANS = importlib.import_module('generate-highlight-validation').XTRANS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--development', action='store_true')
    mode.add_argument('--validation', action='store_true')
    parser.add_argument('--validation-round', type=int, choices=[1,2,3], default=1)
    args = parser.parse_args()
    if args.development and args.validation_round != 1:
        parser.error('--validation-round requires --validation')
    second = args.validation and args.validation_round == 2
    third = args.validation and args.validation_round == 3
    args.output.mkdir()
    rng = random.Random(48271 if third else 38353 if second else 28351 if args.validation else 98103)
    models = [('d750', duel.matrix('third_party/nikon-reference-data/nikon_d750.rs', 'D750_MATRIX')),
              ('d800', duel.matrix('third_party/nikon-reference-data/nikon_d750.rs', 'D800_MATRIX')),
              ('xt5', duel.matrix('third_party/fuji-codec/xt5.rs', 'MATRIX'))]
    w, h = (210,150) if third else (198,138) if second else (186, 126) if args.validation else (168, 108)
    white = [.948,1.,1.02] if third else [.976,1.,.857] if second else [.987, 1., .703] if args.validation else duel.D65
    kinds = ['step-neutral', 'step-warm', 'step-cool', 'occlusion-neutral',
             'occlusion-warm', 'chroma-edge', 'noisy-warm', 'noisy-cool',
             'fully-clipped-neutral', 'fully-clipped-colour']
    sources, descriptions, bayer = [], [], []
    for model, cm in models:
        neutral = duel.color.transform(cm, white)
        transform = duel.wb.multiply(cm, duel.wb.multiply(duel.adapt(white), duel.RGB_TO_XYZ))
        transform = [[v/neutral[1] for v in row] for row in transform]
        layouts = duel.PATTERNS if model != 'xt5' else [
            [XTRANS[((i//6+dy)%6)*6+(i%6+dx)%6] for i in range(36)]
            for dx, dy in ([(0,4),(2,3),(3,0),(5,1)] if third else [(0,1),(2,4),(4,2),(5,3)] if second else
                          [(2,1),(3,4),(5,0),(0,5)] if args.validation else [(0,0),(1,3),(4,5),(5,2)])]
        for kind in kinds:
            hue = ([1., .56, .29] if kind.endswith('warm') else
                   [.32, .63, 1.] if kind.endswith('cool') else [1., 1., 1.])
            if args.validation and kind.endswith(('warm', 'cool')):
                hue = [v*rng.uniform(.92,1.08) for v in hue]
            peak = (1.68 if third else 1.57 if second else 1.63 if args.validation else 1.48)/max(duel.color.transform(transform, hue))
            for phase, pattern in enumerate(layouts):
                truth = []
                for y in range(h):
                    for x in range(w):
                        t = (y/(h-1) if phase >= 2 else x/(w-1))
                        light = .32+.68*t
                        if kind.startswith('step-'):
                            # Both axes and both parities; validation moves every edge.
                            p = y if phase >= 2 else x
                            length = h if phase >= 2 else w
                            cuts = [length//3 + (phase%2) + (2 if second else 0),
                                    2*length//3 + (3-phase if third else 5-phase if second else 1 if args.validation else 0)]
                            light = .39 if p < cuts[0] else (.77 if p < cuts[1] else 1.)
                        rgb = [v*peak*light for v in hue]
                        if kind.startswith('occlusion-'):
                            light = .26+.74*x/(w-1)+.013*y/(h-1)
                            rgb = [v*peak*light for v in hue]
                            edge = (.49*y+31+7*math.sin(.055*y) if third else
                                    .61*y+28+5*math.sin(.04*y) if second else (.71 if args.validation else .53)*y+23)
                            if x < edge:
                                rgb = [.11, .19, .07]
                        if kind == 'chroma-edge':
                            hue2 = [1.,.51,.22] if x < .61*y+w*.32 else [.23,.62,1.]
                            scale = (1.65 if args.validation else 1.51)/max(duel.color.transform(transform,hue2))
                            rgb = [v*scale*(.35+.65*y/(h-1)) for v in hue2]
                        if kind == 'fully-clipped-neutral': rgb = [8.,8.,8.]
                        if kind == 'fully-clipped-colour': rgb = [8.,7.,6.]
                        truth.append(rgb)
                camera = [duel.color.transform(transform, p) for p in truth]
                period = math.isqrt(len(pattern))
                values = [camera[y*w+x][pattern[(y%period)*period+x%period]] for y in range(h) for x in range(w)]
                noise = .0027 if third else .0023 if second else .0019 if args.validation else .0011
                observed = [v+(rng.uniform(-noise,noise) if kind.startswith('noisy-') else 0.) for v in values]
                samples = [min(duel.WHITE,max(duel.BLACK,round(duel.BLACK+(duel.WHITE-duel.BLACK)*v))) for v in observed]
                ident = f'{model}-{kind}-p{phase}'
                raw = duel.dng(w,h,pattern,cm,neutral,samples)
                rawpath = args.output/f'{ident}.dng'; rawpath.write_bytes(raw)
                refpath = args.output/f'{ident}.f32'
                linear = b''.join(struct.pack('<fff',*p) for p in truth); refpath.write_bytes(linear)
                (args.output/f'{ident}.cfa').write_bytes(b''.join(struct.pack('<f',v) for v in values))
                patches = [[8,8,w-16,h-16]]
                source = {'id':ident,'source':str(rawpath.resolve()),'sha256':hashlib.sha256(raw).hexdigest(),
                          'camera':'Generated '+model,'truth':str(refpath.resolve()),
                          'truth_sha256':hashlib.sha256(linear).hexdigest(),'patches':patches,'repetitions':1}
                sources.append(source)
                if period == 2: bayer.append(source)
                descriptions.append({'id':ident,'kind':kind,'model':model,'white':white,'phase':phase,
                                     'cfa_period':period,'saturated_sites':sum(v==duel.WHITE for v in samples),
                                     'patch_sensor_range':[[min(camera[y*w+x][c] for y in range(y0,y0+ph)
                                        for x in range(x0,x0+pw) for c in range(3)),
                                        max(camera[y*w+x][c] for y in range(y0,y0+ph)
                                        for x in range(x0,x0+pw) for c in range(3))]
                                        for x0,y0,pw,ph in patches]})
    for name, rows in [('manifest',sources),('bayer',bayer),('description',descriptions)]:
        data = {'schema':1, 'sources':rows}
        if name == 'description':
            data.update(scope=__doc__, validation=args.validation,
                        validation_round=args.validation_round if args.validation else 0)
        (args.output/f'{name}.json').write_text(json.dumps(data,indent=2)+'\n')
    print(f'Generated {len(sources)} cases, {len(bayer)} admitted by the historical engine')


if __name__ == '__main__':
    main()
