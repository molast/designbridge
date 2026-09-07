#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
echo "setup-mcp.sh 已兼容转交给个人插件安装流程。"
exec "$SCRIPT_DIR/install-plugin.sh"
