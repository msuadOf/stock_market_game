#!/usr/bin/env bash
# =====================================================================
# build.sh —— 一键编译打包全部（Linux / macOS）
#
# 顺序：
#   1. cargo test -p engine                 （铁律一：测试先行）
#   2. cargo clippy -p engine               （-D warnings 零警告）
#   3. wasm-pack build apps/web-wasm         （nightly + wasm-bindgen-rayon）
#   4. cp wasm pkg -> apps/web/wasm-pkg/     （前端消费 WASM 产物）
#   5. pnpm install                          （前端依赖）
#   6. pnpm --filter web build               （Vite 打包前端）
#   7. cargo build -p server --release       （Axum 后端）
#   8. cargo build -p stock-market-game --release（Tauri 桌面）
#
# 任意一步失败即退出（set -e）。在仓库根目录运行：./scripts/build.sh
#
# 备注：Linux 上构建 Tauri(步骤 8) 需要额外系统依赖，否则 link 阶段会失败：
#   sudo apt-get install -y \
#     libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
# macOS 一般开箱即用（Xcode Command Line Tools）。
# =====================================================================
set -euo pipefail

# 切到仓库根（脚本在 scripts/ 下）。
cd "$(dirname "$0")/.."
echo "[build] 工作目录: $(pwd)"
echo

# ---------------------------------------------------------------------
# 1) engine 单元测试
# ---------------------------------------------------------------------
echo "[1/8] cargo test -p engine"
cargo test -p engine
echo

# ---------------------------------------------------------------------
# 2) engine clippy（警告即错误）
# ---------------------------------------------------------------------
echo "[2/8] cargo clippy -p engine --all-targets -- -D warnings"
cargo clippy -p engine --all-targets -- -D warnings
echo

# ---------------------------------------------------------------------
# 3) WASM 构建（nightly toolchain，web target，release）
# ---------------------------------------------------------------------
echo "[3/8] wasm-pack build apps/web-wasm --target web --release"
RUSTUP_TOOLCHAIN=nightly wasm-pack build apps/web-wasm --target web --release
echo

# ---------------------------------------------------------------------
# 4) 把 wasm 产物 cp 到前端消费目录
# ---------------------------------------------------------------------
echo "[4/8] cp apps/web-wasm/pkg/* -> apps/web/wasm-pkg/"
mkdir -p apps/web/wasm-pkg
cp -r apps/web-wasm/pkg/* apps/web/wasm-pkg/
echo

# ---------------------------------------------------------------------
# 5) 前端依赖安装
# ---------------------------------------------------------------------
echo "[5/8] pnpm install"
pnpm install
echo

# ---------------------------------------------------------------------
# 6) 前端构建（tsc -b + vite build）
# ---------------------------------------------------------------------
echo "[6/8] pnpm --filter web build"
pnpm --filter web build
echo

# ---------------------------------------------------------------------
# 7) 后端构建（release）
# ---------------------------------------------------------------------
echo "[7/8] cargo build -p server --release"
cargo build -p server --release
echo

# ---------------------------------------------------------------------
# 8) Tauri 桌面构建（release，crate 名 stock-market-game）
#    Linux 下若 link 报错找不到 webkit2gtk，请先装系统依赖（见文件头备注）。
# ---------------------------------------------------------------------
echo "[8/8] cargo build -p stock-market-game --release"
cargo build -p stock-market-game --release
echo

echo "==============================================="
echo "[build] 全部步骤成功完成！"
echo "==============================================="
