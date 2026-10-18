@echo off
rem Double-click to install OpenBark for this Windows user.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
pause
