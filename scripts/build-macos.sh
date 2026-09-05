#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TR_ROOT"
./scripts/cargo-local.sh build --release --workspace --locked --offline
TR_APP="$TR_ROOT/dist/TrueRenderer.app"
mkdir -p "$TR_APP/Contents/MacOS" "$TR_APP/Contents/Resources"
# macOS commonly uses a case-insensitive filesystem: do not keep a launcher and
# a binary whose names differ only by case.
rm -f "$TR_APP/Contents/MacOS/truerenderer" "$TR_APP/Contents/MacOS/TrueRenderer"
cp target/release/truerenderer "$TR_APP/Contents/MacOS/TrueRenderer"
cp target/release/tr-worker "$TR_APP/Contents/MacOS/tr-worker"
cp scripts/Info.plist "$TR_APP/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$TR_APP/Contents/MacOS/tr-worker"
/usr/bin/codesign --force --sign - "$TR_APP/Contents/MacOS/TrueRenderer"
/usr/bin/codesign --force --sign - "$TR_APP"
/usr/bin/codesign --verify --deep --strict "$TR_APP"
printf 'Bundle interno creato: %s\n' "$TR_APP"
