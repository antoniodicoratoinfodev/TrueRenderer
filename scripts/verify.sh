#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TR_ROOT"
./scripts/cargo-local.sh fmt --all --check
python3 scripts/test-preview-reporting.py
./scripts/cargo-local.sh clippy --workspace --all-targets --locked --offline -- -D warnings
./scripts/cargo-local.sh build --workspace --locked --offline
./scripts/cargo-local.sh test --workspace --locked --offline
if [ -n "${TR_RAW_SAMPLE:-}" ]; then
  TR_WORKER_BINARY="$TR_ROOT/target/debug/tr-worker" ./scripts/cargo-local.sh test --workspace --locked --offline -- --ignored
else
  printf '%s\n' 'TR_RAW_SAMPLE unset: skipping the three private-camera tests; generated fixtures and worker integrations still run.'
  TR_WORKER_BINARY="$TR_ROOT/target/debug/tr-worker" ./scripts/cargo-local.sh test --workspace --locked --offline -- --ignored \
    --skip full_quality_delivers_the_source_at_its_own_resolution \
    --skip real_containers_deliver_their_embedded_preview_and_declare_the_stage \
    --skip real_raw_export_preserves_every_active_sample
fi
python3 scripts/test-worker.py
./target/debug/truerenderer --verify-resampling
if [ "${1:-}" = "--gui" ]; then
  ./target/debug/truerenderer --sampling-smoke
  ./target/debug/truerenderer --verify-sampling-screenshots
  python3 -c 'import json; r=json.load(open("reports/smoke-macos.json")); assert r["passed"],r; print("Native smoke passed")'
fi
