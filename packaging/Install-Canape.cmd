@echo off
rem Starts install-release.ps1 as administrator from this folder.
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -NoProfile -Command "Start-Process -Verb RunAs -FilePath powershell -ArgumentList '-NoProfile -ExecutionPolicy Bypass -NoExit -File ""%~dp0install-release.ps1""'"
    exit /b
)
powershell -NoProfile -ExecutionPolicy Bypass -NoExit -File "%~dp0install-release.ps1"
