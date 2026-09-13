@echo off
setlocal
node "%~dp0build-matrix.mjs" %*
exit /b %errorlevel%
