#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ ! -d "$TR_ROOT/dist/TrueRenderer.app" ]; then
  "$TR_ROOT/scripts/build-macos.sh"
fi
exec /usr/bin/open "$TR_ROOT/dist/TrueRenderer.app"
