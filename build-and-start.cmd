@echo off
rem Closes a running Gaia, builds Gaia Desktop (release, no installer) and starts it.
cd /d "%~dp0"

tasklist /fi "imagename eq gaia.exe" 2>nul | find /i "gaia.exe" >nul
if not errorlevel 1 (
  echo Gaia is running - closing it...
  taskkill /im gaia.exe >nul 2>&1
  timeout /t 3 /nobreak >nul
  tasklist /fi "imagename eq gaia.exe" 2>nul | find /i "gaia.exe" >nul
  if not errorlevel 1 (
    echo Still running - forcing it closed...
    taskkill /f /im gaia.exe >nul 2>&1
    timeout /t 1 /nobreak >nul
  )
)

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
