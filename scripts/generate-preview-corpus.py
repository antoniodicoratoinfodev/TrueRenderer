#!/usr/bin/env python3
"""Generate 1000 distinct synthetic Bayer signals; never use personal photos."""
from pathlib import Path
import array, hashlib, json, struct
ROOT=Path(__file__).resolve().parents[1]
source=(ROOT/'var/format-fixtures/07-bayer.dng').read_bytes()
w,h=512,384
ifd=struct.unpack_from('<I',source,4)[0]
count=struct.unpack_from('<H',source,ifd)[0]
entries={struct.unpack_from('<H',source,ifd+2+i*12)[0]:ifd+2+i*12 for i in range(count)}
offset=struct.unpack_from('<I',source,entries[273]+8)[0]
header=bytearray(source[:offset])
for tag,value in [(256,w),(257,h),(278,h),(279,w*h*2)]:struct.pack_into('<I',header,entries[tag]+8,value)
for tag,values in [(50720,[w,h]),(50829,[0,0,h,w])]:
    struct.pack_into('<'+'I'*len(values),header,struct.unpack_from('<I',header,entries[tag]+8)[0],*values)
pixels=array.array('H');pixels.frombytes(source[offset:])
base=[pixels[((y//2)*4+y%2)*1024+(x//2)*4+x%2] for y in range(h) for x in range(w)]
folder=ROOT/'var/preview-corpus';folder.mkdir(parents=True,exist_ok=True)
manifest=[]
for i in range(1000):
    signal=array.array('H',(v+i*7 for v in base))
    data=header+signal.tobytes()
    name=f'synthetic-{i:04}.dng'
    (folder/name).write_bytes(data)
    manifest.append({'file':name,'sha256':hashlib.sha256(data).hexdigest()})
assert len({m['sha256'] for m in manifest})==1000
(folder/'manifest.json').write_text(json.dumps({'scope':'1000 distinct synthetic Bayer signals, not real camera RAWs or 12/24/45 MP inputs','size':[w,h],'files':manifest},indent=2)+'\n')
print('Generated 1000 distinct 512x384 DNGs under var/preview-corpus')
