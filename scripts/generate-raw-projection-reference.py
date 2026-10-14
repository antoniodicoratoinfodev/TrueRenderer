#!/usr/bin/env python3
"""Independent Decimal reference for the final RAW matrix, using exact binary inputs."""
import argparse
from decimal import Decimal, localcontext
import hashlib
import importlib
import json
from pathlib import Path
import random
import struct

duel = importlib.import_module('generate-renderer-duel')


def f32(v):
    return struct.unpack('<f', struct.pack('<f', v))[0]


def bits(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    matrices = []
    for name,path,symbol in [('d750','third_party/nikon-reference-data/nikon_d750.rs','D750_MATRIX'),
                             ('d800','third_party/nikon-reference-data/nikon_d750.rs','D800_MATRIX'),
                             ('xt5','third_party/fuji-codec/xt5.rs','MATRIX')]:
        cm = duel.matrix(path,symbol)
        for label,white in [('d65',duel.D65),('v1',[.95682,1.,.92149]),('v2',[.40/.39,1.,.21/.39])]:
            forward = duel.wb.multiply(cm,duel.wb.multiply(duel.adapt(white),duel.RGB_TO_XYZ))
            neutral = duel.color.transform(cm,white)
            forward = [[v/neutral[r] for v in row] for r,row in enumerate(forward)]
            matrices.append((name+'-'+label,duel.wb.inverse(forward)))
    # Exact powers of two expose coefficient truncation and cancellation,
    # including extended positive and negative values.
    matrices.append(('cancellation',[[1.+2.**-26,-1.,2.**-30],
                                    [2.**-24,1.+2.**-25,-1.],
                                    [-1.,2.**-28,1.+2.**-27]]))
    randomizer = random.Random(20261013)
    signals = [[v,v,v] for v in [-.125,0.,2.**-18,.18,.97,1.,2.,16.]]
    signals += [[f32(randomizer.uniform(-.2,4.)) for _ in range(3)] for _ in range(56)]
    rows=[]
    with localcontext() as context:
        context.prec = 110
        for label,matrix in matrices:
            for signal in signals:
                camera = [f32(v) for v in signal]
                exact = [sum(Decimal.from_float(v)*Decimal.from_float(c)
                             for v,c in zip(row,camera)) for row in matrix]
                rows.append({'model':label,'matrix':matrix,'camera':camera,
                             'expected_bits':[bits(float(v)) for v in exact],
                             'exact_decimal':[str(v) for v in exact]})
    result={'schema':1,'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'scope':'Exact binary matrix coefficients and fp32 camera inputs evaluated with 110 decimal digits. Final values rounded to fp32. Numerical precision only, not physical profiles.',
            'cases':rows}
    with args.output.open('x') as stream:
        json.dump(result,stream,separators=(',',':'),allow_nan=False)
        stream.write('\n')
    print(len(rows),'matrix/vector cases')


if __name__ == '__main__':
    main()
