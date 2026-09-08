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

bundle_dir="$SCRIPT_DIR/src-tauri/target/release/bundle"
for artifact_dir in "$bundle_dir/macos" "$bundle_dir/dmg"; do
  if [[ -d "$artifact_dir" ]]; then
    find "$artifact_dir" -maxdepth 1 -type f \( \
      -name 'DesignBridge_*.dmg' -o \
      -name 'rw.*.DesignBridge_*.dmg' \
    \) -delete
  fi
done

echo "正在打包 DesignBridge 正式版..."
pnpm tauri build --no-sign "$@"

if [[ -d "$bundle_dir" ]]; then
  echo
  echo "打包完成，产物目录：$bundle_dir"
  find "$bundle_dir" -maxdepth 3 \( \
    -type d -name '*.app' -o \
    -type f \( -name '*.dmg' -o -name '*.deb' -o -name '*.AppImage' -o -name '*.msi' -o -name '*.exe' \) \
  \) -print
fi
