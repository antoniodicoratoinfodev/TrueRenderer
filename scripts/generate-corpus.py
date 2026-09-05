#!/usr/bin/env python3
"""Deterministic numerical images; own generated test data, no external assets."""
from pathlib import Path
import hashlib, json, math, struct, zlib

root = Path(__file__).resolve().parents[1] / "corpus"
root.mkdir(exist_ok=True)

def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)

def png(name, width, height, fn, bits=8):
    data = bytearray()
    maximum = (1 << bits) - 1
    for y in range(height):
        data.append(0)
        for x in range(width):
            rgba = fn(x / max(width-1, 1), y / max(height-1, 1), x, y)
            for v in rgba:
                v = round(max(0, min(1, v)) * maximum)
                data.extend(struct.pack(">H", v) if bits == 16 else bytes([v]))
    wire = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, bits, 6, 0, 0, 0))
    wire += chunk(b"sRGB", b"\x00") + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
    path = root / name
    path.write_bytes(wire)
    return {"file": name, "sha256": hashlib.sha256(wire).hexdigest(), "width": width, "height": height, "bits": bits, "source": "TrueRenderer deterministic numerical corpus v1", "input_color": "sRGB"}

patches = [(0.45,.32,.27),(.77,.59,.51),(.37,.48,.61),(.35,.42,.24),(.51,.50,.68),(.38,.72,.65),(.84,.40,.18),(.31,.35,.65),(.74,.30,.37),(.37,.23,.42),(.62,.74,.24),(.89,.63,.18),(.16,.24,.58),(.29,.59,.28),(.70,.20,.24),(.92,.79,.19),(.72,.33,.60),(.06,.53,.64),(.96,.96,.94),(.76,.76,.75),(.60,.60,.59),(.44,.44,.43),(.28,.28,.28),(.13,.13,.13)]
def chart(u,v,x,y):
    col, row = min(int(u*6),5), min(int(v*4),3)
    if (u*6)%1 < .06 or (u*6)%1 > .94 or (v*4)%1 < .08 or (v*4)%1 > .92: return (.11,.11,.11,1)
    return (*patches[row*6+col],1)
def rings(u,v,x,y):
    r = math.hypot((u-.5)*1.5,v-.5)
    wave = (math.sin(r*r*650)+1)/2
    return (wave,wave,wave,1)
def alpha(u,v,x,y):
    r = math.hypot((u-.5)*1.5,v-.5)
    return (.16,.48,.86,max(0,min(1,(.37-r)*8)))
def scene(u,v,x,y,variant=0):
    # Analytic layered edges to inspect banding and resampling, not a photograph.
    if v > .78: return (.20+.15*u, .28+.13*u, .33+.18*u,1)
    if v > .53+.10*math.sin(10*u)+.025*math.sin(39*u): return (.12+.10*v,.22+.12*v,.27+.12*v,1)
    if v > .40+.15*math.sin(7*u+1)+.04*math.sin(18*u): return (.26+.1*v,.36+.12*v,.43+.12*v,1)
    if math.hypot((u-.73)*1.5,v-.22) < .072: return (.96,.81,.59,1)
    return (.42+.35*v+variant, .58+.2*v, .70+.1*v,1)

manifest = [
png("01_Studio_cromatico.png", 1200, 800, chart),
png("02_Paesaggio_analitico.png", 1440, 960, scene),
png("03_Gradiente_16bit.png", 1600, 900, lambda u,v,x,y:(u, u if v<.5 else v, 1-u,1),16),
png("04_Frequenze_radiali.png", 1200, 800, rings),
png("05_Trasparenza.png", 1200, 800, alpha),
png("06_Scala_neutra.png", 1200, 800, lambda u,v,x,y: (*([int(u*15)/15 if v<.5 else u]*3),1)),
png("07_Confronto_A.png", 1440, 960, scene),
png("08_Confronto_B.png", 1440, 960, lambda u,v,x,y:scene(u,v,x,y,.035)),
png("09_Campioni_primari.png", 1200, 800, lambda u,v,x,y: ([(1,.1,.1,1),(.1,1,.1,1),(.1,.2,1,1)][min(int(u*3),2)] if v<.55 else (u,1-u,v,1))),
png("10_Bordi_dispari.png", 1001, 667, lambda u,v,x,y: ((.08,.08,.08,1) if (x//12+y//12)%2 else (.9,.9,.9,1))),
png("11_Ombre_16bit.png", 1200, 800, lambda u,v,x,y: (u*.12,u*.12,u*.12,1),16),
png("12_Luci_16bit.png", 1200, 800, lambda u,v,x,y: (.88+u*.12,.88+u*.12,.88+u*.12,1),16),
]
(root / "manifest.json").write_text(json.dumps({"schema":1,"description":"Only these byte digests are admitted to the R0 decoder. Changing the manifest requires rebuilding the app.","images":manifest}, indent=2) + "\n")
print(f"Generated {len(manifest)} test images in {root}")
