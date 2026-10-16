@echo off
rem Starts Gaia Desktop (last built release) without rebuilding.
if not exist "%~dp0src-tauri\target\release\gaia.exe" (
  echo gaia.exe not found - run build-and-start.cmd first.
  pause
  exit /b 1
)
start "" "%~dp0src-tauri\target\release\gaia.exe"
