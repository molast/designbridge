#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

usage() {
  cat <<'EOF'
用法：
  ./release.sh -m "更新内容"
  ./release.sh -v v0.1.1 -m "更新内容"
  ./release.sh -v v0.1.1 -m "更新内容" --no-push

选项：
  -v, --version VERSION  发布版本，例如 v0.1.1；首次省略时使用项目当前版本，之后自动递增 patch
  -m, --message TEXT     GitHub Release 更新内容，必填
      --no-push          只更新版本、提交并创建 tag，不推送远程
  -h, --help             显示帮助
EOF
}

version=""
message=""
push_release=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    -v|--version)
      [[ $# -ge 2 ]] || { echo "缺少版本号参数。" >&2; exit 1; }
      version="$2"
      shift 2
      ;;
    -m|--message)
      [[ $# -ge 2 ]] || { echo "缺少更新内容参数。" >&2; exit 1; }
      message="$2"
      shift 2
      ;;
    --no-push)
      push_release=false
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "未知参数：$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

[[ -n "$message" ]] || { echo "必须通过 -m 提供发布更新内容。" >&2; exit 1; }

if [[ -n "$(git status --porcelain)" ]]; then
  echo "工作区不干净，请先提交或暂存当前修改后再发布。" >&2
  exit 1
fi

latest_remote_tag="$(git ls-remote --tags --refs origin 'refs/tags/v*' 2>/dev/null || true)"
latest_remote_version="$(printf '%s\n' "$latest_remote_tag" | python3 -c '
import re
import sys

versions = []
for line in sys.stdin:
    match = re.search(r"refs/tags/(v(\d+)\.(\d+)\.(\d+))$", line.strip())
    if match:
        versions.append((tuple(map(int, match.groups()[1:])), match.group(1)))
print(max(versions)[1] if versions else "")
')"

if [[ -z "$version" ]]; then
  if [[ -n "$latest_remote_version" ]]; then
    version="$latest_remote_version"
    echo "已读取 GitHub 最新 tag：$version"
  else
    version="$(git tag --list 'v*' --sort=-v:refname | head -n 1)"
  fi
  if [[ -z "$version" ]]; then
    version="v$(node -p 'require("./package.json").version')"
    echo "未找到远程或本地 tag，将使用项目当前版本创建初始 tag：$version"
  elif [[ "$version" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    version="v${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.$((BASH_REMATCH[3] + 1))"
  else
    echo "无法从本地 tag 推导版本号：$version" >&2
    exit 1
  fi
fi

if [[ ! "$version" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "版本号必须是 vX.Y.Z 格式，例如 v0.1.1。" >&2
  exit 1
fi

version_number="${version#v}"
if git rev-parse "$version" >/dev/null 2>&1; then
  echo "tag 已存在：$version" >&2
  exit 1
fi
if git ls-remote --exit-code --tags origin "refs/tags/$version" >/dev/null 2>&1; then
  echo "远程 tag 已存在：$version" >&2
  exit 1
fi

python3 - "$version_number" <<'PY'
import json
import pathlib
import sys

version = sys.argv[1]

package_path = pathlib.Path("package.json")
package = json.loads(package_path.read_text())
package["version"] = version
package_path.write_text(json.dumps(package, ensure_ascii=False, indent=2) + "\n")

for path in (pathlib.Path("src-tauri/tauri.conf.json"),):
    config = json.loads(path.read_text())
    config["version"] = version
    path.write_text(json.dumps(config, ensure_ascii=False, indent=2) + "\n")
PY

VERSION="$version_number" python3 - <<'PY'
import os
import pathlib
import re

version = os.environ["VERSION"]
for filename in ("src-tauri/Cargo.toml", "src-tauri/Cargo.lock"):
    path = pathlib.Path(filename)
    text = path.read_text()
    pattern = r'(\[\[package\]\]\nname = "designbridge"\nversion = ")[^"]+(" )' if filename.endswith(".lock") else r'(^version = ")[^"]+("$)'
    if filename.endswith(".lock"):
        updated, count = re.subn(r'(\[\[package\]\]\nname = "designbridge"\nversion = ")[^"]+("\n)', rf'\g<1>{version}\g<2>', text, count=1, flags=re.MULTILINE)
    else:
        updated, count = re.subn(pattern, rf'\g<1>{version}\g<2>', text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"无法更新 {filename} 中的 designbridge 版本号")
    path.write_text(updated)
PY

git add package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "chore(release): $version"
git tag -a "$version" -m "$message"

echo "已创建 ${version}。"
if [[ "$push_release" == true ]]; then
  branch="$(git symbolic-ref --short HEAD)"
  git push origin "$branch" "$version"
  echo "已推送 $version，GitHub Actions 将开始构建 macOS Apple Silicon DMG。"
else
  echo "未推送远程。需要发布时执行：git push origin HEAD $version"
fi
