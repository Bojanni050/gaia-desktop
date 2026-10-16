@echo off
rem Builds Gaia Desktop (release, no installer) and starts it.
cd /d "%~dp0"

echo Building Gaia Desktop...
call npm run build:desktop -- --no-bundle
if errorlevel 1 (
  echo.
  echo Build failed - Gaia is not started.
  pause
  exit /b 1
)

echo Starting Gaia...
start "" "%~dp0src-tauri\target\release\gaia.exe"
