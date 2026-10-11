#!/usr/bin/env python3
"""Own 4x4 DNG/SOF3 conformance vectors; no third-party code or photographs.

Reproduce crates/tr-raw/tests/fixtures with --output <directory>.
Category-16 conventions follow DNG 1.7.1 Appendix A, compatibility issue 2.
The predictor-4 vector requires 13-bit residuals for valid 12-bit samples.
"""
import argparse
import hashlib
from pathlib import Path
import struct


def fixture(precision, legacy, predictor, samples):
    def segment(marker, payload):
        return bytes([255, marker]) + struct.pack(">H", len(payload) + 2) + payload

    # Canonical five-bit Huffman codes for categories 0..16.
    data = b"\xff\xd8" + segment(
        0xc4, bytes([0]) + bytes([0, 0, 0, 0, 17] + [0] * 11) + bytes(range(17))
    )
    data += segment(0xc3, struct.pack(">BHHBBBB", precision, 4, 4, 1, 1, 0x11, 0))
    data += segment(0xda, bytes([1, 1, 0, predictor, 0, 0]))
    bits = ""
    for i, value in enumerate(samples):
        x, y = i % 4, i // 4
        if i == 0:
            predicted = 1 << (precision - 1)
        elif y == 0:
            predicted = samples[i - 1]
        elif x == 0:
            predicted = samples[i - 4]
        elif predictor == 1:
            predicted = samples[i - 1]
        else:
            predicted = samples[i - 1] + samples[i - 4] - samples[i - 5]
        delta = (value - predicted + 32768) % 65536 - 32768
        category = abs(delta).bit_length()
        bits += format(category, "05b")
        if category and (category != 16 or legacy):
            magnitude = delta if delta >= 0 else delta + (1 << category) - 1
            bits += format(magnitude, f"0{category}b")
    bits += "1" * ((-len(bits)) % 8)
    entropy = bytes(int(bits[i:i + 8], 2) for i in range(0, len(bits), 8))
    data += entropy.replace(b"\xff", b"\xff\0") + b"\xff\xd9"
    tags = {}

    def integers(tag, kind, values):
        tags[tag] = (kind, len(values), struct.pack(
            "<" + {1: "B", 3: "H", 4: "I"}[kind] * len(values), *values))

    def rational(tag, values, signed=False):
        tags[tag] = (10 if signed else 5, len(values), b"".join(
            struct.pack("<ii" if signed else "<II", a, b) for a, b in values))

    for tag, kind, values in [
        (254, 4, [0]), (256, 4, [4]), (257, 4, [4]), (258, 3, [precision]),
        (259, 3, [7]), (262, 3, [32803]), (274, 3, [1]), (277, 3, [1]),
        (278, 4, [4]), (273, 4, [0]), (279, 4, [len(data)]),
        (33421, 3, [2, 2]), (33422, 1, [0, 1, 1, 2]),
        (50706, 1, [1, 0, 0, 0] if legacy else [1, 4, 0, 0]),
        (50707, 1, [1, 0, 0, 0] if legacy else [1, 1, 0, 0]),
        (50717, 4, [(1 << precision) - 1]), (50778, 3, [21]),
    ]:
        integers(tag, kind, values)
    model = b"TrueRenderer Synthetic\0"
    tags[50708] = (2, len(model), model)
    rational(50721, [(int(i % 4 == 0), 1) for i in range(9)], True)
    rational(50728, [(950456, 1000000), (1, 1), (1089058, 1000000)])
    start = 8 + 2 + len(tags) * 12 + 4
    extra, entries = bytearray(), bytearray()
    for tag, (kind, count, payload) in sorted(tags.items()):
        if len(payload) > 4:
            pointer = struct.pack("<I", start + len(extra))
            extra += payload
            extra += b"\0" * (len(extra) % 2)
        else:
            pointer = payload.ljust(4, b"\0")
        entries += struct.pack("<HHI", tag, kind, count) + pointer
    offset = next(i for i in range(0, len(entries), 12)
                  if struct.unpack_from("<H", entries, i)[0] == 273)
    entries[offset + 8:offset + 12] = struct.pack("<I", start + len(extra))
    return (b"II*\0" + struct.pack("<IH", 8, len(tags)) + entries
            + b"\0" * 4 + extra + data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    samples = [0, 65535, 32767, 32768, 300, 900, 2100, 1500,
               500, 550, 700, 600, 800, 1000, 2000, 1800]
    cases = [
        ("jpeg-modern16", 16, False, 1, samples),
        ("jpeg-dng10-16", 16, True, 1, samples),
        ("jpeg-predictor4-12", 12, False, 4,
         [4000, 0, 4000, 0, 0, 4000, 0, 4000, 4000, 0, 4000, 0, 0, 4000, 0, 4000]),
    ]
    for name, *parameters in cases:
        data = fixture(*parameters)
        (args.output / f"{name}.dng").write_bytes(data)
        print(name, hashlib.sha256(data).hexdigest())


if __name__ == "__main__":
    main()
