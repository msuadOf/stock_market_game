@echo off
chcp 65001 >nul
REM =====================================================================
REM clean.bat - remove build output, dependencies, and local caches (Windows)
REM
REM Usage:
REM   clean.bat            remove artifacts
REM   clean.bat --dry-run  list artifacts without removing them
REM
REM Does not use git clean and never removes source, Git data, saves, or .env files.
REM =====================================================================
setlocal EnableExtensions DisableDelayedExpansion

cd /d "%~dp0"
set "DRY_RUN=0"

if "%~1"=="" goto :validate_repo
if /I "%~1"=="--dry-run" (
    set "DRY_RUN=1"
    goto :validate_repo
)

echo [ERROR] Unknown argument: %~1
echo Usage: clean.bat [--dry-run]
exit /b 2

:validate_repo
set "REPO_ROOT="
for /f "usebackq delims=" %%R in (`git rev-parse --show-toplevel 2^>nul`) do set "REPO_ROOT=%%~fR"
if not defined REPO_ROOT (
    echo [ERROR] Current directory is not a Git repository; cleanup stopped.
    exit /b 1
)
if /I not "%REPO_ROOT%"=="%CD%" (
    echo [ERROR] Script must be located at the repository root; cleanup stopped.
    echo [ERROR] Git root: %REPO_ROOT%
    echo [ERROR] Current directory: %CD%
    exit /b 1
)

if "%DRY_RUN%"=="1" (
    echo [clean] Dry-run mode: no files will be removed.
) else (
    echo [clean] Working directory: %CD%
)
echo.

call :remove_dir "target" || exit /b 1
call :remove_dir "dist" || exit /b 1
call :remove_dir "build" || exit /b 1
call :remove_dir "out" || exit /b 1
call :remove_dir "pkg" || exit /b 1
call :remove_dir "wasm-pack-output" || exit /b 1
call :remove_dir "node_modules" || exit /b 1
call :remove_dir ".pnpm-store" || exit /b 1
call :remove_dir ".cache" || exit /b 1
call :remove_dir ".parcel-cache" || exit /b 1
call :remove_dir ".turbo" || exit /b 1
call :remove_dir "coverage" || exit /b 1
call :remove_dir ".nyc_output" || exit /b 1
call :remove_dir "apps\web\dist" || exit /b 1
call :remove_dir "apps\web\build" || exit /b 1
call :remove_dir "apps\web\node_modules" || exit /b 1
call :remove_dir "apps\web\wasm-pkg" || exit /b 1
call :remove_dir "apps\web-wasm\pkg" || exit /b 1
call :remove_dir "apps\desktop\src-tauri\target" || exit /b 1
call :remove_dir "apps\desktop\src-tauri\gen\schemas" || exit /b 1

for /r %%F in (*.tsbuildinfo) do call :remove_file "%%~fF" || exit /b 1

echo.
if "%DRY_RUN%"=="1" (
    echo [clean] Dry run complete. Run clean.bat to remove artifacts.
) else (
    echo [clean] Cleanup complete.
)
exit /b 0

:remove_dir
if not exist "%~1" exit /b 0
if "%DRY_RUN%"=="1" (
    echo [would remove] %~1
) else (
    echo [remove] %~1
    rmdir /s /q "%~1"
    if errorlevel 1 (
        echo [ERROR] Could not remove directory: %~1
        exit /b 1
    )
)
exit /b 0

:remove_file
if "%DRY_RUN%"=="1" (
    echo [would remove] %~1
) else (
    echo [remove] %~1
    del /f /q "%~1"
    if errorlevel 1 (
        echo [ERROR] Could not remove file: %~1
        exit /b 1
    )
)
exit /b 0
