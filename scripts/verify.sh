#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TR_ROOT"
if python3 -c 'import sys; assert sys.version_info.major == 3' >/dev/null 2>&1; then
  TR_PYTHON=python3
elif python -c 'import sys; assert sys.version_info.major == 3' >/dev/null 2>&1; then
  TR_PYTHON=python
else
  printf '%s\n' 'Python 3 is required (Microsoft Store execution aliases are not sufficient).' >&2
  exit 1
fi
TR_EXE=
TR_PLATFORM=linux
case "$(uname -s)" in
  Darwin) TR_PLATFORM=macos ;;
  MINGW*|MSYS*|CYGWIN*) TR_EXE=.exe; TR_PLATFORM=windows ;;
esac
# Keep each campaign separate from historical reports and durable user data.
mkdir -p "$TR_ROOT/var"
TR_VERIFY_ROOT=$(mktemp -d "$TR_ROOT/var/verify-XXXXXXXX")
cp -R "$TR_ROOT/corpus" "$TR_VERIFY_ROOT/corpus"
mkdir -p "$TR_VERIFY_ROOT/reports"
printf 'Verification artifacts: %s\n' "$TR_VERIFY_ROOT"
export TR_WORKER_BINARY="$TR_ROOT/target/debug/tr-worker$TR_EXE"
# Native Rust does not interpret MSYS /c/... paths in environment variables.
if [ -n "$TR_EXE" ]; then
  TR_WORKER_BINARY=$(cygpath -m "$TR_WORKER_BINARY")
fi
./scripts/cargo-local.sh fmt --all --check
"$TR_PYTHON" scripts/test-preview-reporting.py
./scripts/cargo-local.sh clippy --workspace --all-targets --locked --offline -- -D warnings
./scripts/cargo-local.sh build --workspace --locked --offline
./scripts/cargo-local.sh test --workspace --locked --offline
if [ -n "${TR_RAW_SAMPLE:-}" ]; then
  ./scripts/cargo-local.sh test --workspace --locked --offline -- --ignored
else
  printf '%s\n' 'TR_RAW_SAMPLE unset: skipping the three private-camera tests; generated fixtures and worker integrations still run.'
  ./scripts/cargo-local.sh test --workspace --locked --offline -- --ignored \
    --skip full_quality_delivers_the_source_at_its_own_resolution \
    --skip real_containers_deliver_their_embedded_preview_and_declare_the_stage \
    --skip real_raw_export_preserves_every_active_sample
fi
"$TR_PYTHON" scripts/test-worker.py --report "$TR_VERIFY_ROOT/reports/worker-protocol.json"
./target/debug/truerenderer --root "$TR_VERIFY_ROOT" --verify-resampling
if [ "${1:-}" = "--gui" ]; then
  ./target/debug/truerenderer --root "$TR_VERIFY_ROOT" --sampling-smoke
  ./target/debug/truerenderer --root "$TR_VERIFY_ROOT" --verify-sampling-screenshots
  "$TR_PYTHON" -c 'import json,sys; r=json.load(open(sys.argv[1])); assert r["passed"],r; print("Native smoke passed")' "$TR_VERIFY_ROOT/reports/smoke-$TR_PLATFORM.json"
fi
