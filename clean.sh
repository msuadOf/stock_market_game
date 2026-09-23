#!/usr/bin/env bash
# =====================================================================
# clean.sh —— 清除本仓库的构建产物、依赖与本地缓存（Linux / macOS）
#
# 用法：
#   ./clean.sh            直接清理
#   ./clean.sh --dry-run  只列出将清理的项目
#
# 不会使用 git clean，也不会删除源码、Git 数据、存档或环境变量文件。
# =====================================================================
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$repo_root"

dry_run=false
case "${1:-}" in
  "") ;;
  --dry-run) dry_run=true ;;
  *)
    echo "[ERROR] 未知参数: $1" >&2
    echo "用法: ./clean.sh [--dry-run]" >&2
    exit 2
    ;;
esac

git_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "[ERROR] 当前目录不是 Git 仓库，已停止清理。" >&2
  exit 1
}
if [[ "$git_root" != "$repo_root" ]]; then
  echo "[ERROR] 脚本必须位于仓库根目录，已停止清理。" >&2
  echo "[ERROR] Git 根目录: $git_root" >&2
  echo "[ERROR] 当前目录: $repo_root" >&2
  exit 1
fi

remove_dir() {
  local path="$1"
  [[ -e "$path" ]] || return 0

  if "$dry_run"; then
    echo "[would remove] $path"
  else
    echo "[remove] $path"
    rm -rf -- "$path"
  fi
}

remove_file() {
  local path="$1"
  if "$dry_run"; then
    echo "[would remove] $path"
  else
    echo "[remove] $path"
    rm -f -- "$path"
  fi
}

if "$dry_run"; then
  echo "[clean] 预览模式：不会删除任何文件。"
else
  echo "[clean] 工作目录: $repo_root"
fi
echo

directories=(
  target
  dist
  build
  out
  pkg
  wasm-pack-output
  node_modules
  .pnpm-store
  .cache
  .parcel-cache
  .turbo
  coverage
  .nyc_output
  apps/web/dist
  apps/web/build
  apps/web/node_modules
  apps/web/wasm-pkg
  apps/web-wasm/pkg
  apps/desktop/src-tauri/target
  apps/desktop/src-tauri/gen/schemas
)

for directory in "${directories[@]}"; do
  remove_dir "$directory"
done

while IFS= read -r -d '' tsbuildinfo; do
  remove_file "$tsbuildinfo"
done < <(find . \
  -path './node_modules' -prune -o \
  -path './apps/*/node_modules' -prune -o \
  -type f -name '*.tsbuildinfo' -print0)

echo
if "$dry_run"; then
  echo "[clean] 预览完成。运行 ./clean.sh 可执行清理。"
else
  echo "[clean] 清理完成。"
fi
