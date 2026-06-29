@echo off
chcp 65001 >nul
REM =====================================================================
REM wasm-build.bat —— 只构建 WASM + 前端（纯前端单机版，Windows）
REM
REM 不需要后端(server)/桌面(Tauri)。适合快速出一份可在浏览器单机运行的产物。
REM 顺序：
REM   1. wasm-pack build apps/web-wasm --target web --release  （nightly）
REM   2. copy pkg -> apps/web/wasm-pkg/
REM   3. pnpm install && pnpm --filter web build
REM
REM 任意一步失败即退出（errorlevel 1）。
REM =====================================================================
setlocal enabledelayedexpansion

cd /d "%~dp0.."
echo [wasm-build] 工作目录: %CD%
echo.

REM ---------------------------------------------------------------------
REM 1) WASM 构建（nightly toolchain，web target，release）
REM ---------------------------------------------------------------------
echo [1/3] wasm-pack build apps/web-wasm --target web --release
set "RUSTUP_TOOLCHAIN=nightly"
wasm-pack build apps/web-wasm --target web --release
if errorlevel 1 (
    echo [ERROR] 第 1 步失败：wasm-pack 构建失败
    exit /b 1
)
set "RUSTUP_TOOLCHAIN="
echo.

REM ---------------------------------------------------------------------
REM 2) 把 wasm 产物 copy 到前端消费目录
REM ---------------------------------------------------------------------
echo [2/3] copy apps\web-wasm\pkg -> apps\web\wasm-pkg
if not exist "apps\web\wasm-pkg" mkdir "apps\web\wasm-pkg"
xcopy /e /y /i /q "apps\web-wasm\pkg\*" "apps\web\wasm-pkg\" >nul
if errorlevel 1 (
    echo [ERROR] 第 2 步失败：复制 wasm 产物失败
    exit /b 1
)
echo.

REM ---------------------------------------------------------------------
REM 3) 前端依赖 + 构建
REM ---------------------------------------------------------------------
echo [3/3] pnpm install ^&^& pnpm --filter web build
call pnpm install
if errorlevel 1 (
    echo [ERROR] 第 3 步失败：pnpm install 失败
    exit /b 1
)
call pnpm --filter web build
if errorlevel 1 (
    echo [ERROR] 第 3 步失败：前端 build 失败
    exit /b 1
)
echo.

echo ===============================================
echo [wasm-build] WASM + 前端构建成功完成！
echo ===============================================
exit /b 0
