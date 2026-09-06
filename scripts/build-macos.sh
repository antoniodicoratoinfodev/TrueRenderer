#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TR_ROOT"
./scripts/cargo-local.sh build --release --workspace --locked --offline
python3 scripts/package-macos.py --bundle "${1:-dist/TrueRenderer.app}"
