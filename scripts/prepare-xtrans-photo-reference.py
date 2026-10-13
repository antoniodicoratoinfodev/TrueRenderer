#!/usr/bin/env python3
"""Prepare fixed remosaic references from hash-pinned public RGB images.

Offline research only; needs Pillow, not added to the product.
Source images are read-only. Linearized RGB is a reconstruction surrogate,
not camera/spectral ground truth or a colorimetric calibration.
"""
import argparse
from array import array
import hashlib
import io
import json
from pathlib import Path
import sys

from PIL import Image, __version__ as pillow_version


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-manifest", type=Path,
                        help="JSON with photographs, filenames relative to its directory, SHA-256 and source rights")
    parser.add_argument("--candidate", choices=("DR4", "DR5", "DR6"), default="DR4")
    parser.add_argument("--role", default="held-out photographic surrogate")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    if not output.is_relative_to(root / "var"):
        parser.error("references must be kept under the private var/ directory")
    source_manifest = (args.source_manifest or root / "sample-photos/manifest.json").resolve()
    source_bytes = source_manifest.read_bytes()
    sources = json.loads(source_bytes)["photographs"]
    if not isinstance(sources, list) or not 1 <= len(sources) <= 512:
        raise ValueError("one to 512 source images required")
    if len({item["sha256"] for item in sources}) != len(sources):
        raise ValueError("duplicate source images")
    output.mkdir(parents=True, exist_ok=False)
    width, height = 224, 176
    result = {
        "protocol": "TRExp-photo-remosaic-v1",
        "source_manifest_sha256": digest(source_bytes),
        "candidate": args.candidate,
        "role": args.role,
        "pillow_version": pillow_version,
        "phases": 36,
        "border": 16,
        "noise_sigma": [0.0, 0.012],
        "seed": 39127651,
        "other_category_and_detail_rmse_max_ratio": 1.05,
        "absolute_rmse_floor": 0.000001,
        "sampling": "nine fixed 3x3 grid centers per source, native pixels, no resizing",
        "reference": "Encoded RGB divided by 255, mathematical inverse sRGB transfer, fp32 LE RGB",
        "limits": [
            "Already rendered RGB with prior reconstruction and tone rendering; not native sensor data.",
            "Assumed sRGB transfer is a declared surrogate transform, not measured sensor response.",
            "Reconstruction comparison only; no absolute camera color or noise calibration.",
            "All photographs reserved for verification after freezing the candidate.",
        ],
        "sources": [],
        "tiles": [],
    }
    # Exact byte-domain LUT; double arithmetic followed by one fp32 rounding.
    linear_lut = []
    for code in range(256):
        encoded = code / 255.0
        linear_lut.append(encoded / 12.92 if encoded <= 0.04045 else ((encoded + 0.055) / 1.055) ** 2.4)
    for source_id, item in enumerate(sources):
        path = source_manifest.parent / item["filename"]
        before = path.read_bytes()
        if digest(before) != item["sha256"]:
            raise ValueError(f"source hash changed: {path.name}")
        with Image.open(io.BytesIO(before)) as opened:
            image = opened.convert("RGB")
        sw, sh = image.size
        if sw < width or sh < height:
            raise ValueError("source too small for fixed crops")
        result["sources"].append({**item, "id": source_id, "width": sw, "height": sh})
        coordinates = set()
        for gy in range(3):
            for gx in range(3):
                x = min(max((2 * gx + 1) * sw // 6 - width // 2, 0), sw - width)
                y = min(max((2 * gy + 1) * sh // 6 - height // 2, 0), sh - height)
                if (x, y) in coordinates:
                    raise ValueError("source too small for nine distinct fixed crops")
                coordinates.add((x, y))
                encoded = image.crop((x, y, x + width, y + height)).tobytes()
                linear = array("f", (linear_lut[code] for code in encoded))
                if linear.itemsize != 4:
                    raise ValueError("platform does not provide 32-bit array floats")
                if sys.byteorder != "little":
                    linear.byteswap()
                data = linear.tobytes()
                name = f"photo-{source_id}-tile-{gy * 3 + gx}.rgbf32"
                (output / name).write_bytes(data)
                result["tiles"].append({
                    "file": name, "source_id": source_id, "x": x, "y": y,
                    "width": width, "height": height, "sha256": digest(data),
                })
        if digest(path.read_bytes()) != item["sha256"]:
            raise ValueError("source changed during reference preparation")
    (output / "manifest.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"Prepared {len(result['tiles'])} fixed photographic tiles; sources unchanged")


if __name__ == "__main__":
    main()
