@echo off
rem Builds Gaia Desktop (release, no installer) and stops there.
rem A running Gaia is closed first: the linker cannot overwrite a locked gaia.exe.
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
  echo Build failed.
  pause
  exit /b 1
)

echo.
echo Build finished: %~dp0src-tauri\target\release\gaia.exe
pause
