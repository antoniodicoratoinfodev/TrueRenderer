#!/usr/bin/env python3
"""Verify the pinned upstream copy and the native build inventory, offline."""
import hashlib
import json
from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
vendor = root / "third_party/libraw"
manifest = json.loads((vendor / "manifest-truerenderer.json").read_text(encoding="utf8"))
for name, expected in manifest["files_sha256"].items():
    assert hashlib.sha256((vendor / name).read_bytes()).hexdigest() == expected, name
actual = {str(p.relative_to(vendor)).replace("\\", "/") for folder in ("src", "libraw", "internal") for p in (vendor / folder).rglob("*") if p.is_file()}
expected = {name for name in manifest["files_sha256"] if "/" in name}
assert actual == expected, (actual - expected, expected - actual)
build = (root / "crates/tr-worker/build.rs").read_text(encoding="utf8")
sources = re.search(r"const LIBRAW_SOURCES:.*?= &?\[(.*?)\];", build, re.S).group(1)
assert re.findall(r'"([^\"]+\.cpp)"', sources) == manifest["compiled_sources"]
for define in manifest["defines"]:
    assert f'build.define("{define}",' in build, define
recipe = (root / "crates/tr-core/src/decoder.rs").read_text(encoding="utf8")
assert recipe.count(f'"LibRaw-{manifest["version"]}-') == 3
assert manifest["license_election"] == "CDDL-1.0"
for source in manifest["compiled_sources"]:
    content = (vendor / source).read_text(encoding="utf8", errors="replace")
    assert not re.search(r"GNU GENERAL PUBLIC LICENSE", content, re.I), source
print(f'LibRaw {manifest["version"]}: {len(manifest["files_sha256"])} upstream hashes, {len(manifest["compiled_sources"])} build sources and 3 recipes verified')
