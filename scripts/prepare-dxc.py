#!/usr/bin/env python3
"""Prepare the pinned Microsoft x64 shader compiler in an app's runtime/dxc.

This is a build/package step, never an application startup download. --archive
supports an offline build using the identical verified upstream release.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import urllib.request
import zipfile

VERSION = "1.8.2505.1"
URL = ("https://github.com/microsoft/DirectXShaderCompiler/releases/download/"
       "v1.8.2505.1/dxc_2025_07_14.zip")
SHA256 = "9ad895a6b039e3a8f8c22a1009f866800b840a74b50db9218d13319e215ea8a4"
FILES = {
    "bin/x64/dxcompiler.dll": "dxcompiler.dll",
    "bin/x64/dxil.dll": "dxil.dll",
    "LICENSE-MIT.txt": "LICENSE-MIT.txt",
    "LICENSE-LLVM.txt": "LICENSE-LLVM.txt",
    "LICENSE-MS.txt": "LICENSE-MS.txt",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True,
                        help="Destination runtime/dxc directory in the package")
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    if args.archive:
        archive = args.archive.read_bytes()
    else:
        with urllib.request.urlopen(URL, timeout=60) as response:
            archive = response.read(40 * 1024 * 1024)
    if hashlib.sha256(archive).hexdigest() != SHA256:
        raise SystemExit("DXC archive SHA-256 mismatch; package unchanged")
    with zipfile.ZipFile(io.BytesIO(archive)) as source:
        files = {name: source.read(member) for member, name in FILES.items()}
    # Select explicit members; no archive paths are extracted onto the disk.
    args.output.mkdir(parents=True, exist_ok=True)
    for name, data in files.items():
        (args.output / name).write_bytes(data)
    manifest = {"component": "Microsoft DirectX Shader Compiler", "version": VERSION,
                "architecture": "x64", "url": URL, "archive_sha256": SHA256,
                "files": {name: hashlib.sha256(data).hexdigest()
                          for name, data in files.items()}}
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n",
                                               encoding="utf-8")
    print(f"Prepared DXC {VERSION}, {len(files)} verified files in {args.output}")


if __name__ == "__main__":
    main()
