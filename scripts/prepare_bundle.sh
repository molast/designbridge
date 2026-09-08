#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_DIR="$ROOT_DIR/src-tauri"
STAGE_DIR="$TAURI_DIR/generated-resources/designbridge-installer"
PLUGIN_STAGE="$STAGE_DIR/plugins/designbridge"
BROWSER_EXTENSION_STAGE="$STAGE_DIR/browser-extension"
BROWSER_HOST_STAGE="$STAGE_DIR/browser-host/bin"

rm -rf "$STAGE_DIR"
mkdir -p "$PLUGIN_STAGE" "$STAGE_DIR/scripts" "$BROWSER_EXTENSION_STAGE" "$BROWSER_HOST_STAGE"
cp -R "$ROOT_DIR/plugins/designbridge/." "$PLUGIN_STAGE/"
cp -R "$ROOT_DIR/browser-extension/." "$BROWSER_EXTENSION_STAGE/"
cp "$ROOT_DIR/install-plugin.sh" "$STAGE_DIR/install-plugin.sh"
cp "$ROOT_DIR/scripts/install_personal_plugin.mjs" "$STAGE_DIR/scripts/install_personal_plugin.mjs"

echo "正在构建随客户端发布的 DesignBridge MCP..."
cargo build --release \
  --manifest-path "$TAURI_DIR/Cargo.toml" \
  --bin designbridge-mcp

mkdir -p "$PLUGIN_STAGE/bin"
cp "$TAURI_DIR/target/release/designbridge-mcp" "$PLUGIN_STAGE/bin/designbridge-mcp"
chmod +x "$PLUGIN_STAGE/bin/designbridge-mcp" "$PLUGIN_STAGE/scripts/mcp.sh"

echo "正在构建浏览器扩展通信程序..."
cargo build --release \
  --manifest-path "$TAURI_DIR/Cargo.toml" \
  --bin designbridge-browser-host

cp "$TAURI_DIR/target/release/designbridge-browser-host" "$BROWSER_HOST_STAGE/designbridge-browser-host"
chmod +x "$BROWSER_HOST_STAGE/designbridge-browser-host"

echo "DesignBridge 插件、MCP 和浏览器扩展已准备到客户端资源目录。"
