@echo off
chcp 65001 >nul
setlocal
node "%~dp0frontend-build.mjs" --package-manager corepack %*
exit /b %ERRORLEVEL%
