@echo off
setlocal
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0server-build.ps1" %*
exit /b %errorlevel%
