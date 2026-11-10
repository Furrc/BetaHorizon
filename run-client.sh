#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

: "${OLDMINE_RENDER_DEBUG:=1}"
export OLDMINE_RENDER_DEBUG

exec cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" -p oldmine-client -- "$@"
