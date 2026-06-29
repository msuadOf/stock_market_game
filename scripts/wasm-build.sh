#!/usr/bin/env bash
# =====================================================================
# wasm-build.sh —— 只构建 WASM + 前端（纯前端单机版，Linux / macOS）
#
# 不需要后端(server)/桌面(Tauri)。适合快速出一份可在浏览器单机运行的产物。
# 顺序：
#   1. wasm-pack build apps/web-wasm --target web --release  （nightly）
#   2. cp pkg/* -> apps/web/wasm-pkg/
#   3. pnpm install && pnpm --filter web build
#
# 任意一步失败即退出（set -e）。
# =====================================================================
set -euo pipefail

cd "$(dirname "$0")/.."
echo "[wasm-build] 工作目录: $(pwd)"
echo

# ---------------------------------------------------------------------
# 1) WASM 构建（nightly toolchain，web target，release）
# ---------------------------------------------------------------------
echo "[1/3] wasm-pack build apps/web-wasm --target web --release"
RUSTUP_TOOLCHAIN=nightly wasm-pack build apps/web-wasm --target web --release
echo

# ---------------------------------------------------------------------
# 2) 把 wasm 产物 cp 到前端消费目录
# ---------------------------------------------------------------------
echo "[2/3] cp apps/web-wasm/pkg/* -> apps/web/wasm-pkg/"
mkdir -p apps/web/wasm-pkg
cp -r apps/web-wasm/pkg/* apps/web/wasm-pkg/
echo

# ---------------------------------------------------------------------
# 3) 前端依赖 + 构建
# ---------------------------------------------------------------------
echo "[3/3] pnpm install && pnpm --filter web build"
pnpm install
pnpm --filter web build
echo

echo "==============================================="
echo "[wasm-build] WASM + 前端构建成功完成！"
echo "==============================================="
