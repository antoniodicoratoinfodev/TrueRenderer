#!/usr/bin/env python3
"""Known-model highlight scenes; coefficients are not measured camera truth."""
import argparse
import hashlib
import importlib
import json
import math
from pathlib import Path
import random
import struct

duel = importlib.import_module('generate-renderer-duel')
XTRANS = [1,1,0,1,1,2, 1,1,2,1,1,0, 2,0,1,0,2,1,
          1,1,2,1,1,0, 1,1,0,1,1,2, 0,2,1,2,0,1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--validation', action='store_true')
    args = parser.parse_args()
    args.output.mkdir()
    rng = random.Random(17031 if args.validation else 9031)
    models = [('d750',duel.matrix('third_party/nikon-reference-data/nikon_d750.rs','D750_MATRIX')),
              ('d800',duel.matrix('third_party/nikon-reference-data/nikon_d750.rs','D800_MATRIX')),
              ('xt5',duel.matrix('third_party/fuji-codec/xt5.rs','MATRIX'))]
    w,h = (174,114) if args.validation else (156,96)
    sources, descriptions, bayer = [], [], []
    for model,cm in models:
        layouts = duel.PATTERNS if model != 'xt5' else [
            [XTRANS[((i//6+dy)%6)*6+(i%6+dx)%6] for i in range(36)]
            for dx,dy in ([(1,2),(3,5),(4,1),(5,4)] if args.validation else [(0,0),(1,3),(4,5),(5,2)])]
        for kind in ['neutral','warm','cool','edge','conflict','noisy','unclipped','fully-clipped']:
            white = ([.40/.39,1.,.21/.39] if args.validation else duel.D65)
            neutral = duel.color.transform(cm,white)
            transform = duel.wb.multiply(cm,duel.wb.multiply(duel.adapt(white),duel.RGB_TO_XYZ))
            transform = [[v/neutral[1] for v in row] for row in transform]
            hue = {'warm':[1.,.59,.32],'cool':[.36,.68,1.]}.get(kind,[1.,1.,1.])
            if args.validation and kind in ('warm','cool'):
                hue = [v*rng.uniform(.93,1.07) for v in hue]
            # Fix peak relative to the known maximum camera channel. This
            # deliberately exercises partial clipping at different whites.
            peak = (1.49 if args.validation else 1.38)/max(duel.color.transform(transform,hue))
            truth=[]
            for y in range(h):
                for x in range(w):
                    ramp = .24+.76*x/(w-1)+.015*y/(h-1)
                    rgb = [c*peak*ramp for c in hue]
                    if kind=='edge' and x<.53*y+24: rgb=[.06,.17,.11]
                    if kind=='conflict':
                        rgb = ([.33,.76,.24] if x<w*.27 else [.24,.41,.69])
                        rgb = [v*(.45 if x<w*.27 else 2.1) for v in rgb]
                    if kind=='unclipped': rgb=[v*.43 for v in rgb]
                    if kind=='fully-clipped': rgb=[4.,4.,4.]
                    truth.append(rgb)
            camera = [duel.color.transform(transform,p) for p in truth]
            for phase,pattern in enumerate(layouts):
                period=math.isqrt(len(pattern)); ident=f'{model}-{kind}-p{phase}'
                unclipped=[camera[y*w+x][pattern[(y%period)*period+x%period]] for y in range(h) for x in range(w)]
                noise = .0017 if args.validation else .001
                observed = [v+(rng.uniform(-noise,noise) if kind=='noisy' else 0.) for v in unclipped]
                samples=[min(duel.WHITE,max(duel.BLACK,round(duel.BLACK+(duel.WHITE-duel.BLACK)*v))) for v in observed]
                raw=duel.dng(w,h,pattern,cm,neutral,samples)
                linear=b''.join(struct.pack('<fff',*p) for p in truth)
                rawpath=args.output/f'{ident}.dng'; rawpath.write_bytes(raw)
                refpath=args.output/f'{ident}.f32'; refpath.write_bytes(linear)
                camera_data=b''.join(struct.pack('<f',v) for v in unclipped)
                (args.output/f'{ident}.cfa').write_bytes(camera_data)
                patches=[[8,8,w-16,h-16],[w*2//3,12,w//3-12,h-24]]
                source={'id':ident,'source':str(rawpath.resolve()),'sha256':hashlib.sha256(raw).hexdigest(),
                        'camera':'Generated '+model,'truth':str(refpath.resolve()),'truth_sha256':hashlib.sha256(linear).hexdigest(),
                        'patches':patches,'repetitions':1}
                sources.append(source)
                if period==2: bayer.append(source)
                descriptions.append({'id':ident,'kind':kind,'cfa_period':period,'matrix':cm,'white':white,'phase':phase,
                    'sensor_at_white':sum(v==duel.WHITE for v in samples),
                    'patch_sensor_range':[[min(camera[y*w+x][c] for y in range(y0,y0+ph) for x in range(x0,x0+pw) for c in range(3)),
                                           max(camera[y*w+x][c] for y in range(y0,y0+ph) for x in range(x0,x0+pw) for c in range(3))]
                                          for x0,y0,pw,ph in patches]})
    for name,items in [('manifest',sources),('bayer',bayer)]:
        (args.output/f'{name}.json').write_text(json.dumps({'schema':1,'sources':items},indent=2)+'\n')
    (args.output/'description.json').write_text(json.dumps({'scope':__doc__,'validation':args.validation,'sources':descriptions},indent=2)+'\n')
    print('Generated',len(sources),'scenes;',len(bayer),'also admitted by the historical engine')


if __name__=='__main__':
    main()
