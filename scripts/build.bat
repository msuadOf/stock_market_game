@echo off
setlocal
if not "%~1"=="server" goto ui
call "%~dp0server-build.bat" %*
exit /b %errorlevel%
:ui
if "%~1"=="" goto help
if "%~1"=="--help" goto help_argument
if "%~1"=="-h" goto help_argument
node "%~dp0build-targets.mjs" %*
exit /b %errorlevel%
:help_argument
if not "%~2"=="" (
  echo [build] help does not accept additional arguments. 1^>^&2
  exit /b 1
)
goto help
:help
echo Usage: scripts\build.bat ^<desktop^|webui^|webui-server^|server^> [--jobs N] [--dry-run] [--output target/build-artifacts/NAME]
echo Server requires Rust and Windows PowerShell, not Node. UI builds require Node, Corepack pnpm, Rust and wasm-pack.
echo Desktop builds Windows-native Tauri MSI/NSIS installers, not Linux/macOS packages.
echo Shared deadline: 300000ms. Default existing outputs are refused; use --output for a fresh repeat build.
echo Compilation does not run full regression. CI regression gates are unchanged.
exit /b 0
