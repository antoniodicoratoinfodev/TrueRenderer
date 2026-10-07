#!/usr/bin/env python3
"""Make metadata-only WB variants of an explicit, generated single-illuminant DNG.

Sensor samples and every byte outside AsShotNeutral stay identical. Apple pairs
are numerically fitted to integer Kelvin/tint using the installed CIRAWFilter,
so the app's integer UI does not introduce a rounding mismatch in the oracle.
This is a development-stage check, not camera colour qualification.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
original = a.source.read_bytes()
assert original[:8] == b'II*\0\x08\0\0\0'
n = struct.unpack_from('<H', original, 8)[0]
tags = {struct.unpack_from('<H', original, 10+i*12)[0]: 10+i*12 for i in range(n)}
assert 50722 not in tags, 'Requires single illuminant'
entry = tags[50728]
assert struct.unpack_from('<HI', original, entry+2) == (5, 3)
offset = struct.unpack_from('<I', original, entry+8)[0]
assert [struct.unpack_from('<II', original, offset+i*8) for i in range(3)] == [(1, 1)]*3
(a.output/'original.dng').write_bytes(original)

def write_variant(name, red, blue):
    data = bytearray(original)
    for c, gain in enumerate([red, 1, blue]):
        struct.pack_into('<II', data, offset+c*8, round(100000000/gain), 100000000)
    assert data[:offset] == original[:offset] and data[offset+24:] == original[offset+24:]
    path = a.output/name
    path.write_bytes(data)
    return path

cases = []
for red, blue in [(1500,750), (800,1250), (1250,1500)]:
    name = f'sensor-{red}-{blue}.dng'
    # Exact ratios, no decimal approximation for the relative-gain engines.
    data = bytearray(original)
    for c,gain in enumerate([red,1000,blue]): struct.pack_into('<II',data,offset+c*8,1000,gain)
    (a.output/name).write_bytes(data)
    for engine in ['LibRawBilinear','LibRawAhd','TrueRenderer']:
        cases.append(dict(engine=engine,camera=name,wb=dict(red=red,blue=blue),tolerance=0.00002))

with tempfile.TemporaryDirectory() as temporary:
    helper = Path(temporary)/'read-wb'
    source = helper.with_suffix('.m')
    source.write_text('''#import <Foundation/Foundation.h>
#import <CoreImage/CoreImage.h>
int main(int argc, char **argv) { @autoreleasepool {
 if(argc!=2)return 2;
 NSData *d=[NSData dataWithContentsOfFile:[NSString stringWithUTF8String:argv[1]]];
 CIRAWFilter *f=[CIRAWFilter filterWithImageData:d identifierHint:@"com.adobe.raw-image"];
 if(!f.decoderVersion.length)return 3;
 printf("[%.9g,%.9g]\\n", f.neutralTemperature,f.neutralTint);
} }
''')
    subprocess.run(['xcrun','clang','-fobjc-arc','-framework','Foundation','-framework','CoreImage',str(source),'-o',str(helper)], check=True)
    def read(name, gains):
        file = write_variant(name, *[math.exp(x) for x in gains])
        return json.loads(subprocess.check_output([str(helper),str(file)]))
    for temperature,tint in [(3500,-20),(6500,0),(9000,25)]:
        name = f'apple-{temperature}-{tint}.dng'
        gains = [0.,0.]
        for iteration in range(20):
            actual = read(name,gains)
            residual = [temperature-actual[0],tint-actual[1]]
            if abs(residual[0]) < 0.02 and abs(residual[1]) < 0.0002: break
            delta = 0.001
            columns = []
            for c in range(2):
                probe = gains.copy();probe[c] += delta
                b = read(name,probe)
                columns.append([(b[i]-actual[i])/delta for i in range(2)])
            x,y = columns
            det = x[0]*y[1]-x[1]*y[0]
            step = [(residual[0]*y[1]-residual[1]*y[0])/det, (x[0]*residual[1]-x[1]*residual[0])/det]
            gains = [g+max(-0.3,min(0.3,s)) for g,s in zip(gains,step)]
        actual = read(name,gains)
        assert abs(actual[0]-temperature)<0.02 and abs(actual[1]-tint)<0.0002, actual
        cases.append(dict(engine='Apple',camera=name,wb=dict(red=1000,blue=1000,apple_temperature=temperature,apple_tint=tint),metadata_native_wb=actual,tolerance=0.00002))

for case in cases:
    data = (a.output/case['camera']).read_bytes()
    assert data[:offset] == original[:offset] and data[offset+24:] == original[offset+24:]
    case['camera_sha256'] = hashlib.sha256(data).hexdigest()
manifest = dict(source='original.dng',source_sha256=hashlib.sha256(original).hexdigest(),only_changed_tag='AsShotNeutral (50728)',sensor_samples_identical=True,cases=cases)
(a.output/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Generated',len(cases),'WB cases; identical sensor data:', a.output)
