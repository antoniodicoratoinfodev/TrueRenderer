#!/usr/bin/env python3
"""Two different linear scenes with identical clipped RAWs: an identifiability check."""
import argparse
import hashlib
import importlib
import json
from pathlib import Path
import struct

duel = importlib.import_module('generate-renderer-duel')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.mkdir()
    models = [('d750', duel.matrix('third_party/nikon-reference-data/nikon_d750.rs', 'D750_MATRIX')),
              ('d800', duel.matrix('third_party/nikon-reference-data/nikon_d750.rs', 'D800_MATRIX')),
              ('xt5', duel.matrix('third_party/fuji-codec/xt5.rs', 'MATRIX'))]
    w, h = 60, 42
    sources, descriptions = [], []
    for model, cm in models:
        neutral = duel.color.transform(cm, duel.D65)
        transform = duel.wb.multiply(cm, duel.RGB_TO_XYZ)
        transform = [[v/neutral[1] for v in row] for row in transform]
        near_limit = duel.color.transform(duel.wb.inverse(transform), [1.001]*3)
        for phase, pattern in enumerate(duel.PATTERNS):
            raw = duel.dng(w,h,pattern,cm,neutral,[duel.WHITE]*(w*h))
            rawpath = args.output/f'{model}-p{phase}.dng'; rawpath.write_bytes(raw)
            for kind, rgb in [('limit-colour',near_limit),('bright-neutral',[4.,4.,4.])]:
                camera = duel.color.transform(transform, rgb)
                assert min(camera)>1 and min(rgb)>0
                ident = f'{model}-{kind}-p{phase}'
                reference = struct.pack('<fff',*rgb)*(w*h)
                refpath = args.output/f'{ident}.f32'; refpath.write_bytes(reference)
                sources.append({'id':ident,'source':str(rawpath.resolve()),
                    'sha256':hashlib.sha256(raw).hexdigest(),'camera':'Generated '+model,
                    'truth':str(refpath.resolve()),'truth_sha256':hashlib.sha256(reference).hexdigest(),
                    'patches':[[6,6,w-12,h-12]],'repetitions':1})
                descriptions.append({'id':ident,'kind':kind,'model':model,'cfa_period':2,
                    'phase':phase,'true_rgb':rgb,'true_camera':camera,
                    'patch_sensor_range':[[min(camera),max(camera)]]})
    for name,rows in [('manifest',sources),('description',descriptions)]:
        data = {'schema':1, 'sources':rows}
        if name == 'description':
            data['scope'] = __doc__
        (args.output/f'{name}.json').write_text(json.dumps(data,indent=2)+'\n')
    print('Generated 12 identical-input pairs with different true colours/intensities')


if __name__ == '__main__':
    main()
