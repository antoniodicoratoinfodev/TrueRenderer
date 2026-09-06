#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TR_ROOT"
./scripts/cargo-local.sh fmt --all --check
./scripts/cargo-local.sh clippy --workspace --all-targets --locked --offline -- -D warnings
./scripts/cargo-local.sh build --workspace --locked --offline
./scripts/cargo-local.sh test --workspace --locked --offline
TR_WORKER_BINARY="$TR_ROOT/target/debug/tr-worker" ./scripts/cargo-local.sh test --workspace --locked --offline -- --ignored
python3 scripts/test-worker.py
./target/debug/truerenderer --verify-resampling
if [ "${1:-}" = "--gui" ]; then
  ./target/debug/truerenderer --sampling-smoke
  ./target/debug/truerenderer --verify-sampling-screenshots
  python3 -c 'import json; r=json.load(open("reports/smoke-macos.json")); assert r["passed"],r; print("Native smoke passed")'
fi
