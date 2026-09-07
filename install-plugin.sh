#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v node >/dev/null 2>&1; then
  echo "未找到 Node.js，无法安装 DesignBridge Codex 插件。请先运行 ./start.sh 检查开发环境。" >&2
  exit 1
fi

if ! command -v codex >/dev/null 2>&1; then
  echo "未找到 Codex 命令。请先安装 Codex，再重新运行此脚本。" >&2
  exit 1
fi

bundled_mcp_binary="$SCRIPT_DIR/plugins/designbridge/bin/designbridge-mcp"
mcp_build_dir=""
cleanup_mcp_build() {
  if [[ -n "$mcp_build_dir" && -d "$mcp_build_dir" ]]; then
    rm -rf "$mcp_build_dir"
  fi
}
trap cleanup_mcp_build EXIT

if [[ ! -x "$bundled_mcp_binary" ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "未找到 Cargo，无法准备 DesignBridge MCP 运行文件。请先安装 Rust/Cargo，再重试。" >&2
    exit 1
  fi

  echo "正在准备 DesignBridge MCP 运行文件..."
  mcp_build_dir="$(mktemp -d "${TMPDIR:-/tmp}/designbridge-mcp.XXXXXX")"
  cargo build --release \
    --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" \
    --bin designbridge-mcp \
    --target-dir "$mcp_build_dir"
  bundled_mcp_binary="$mcp_build_dir/release/designbridge-mcp"
fi

if [[ ! -x "$bundled_mcp_binary" ]]; then
  echo "无法准备 DesignBridge MCP 运行文件。" >&2
  exit 1
fi

install_output="$(DESIGNBRIDGE_MCP_BINARY="$bundled_mcp_binary" node "$SCRIPT_DIR/scripts/install_personal_plugin.mjs" "$SCRIPT_DIR")"
printf '%s\n' "$install_output"
marketplace_name="$(printf '%s\n' "$install_output" | sed -n 's/^MARKETPLACE_NAME=//p')"
plugin_dir="$(printf '%s\n' "$install_output" | sed -n 's/^PLUGIN_DIR=//p')"

if [[ -z "$marketplace_name" || -z "$plugin_dir" ]]; then
  echo "无法确定个人插件市场或安装目录。" >&2
  exit 1
fi

plugin_launcher="$plugin_dir/scripts/mcp.sh"
if [[ ! -x "$plugin_launcher" ]]; then
  echo "DesignBridge MCP 启动脚本不存在或不可执行：$plugin_launcher" >&2
  exit 1
fi

echo "正在将 DesignBridge 安装到 Codex..."
codex plugin add "designbridge@$marketplace_name" >/dev/null

# Keep an explicit user-level registration so Codex Desktop lists the server in
# Settings. The plugin's .mcp.json remains available as a portable fallback.
codex mcp remove designbridge >/dev/null 2>&1 || true
codex mcp add designbridge -- /bin/bash "$plugin_launcher" >/dev/null

mcp_config="$(codex mcp get designbridge 2>/dev/null || true)"
if [[ -z "$mcp_config" ]] || ! printf '%s\n' "$mcp_config" | grep -F -- "$plugin_launcher" >/dev/null; then
  echo "插件文件已安装，但 DesignBridge MCP 未正确注册。请再次点击安装按钮重试。" >&2
  exit 1
fi

echo "DesignBridge Codex 插件和 MCP 服务安装完成。"
echo "请完全退出并重新打开 Codex，然后新建一个任务；之后在任意业务项目中粘贴 designbridge:// 链接即可使用。"
