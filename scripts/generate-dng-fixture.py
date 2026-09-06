#!/usr/bin/env python3
"""Own uncompressed Bayer DNG, without embedded JPEG/preview. No third-party photo."""
from pathlib import Path
import struct,json
root=Path(__file__).resolve().parents[1]
folder=root/'var/format-fixtures';folder.mkdir(parents=True,exist_ok=True)
w,h=1024,768
tags=[]
def add(tag,type,values):
    if type==2: data=values.encode()+b'\0';count=len(data)
    elif type==1: data=bytes(values);count=len(values)
    elif type in (3,4):data=struct.pack('<'+('H' if type==3 else 'I')*len(values),*values);count=len(values)
    elif type in (5,10):data=b''.join(struct.pack('<'+('II' if type==5 else 'ii'),a,b) for a,b in values);count=len(values)
    tags.append([tag,type,count,data])
for tag,typ,v in [(254,4,[0]),(256,4,[w]),(257,4,[h]),(258,3,[16]),(259,3,[1]),(262,3,[32803]),(271,2,'TrueRenderer'),(272,2,'Synthetic DNG'),(273,4,[0]),(274,3,[1]),(277,3,[1]),(278,4,[h]),(279,4,[w*h*2]),(284,3,[1]),(33421,3,[2,2]),(33422,1,[0,1,1,2]),(50706,1,[1,4,0,0]),(50707,1,[1,1,0,0]),(50708,2,'TrueRenderer Synthetic DNG'),(50710,1,[0,1,2]),(50711,3,[1]),(50713,3,[1,1]),(50714,5,[(64,1)]),(50717,4,[65535]),(50718,5,[(1,1),(1,1)]),(50719,4,[0,0]),(50720,4,[w,h]),(50721,10,[(32406,10000),(-15372,10000),(-4986,10000),(-9689,10000),(18758,10000),(415,10000),(557,10000),(-2040,10000),(10570,10000)]),(50727,5,[(1,1)]*3),(50728,5,[(1,1)]*3),(50730,10,[(0,1)]),(50778,3,[21]),(50829,4,[0,0,h,w])]: add(tag,typ,v)
tags.sort();start=8+2+12*len(tags)+4
extra=bytearray();entries=[]
for tag,typ,count,data in tags:
    if len(data)<=4:value=data.ljust(4,b'\0')
    else:
        value=struct.pack('<I',start+len(extra));extra.extend(data)
        if len(extra)%2:extra.append(0)
    entries.append((tag,typ,count,value))
image_offset=start+len(extra)
entries=[(tag,typ,count,struct.pack('<I',image_offset) if tag==273 else value) for tag,typ,count,value in entries]
header=b'II*\0'+struct.pack('<I',8)+struct.pack('<H',len(entries))+b''.join(struct.pack('<HHI',t,y,n)+v for t,y,n,v in entries)+b'\0'*4+extra
pixels=bytearray()
for y in range(h):
    for x in range(w):
        c=[0,1,1,2][(y%2)*2+x%2]
        rgb=[.1+.65*x/(w-1),.1+.65*y/(h-1),.2+.4*(x+y)/(w+h-2)]
        pixels.extend(struct.pack('<H',round(64+rgb[c]*(65535-64))))
path=folder/'07-bayer.dng';path.write_bytes(header+pixels)
manifest=json.loads((folder/'manifest.json').read_text())
manifest=[i for i in manifest if i['file']!=path.name]
manifest.append({'file':path.name,'width':w,'height':h,'source_bits':16,'orientation':1,'generator':'own uncompressed Bayer RGGB DNG; no embedded preview'})
(folder/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Generated own Bayer DNG:',path)
