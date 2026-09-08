#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v pnpm >/dev/null 2>&1; then
  echo "未找到 pnpm，请先安装 pnpm。" >&2
  exit 1
fi

if [[ ! -x node_modules/.bin/tauri || ! -x node_modules/.bin/vite ]]; then
  echo "正在安装项目依赖..."
  pnpm install
fi

if [[ "$(uname -s)" == "Darwin" ]]; then
  DEV_EXTENSION_DIR="${HOME}/Library/Application Support/com.designbridge.app/browser-extension"
  DEV_BROWSER_HOST_DIR="${HOME}/Library/Application Support/com.designbridge.app/browser-host"
  mkdir -p "$DEV_EXTENSION_DIR"
  cp -R "$SCRIPT_DIR/browser-extension/." "$DEV_EXTENSION_DIR/"

  echo "正在构建浏览器扩展通信程序..."
  cargo build \
    --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" \
    --bin designbridge-browser-host
  mkdir -p "$DEV_BROWSER_HOST_DIR"
  DEV_BROWSER_HOST_TMP="$DEV_BROWSER_HOST_DIR/.designbridge-browser-host.$$"
  cp "$SCRIPT_DIR/src-tauri/target/debug/designbridge-browser-host" \
    "$DEV_BROWSER_HOST_TMP"
  chmod +x "$DEV_BROWSER_HOST_TMP"
  mv -f "$DEV_BROWSER_HOST_TMP" "$DEV_BROWSER_HOST_DIR/designbridge-browser-host"
  echo "浏览器扩展源码和通信程序已同步到开发安装目录。"
fi

exec pnpm tauri dev "$@"
