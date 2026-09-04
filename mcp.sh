#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec cargo run --quiet --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" --bin designbridge-mcp
