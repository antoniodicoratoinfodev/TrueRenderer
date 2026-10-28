#!/usr/bin/env python3
"""Independent v1 document writer, using only Python's standard library."""
import argparse
import hashlib
import json
import struct
from pathlib import Path


def fixture():
    width, height = 320, 240
    recipe = dict(schema_version=1, process_version=1, raw_engine="LibRawBilinear",
                  exposure_ev=0.0, brightness=0.0, contrast=0.0, highlights=0.0,
                  shadows=0.0, whites=0.0, blacks=0.0, temperature=0.0, tint=0.0,
                  saturation=0.0, curve=[])
    layer = dict(name="Base sintetica", width=width, height=height, visible=True,
                 locked=False, opacity=1.0, blend="Normal",
                 transform=dict(x=0.0, y=0.0, scale=1.0, angle=0.0), recipe=recipe)
    copy = dict(layer, name="Copia indipendente", opacity=0.65,
                transform=dict(x=80.0, y=40.0, scale=0.6, angle=15.0))
    document = dict(version=1, width=width, height=height, ppi=300.0, unit="Pixels",
                    color_space="Srgb", layers=[layer, copy])
    manifest = json.dumps(dict(document=document, selection=False, masks=[False, False]),
                          ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    body = bytearray(b"TRDOC\0\x01\0" + struct.pack("<I", len(manifest)) + manifest)
    for second in (False, True):
        for y in range(height):
            for x in range(width):
                pixel = (x / width, y / height, 0.1, 1.0)
                if second:
                    pixel = (0.05, 0.1, 0.7, 0.8)
                body.extend(struct.pack("<4f", *pixel))
    return body + hashlib.sha256(body).digest()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("xb") as stream:
        stream.write(fixture())
