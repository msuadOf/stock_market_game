@echo off
chcp 65001 >nul
REM =====================================================================
REM build.bat —— 一键编译打包全部（Windows）
REM
REM 顺序：
REM   1. cargo fmt + cargo test --workspace   （格式与全量 Rust 测试）
REM   2. cargo clippy --workspace             （-D warnings 零警告）
REM   3. wasm-pack build apps/web-wasm         （nightly + wasm-bindgen-rayon）
REM   4. copy wasm pkg -> apps/web/wasm-pkg/   （前端消费 WASM 产物）
REM   5. pnpm install --frozen-lockfile        （前端依赖）
REM   6. pnpm web test + lint + build           （完整前端门禁）
REM   7. cargo build -p server --release       （Axum 后端）
REM   8. cargo build -p stock-market-game --release（Tauri 桌面）
REM
REM 任意一步失败即退出（errorlevel 1）。在仓库根目录运行：scripts\build.bat
REM =====================================================================
setlocal

REM 切到仓库根（脚本在 scripts/ 下，取其父目录）。
cd /d "%~dp0.."
echo [build] 工作目录: %CD%
echo.

REM ---------------------------------------------------------------------
REM 1) Rust 格式与 workspace 全量测试
REM ---------------------------------------------------------------------
echo [1/8] cargo fmt --all --check ^&^& cargo test --workspace
cargo fmt --all --check
if errorlevel 1 (
    echo [ERROR] Rust formatting check failed
    exit /b 1
)
cargo test --workspace
if errorlevel 1 (
    echo [ERROR] Rust workspace tests failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 2) workspace clippy（警告即错误）
REM ---------------------------------------------------------------------
echo [2/8] cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
if errorlevel 1 (
    echo [ERROR] Clippy failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 3) WASM 构建（nightly toolchain，web target，release）
REM ---------------------------------------------------------------------
echo [3/8] wasm-pack build apps/web-wasm --target web --release
powershell -NoProfile -NonInteractive -Command "$env:RUSTUP_TOOLCHAIN='nightly-2026-09-05'; wasm-pack build apps/web-wasm --target web --release; exit $LASTEXITCODE"
if errorlevel 1 (
    echo [ERROR] wasm-pack build failed
    exit /b 1
)
node scripts\check-wasm-threading.mjs
if errorlevel 1 (
    echo [ERROR] WASM shared-memory contract check failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 4) 把 wasm 产物 copy 到前端消费目录
REM ---------------------------------------------------------------------
echo [4/8] copy apps\web-wasm\pkg to apps\web\wasm-pkg
if not exist "apps\web\wasm-pkg" mkdir "apps\web\wasm-pkg"
xcopy /e /y /i /q "apps\web-wasm\pkg\*" "apps\web\wasm-pkg\" >nul
if errorlevel 1 (
    echo [ERROR] WASM package copy failed
    exit /b 1
)
node scripts\check-wasm-threading.mjs apps/web/wasm-pkg/web_wasm.js
if errorlevel 1 (
    echo [ERROR] copied WASM shared-memory contract check failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 5) 前端依赖安装
REM ---------------------------------------------------------------------
echo [5/8] pnpm install --frozen-lockfile
call pnpm install --frozen-lockfile
if errorlevel 1 (
    echo [ERROR] pnpm install failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 6) 前端测试、lint 与构建
REM ---------------------------------------------------------------------
echo [6/8] pnpm --filter web test ^&^& lint ^&^& build
call pnpm --filter web test
if errorlevel 1 (
    echo [ERROR] web tests failed
    exit /b 1
)
call pnpm --filter web lint
if errorlevel 1 (
    echo [ERROR] web lint failed
    exit /b 1
)
call pnpm --filter web build
if errorlevel 1 (
    echo [ERROR] web build failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 7) 后端构建（release）
REM ---------------------------------------------------------------------
echo [7/8] cargo build -p server --release
cargo build -p server --release
if errorlevel 1 (
    echo [ERROR] server release build failed
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 8) Tauri 桌面构建（release，crate 名 stock-market-game）
REM ---------------------------------------------------------------------
echo [8/8] cargo build -p stock-market-game --release
cargo build -p stock-market-game --release
if errorlevel 1 (
    echo [ERROR] desktop release build failed
    exit /b 1
)
echo.

echo ===============================================
echo [build] 全部步骤成功完成！
echo ===============================================
exit /b 0
