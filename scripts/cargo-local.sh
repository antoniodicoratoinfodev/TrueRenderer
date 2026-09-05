#!/bin/sh
set -eu
TR_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -x "$TR_ROOT/.tools/cargo/bin/cargo" ]; then
  export CARGO_HOME="$TR_ROOT/.tools/cargo"
  export RUSTUP_HOME="$TR_ROOT/.tools/rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
cd "$TR_ROOT"
exec cargo "$@"
