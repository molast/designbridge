#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! command -v npm >/dev/null 2>&1; then
  echo "未找到 npm，请先安装 Node.js（建议使用 Node.js 18 或更高版本）。" >&2
  exit 1
fi

if [[ ! -x node_modules/.bin/tauri || ! -x node_modules/.bin/vite ]]; then
  echo "正在安装项目依赖..."
  npm install
fi

exec npm run tauri dev -- "$@"
