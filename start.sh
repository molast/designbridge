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

exec pnpm tauri dev "$@"
