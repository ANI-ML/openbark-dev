@echo off
rem Double-click to uninstall OpenBark. Your encounters and models are kept unless you choose to delete them.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %*
pause
