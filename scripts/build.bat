@echo off
chcp 65001 >nul
REM =====================================================================
REM build.bat —— 一键编译打包全部（Windows）
REM
REM 顺序：
REM   1. cargo test -p engine                 （铁律一：测试先行）
REM   2. cargo clippy -p engine               （-D warnings 零警告）
REM   3. wasm-pack build apps/web-wasm         （nightly + wasm-bindgen-rayon）
REM   4. copy wasm pkg -> apps/web/wasm-pkg/   （前端消费 WASM 产物）
REM   5. pnpm install                          （前端依赖）
REM   6. pnpm --filter web build               （Vite 打包前端）
REM   7. cargo build -p server --release       （Axum 后端）
REM   8. cargo build -p stock-market-game --release（Tauri 桌面）
REM
REM 任意一步失败即退出（errorlevel 1）。在仓库根目录运行：scripts\build.bat
REM =====================================================================
setlocal enabledelayedexpansion

REM 切到仓库根（脚本在 scripts/ 下，取其父目录）。
cd /d "%~dp0.."
echo [build] 工作目录: %CD%
echo.

REM ---------------------------------------------------------------------
REM 1) engine 单元测试
REM ---------------------------------------------------------------------
echo [1/8] cargo test -p engine
cargo test -p engine
if errorlevel 1 (
    echo [ERROR] 第 1 步失败：engine 测试未通过
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 2) engine clippy（警告即错误）
REM ---------------------------------------------------------------------
echo [2/8] cargo clippy -p engine --all-targets -- -D warnings
cargo clippy -p engine --all-targets -- -D warnings
if errorlevel 1 (
    echo [ERROR] 第 2 步失败：engine clippy 有警告/错误
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 3) WASM 构建（nightly toolchain，web target，release）
REM ---------------------------------------------------------------------
echo [3/8] wasm-pack build apps/web-wasm --target web --release
set "RUSTUP_TOOLCHAIN=nightly"
wasm-pack build apps/web-wasm --target web --release
if errorlevel 1 (
    echo [ERROR] 第 3 步失败：wasm-pack 构建失败
    exit /b 1
)
set "RUSTUP_TOOLCHAIN="
echo.

REM ---------------------------------------------------------------------
REM 4) 把 wasm 产物 copy 到前端消费目录
REM ---------------------------------------------------------------------
echo [4/8] copy apps\web-wasm\pkg -> apps\web\wasm-pkg
if not exist "apps\web\wasm-pkg" mkdir "apps\web\wasm-pkg"
xcopy /e /y /i /q "apps\web-wasm\pkg\*" "apps\web\wasm-pkg\" >nul
if errorlevel 1 (
    echo [ERROR] 第 4 步失败：复制 wasm 产物失败
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 5) 前端依赖安装
REM ---------------------------------------------------------------------
echo [5/8] pnpm install
call pnpm install
if errorlevel 1 (
    echo [ERROR] 第 5 步失败：pnpm install 失败
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 6) 前端构建（tsc -b + vite build）
REM ---------------------------------------------------------------------
echo [6/8] pnpm --filter web build
call pnpm --filter web build
if errorlevel 1 (
    echo [ERROR] 第 6 步失败：前端 build 失败
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 7) 后端构建（release）
REM ---------------------------------------------------------------------
echo [7/8] cargo build -p server --release
cargo build -p server --release
if errorlevel 1 (
    echo [ERROR] 第 7 步失败：server 构建失败
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 8) Tauri 桌面构建（release，crate 名 stock-market-game）
REM ---------------------------------------------------------------------
echo [8/8] cargo build -p stock-market-game --release
cargo build -p stock-market-game --release
if errorlevel 1 (
    echo [ERROR] 第 8 步失败：Tauri(stock-market-game) 构建失败
    exit /b 1
)
echo.

echo ===============================================
echo [build] 全部步骤成功完成！
echo ===============================================
exit /b 0
