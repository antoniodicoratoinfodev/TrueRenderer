#!/usr/bin/env python3
"""Generate known-model Bayer DNGs for the two-renderer comparison, never camera truth."""
import argparse
import hashlib
import importlib
import json
import math
from pathlib import Path
import re
import struct

wb = importlib.import_module("analyze-raw-wb")
color = importlib.import_module("calibrate-raw-profile")
ROOT = Path(__file__).resolve().parents[1]
XYZ_TO_RGB = [[1.716651187971268, -.355670783776392, -.253366281373660],
              [-.666684351832489, 1.616481236634939, .015768545813911],
              [.017639857445311, -.042770613257809, .942103121235474]]
RGB_TO_XYZ = wb.inverse(XYZ_TO_RGB)
BRADFORD = [[.8951, .2664, -.1614], [-.7502, 1.7135, .0367], [.0389, -.0685, 1.0296]]
D65 = [.9504559270516716, 1., 1.0890577507598784]
PATTERNS = [[0, 1, 1, 2], [2, 1, 1, 0], [1, 0, 2, 1], [1, 2, 0, 1]]
BLACK, WHITE = 512, 65000


def adapt(white):
    a, b = color.transform(BRADFORD, D65), color.transform(BRADFORD, white)
    return wb.multiply(wb.inverse(BRADFORD), wb.multiply(
        [[b[r]/a[r] if r == c else 0. for c in range(3)] for r in range(3)], BRADFORD))


def matrix(path, name):
    text = (ROOT / path).read_text()
    values = [int(x) / 10000 for x in re.search(rf'const {name}:.*?=\s*\[([^]]+)\]', text, re.S)[1].split(',') if x.strip()]
    assert len(values) == 9
    return [values[i:i+3] for i in range(0, 9, 3)]


def dng(width, height, pattern, cm, neutral, samples):
    tags = {}
    def ints(tag, kind, values):
        tags[tag] = kind, len(values), struct.pack('<'+{1:'B', 3:'H', 4:'I'}[kind]*len(values), *values)
    def rational(tag, values, signed=False):
        tags[tag] = (10 if signed else 5), len(values), b''.join(
            struct.pack('<iI' if signed else '<II', round(v*(1 if v == int(v) else 10000000)),
                        1 if v == int(v) else 10000000) for v in values)
    def text(tag, value):
        data = value.encode()+b'\0'
        tags[tag] = 2, len(data), data
    for tag, kind, value in [(254,4,0),(256,4,width),(257,4,height),(258,3,16),(259,3,1),
                             (262,3,32803),(273,4,0),(274,3,1),(277,3,1),(278,4,height),
                             (279,4,len(samples)*2),(284,3,1),(50711,3,1),(50717,4,WHITE),(50778,3,21)]:
        ints(tag, kind, [value])
    period = math.isqrt(len(pattern))
    assert period in (2, 6) and period*period == len(pattern)
    ints(33421,3,[period,period]); ints(33422,1,pattern)
    ints(50706,1,[1,4,0,0]); ints(50707,1,[1,1,0,0]); ints(50710,1,[0,1,2])
    ints(50713,3,[1,1]); rational(50714,[BLACK]); rational(50728,neutral)
    rational(50721,[v for row in cm for v in row],True)
    text(271,'TrueRenderer'); text(272,'Synthetic DNG'); text(50708,'TrueRenderer Synthetic DNG')
    end = 8+2+12*len(tags)+4
    entries, extra = bytearray(), bytearray()
    strip = None
    for tag,(kind,count,data) in sorted(tags.items()):
        entries.extend(struct.pack('<HHI',tag,kind,count))
        if tag == 273:
            strip = len(entries)
        if len(data) <= 4:
            entries.extend(data.ljust(4,b'\0'))
        else:
            entries.extend(struct.pack('<I',end+len(extra))); extra.extend(data)
            if len(extra)%2: extra.extend(b'\0')
    entries[strip:strip+4] = struct.pack('<I',end+len(extra))
    return b'II*\0\x08\0\0\0'+struct.pack('<H',len(tags))+entries+bytes(4)+extra+struct.pack('<'+'H'*len(samples),*samples)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output',type=Path)
    parser.add_argument('--validation', action='store_true', help='Independent colours, illuminants, clipped coloured lights and edges')
    args = parser.parse_args()
    args.output.mkdir()
    models = [('d750',matrix('third_party/nikon-reference-data/nikon_d750.rs','D750_MATRIX')),
              ('d800',matrix('third_party/nikon-reference-data/nikon_d750.rs','D800_MATRIX')),
              ('xt5',matrix('third_party/fuji-codec/xt5.rs','MATRIX'))]
    whites = [('d65',D65),('d50',[.9642956764295677,1.,.8251046025104603]),
              ('a',[.44757/.40745,1.,(1.-.44757-.40745)/.40745])]
    palette = [[v,v,v] for v in [.015,.03,.08,.18,.4,.65,.85,.92]]
    palette += [[.08+.12*r,.08+.12*g,.08+.12*b] for r,g,b in
                [(1,1,0),(1,0,1),(0,1,1),(2,1,0),(2,0,1),(1,2,0),(0,2,1),(1,0,2),
                 (0,1,2),(2,2,0),(2,0,2),(0,2,2),(3,1,1),(1,3,1),(1,1,3),(2,2,1)]]
    sources, descriptions = [], []
    def emit(name, cm, white, truth, w, h, patches, kind, labels):
        neutral = color.transform(cm,white)
        transform = wb.multiply(cm,wb.multiply(adapt(white),RGB_TO_XYZ))
        transform = [[v/neutral[1] for v in row] for row in transform]
        camera = [color.transform(transform,p) for p in truth]
        for phase, pattern in enumerate(PATTERNS):
            ident = f'{name}-p{phase}'
            unclipped = [camera[i][pattern[(i//w%2)*2+i%w%2]] for i in range(w*h)]
            samples = [min(WHITE,max(BLACK,round(BLACK+(WHITE-BLACK)*v))) for v in unclipped]
            raw = dng(w,h,pattern,cm,neutral,samples)
            linear = b''.join(struct.pack('<fff',*p) for p in truth)
            rawpath=args.output/f'{ident}.dng';rawpath.write_bytes(raw)
            refpath=args.output/f'{ident}.f32';refpath.write_bytes(linear)
            sources.append({'id':ident,'source':str(rawpath.resolve()),'sha256':hashlib.sha256(raw).hexdigest(),
                            'camera':'Generated Bayer: '+name,'truth':str(refpath.resolve()),
                            'truth_sha256':hashlib.sha256(linear).hexdigest(),'patches':patches,'repetitions':1})
            descriptions.append({'id':ident,'kind':kind,'matrix':cm,'white':white,'phase':phase,
                                 'labels':labels,'sensor_below_zero':sum(v<0 for v in unclipped),
                                 'sensor_at_or_above_white':sum(v>=1 for v in unclipped),
                                 'patch_sensor_range':[[min(camera[y*w+x][c] for y in range(y0,y0+ph) for x in range(x0,x0+pw) for c in range(3)),max(camera[y*w+x][c] for y in range(y0,y0+ph) for x in range(x0,x0+pw) for c in range(3))] for x0,y0,pw,ph in patches]})
    if args.validation:
        validation_whites = [('d65', D65), ('v1', [.95682, 1., .92149]),
                             ('v2', [.40/.39, 1., .21/.39])]
        hues = [[.72,.24,.17], [.24,.70,.20], [.18,.28,.75],
                [.70,.64,.21], [.63,.22,.62], [.20,.63,.67]]
        w,h=192,128
        patches=[[x*32+6,y*32+6,20,20] for y in range(4) for x in range(6)]
        for model,cm in models:
            for label,white in validation_whites:
                for kind,levels in [('validation-color', [.09,.23,.41,.61]),
                                    ('clipped-color', [.93,1.41,2.13,3.71])]:
                    palette = [[c*level for c in hue] for level in levels for hue in hues]
                    truth=[palette[(y//32)*6+x//32] for y in range(h) for x in range(w)]
                    emit(f'{kind}-{model}-{label}',cm,white,truth,w,h,patches,kind,
                         [[level,hue] for level in levels for hue in range(6)])
            for scene in range(2):
                truth=[]
                for y in range(h):
                    for x in range(w):
                        if scene == 0:
                            # Bright neutral sky against an oblique coloured object.
                            rgb = [1.47,1.47,1.47] if x > .73*y+39 else [.13,.32,.24]
                        else:
                            # Coloured emitters, stripe boundaries and a smooth intensity ramp.
                            hue = hues[(x//24+y//31)%6]
                            rgb = [c*(.37+3.6*y/(h-1)) for c in hue]
                        truth.append(rgb)
                emit(f'validation-edge-{model}-{scene}',cm,D65,truth,w,h,
                     [[8,8,w-16,h-16]],'clipped-edge',[scene])
        for scene in range(8):
            truth=[]
            for y in range(h):
                for x in range(w):
                    v = .36+.19*math.sin((.17+.09*scene)*x+(.11+.047*scene)*y+.31)
                    truth.append([v,v,v] if scene<6 else
                                 [v,.32+.18*math.cos(.37*x-.29*y),.29+.16*math.sin(.53*y+.21*x)])
            emit(f'validation-detail-{scene}',XYZ_TO_RGB,D65,truth,w,h,
                 [[8,8,w-16,h-16]],'detail',[scene])
        (args.output/'manifest.json').write_text(json.dumps({'schema':1,'sources':sources},indent=2)+'\n')
        (args.output/'description.json').write_text(json.dumps({'scope':'Independent mathematical validation; compatibility matrices are model coefficients, not physical camera truth. Includes clipped coloured lights and boundaries.','sources':descriptions},indent=2)+'\n')
        print('Generated',len(sources),'independent validation pairs')
        return
    w,h=192,128
    patches=[[x*32+6,y*32+6,20,20] for y in range(4) for x in range(6)]
    truth=[palette[(y//32)*6+x//32] for y in range(h) for x in range(w)]
    for model,cm in models:
        for label,white in whites:
            emit(f'color-{model}-{label}',cm,white,truth,w,h,patches,'color',list(range(24)))
    w,h=192,128
    for scene in range(8):
        truth=[]
        for y in range(h):
            for x in range(w):
                v = [.1+.6*x/(w-1), .4+.2*math.sin(.63*x), .4+.2*math.sin(.63*y),
                     .4+.2*math.sin(.47*x+.39*y), .65 if x>.61*y+37 else .15,
                     .4+.2*math.sin(.016*x*x+.01*y*y),
                     .4+.16*math.sin(.49*x+.41*y), .4+.15*math.sin(.7*x)][scene]
                truth.append([v,v,v] if scene!=7 else [v,.4+.15*math.cos(.68*y),.4+.15*math.sin(.48*x+.39*y)])
        emit(f'detail-{scene}',XYZ_TO_RGB,D65,truth,w,h,[[8,8,w-16,h-16]],'detail',[scene])
    w,h=192,64
    highlight_palette=[[v,v,v] for v in [.97,.98,.99,1.,1.2,2.]]
    patches=[[x*32+6,6,20,52] for x in range(6)]
    truth=[highlight_palette[x//32] for y in range(h) for x in range(w)]
    for model,cm in models:
        emit(f'highlight-{model}',cm,D65,truth,w,h,patches,'clipped-neutral',[.97,.98,.99,1.,1.2,2.])
    (args.output/'manifest.json').write_text(json.dumps({'schema':1,'sources':sources},indent=2)+'\n')
    (args.output/'description.json').write_text(json.dumps({'scope':'Numerical forward-model truth, not a physical camera characterization. Compatibility matrices reused as model coefficients only; Bayer even for X-T5 coefficients. Clipped patches reported separately.','sources':descriptions},indent=2)+'\n')
    print('Generated',len(sources),'bounded DNG/reference pairs')


if __name__=='__main__':
    main()
