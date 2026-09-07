#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/install-plugin.sh"

printf '\n安装完成。按回车键关闭此窗口。'
if [[ -t 0 ]]; then
  read -r _
fi
