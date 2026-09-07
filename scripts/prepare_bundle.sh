#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_DIR="$ROOT_DIR/src-tauri"
STAGE_DIR="$TAURI_DIR/generated-resources/designbridge-installer"
PLUGIN_STAGE="$STAGE_DIR/plugins/designbridge"

rm -rf "$STAGE_DIR"
mkdir -p "$PLUGIN_STAGE" "$STAGE_DIR/scripts"
cp -R "$ROOT_DIR/plugins/designbridge/." "$PLUGIN_STAGE/"
cp "$ROOT_DIR/install-plugin.sh" "$STAGE_DIR/install-plugin.sh"
cp "$ROOT_DIR/scripts/install_personal_plugin.mjs" "$STAGE_DIR/scripts/install_personal_plugin.mjs"

echo "正在构建随客户端发布的 DesignBridge MCP..."
cargo build --release \
  --manifest-path "$TAURI_DIR/Cargo.toml" \
  --bin designbridge-mcp

mkdir -p "$PLUGIN_STAGE/bin"
cp "$TAURI_DIR/target/release/designbridge-mcp" "$PLUGIN_STAGE/bin/designbridge-mcp"
chmod +x "$PLUGIN_STAGE/bin/designbridge-mcp" "$PLUGIN_STAGE/scripts/mcp.sh"

echo "DesignBridge 插件和 MCP 已准备到客户端资源目录。"
