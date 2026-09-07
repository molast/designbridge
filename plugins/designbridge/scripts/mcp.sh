#!/usr/bin/env bash

set -euo pipefail

PLUGIN_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
MCP_BINARY="$PLUGIN_DIR/bin/designbridge-mcp"

if [[ -x "$MCP_BINARY" ]]; then
  exec "$MCP_BINARY" "$@"
fi

if [[ -f "$PLUGIN_DIR/../../src-tauri/Cargo.toml" ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "未找到 Cargo，无法启动 DesignBridge MCP。" >&2
    exit 1
  fi
  DESIGNBRIDGE_ROOT="$(CDPATH= cd -- "$PLUGIN_DIR/../.." && pwd)"
  exec cargo run --quiet --manifest-path "$DESIGNBRIDGE_ROOT/src-tauri/Cargo.toml" --bin designbridge-mcp "$@"
fi

echo "找不到 DesignBridge MCP 运行文件。请重新在 DesignBridge 客户端点击“安装 Codex 插件”。" >&2
exit 1
