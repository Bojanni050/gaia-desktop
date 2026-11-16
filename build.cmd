@echo off
rem Builds Gaia Desktop (release) and stops there.
rem   build.cmd            - exe only, no installer (fast)
rem   build.cmd installer  - full bundle: MSI, NSIS setup, updater artifacts
rem A running Gaia is closed first: the linker cannot overwrite a locked gaia.exe.
setlocal
cd /d "%~dp0"

set "NOINSTALL=1"
if /i "%~1"=="installer" set "NOINSTALL="

set "EXE=%~dp0src-tauri\target\release\gaia.exe"
set "BUNDLES=%~dp0src-tauri\target\release\bundle"

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
if defined NOINSTALL goto exe_only
echo Including installer - this takes longer...
call npm run build:desktop
goto built

:exe_only
call npm run build:desktop -- --no-bundle

:built
if errorlevel 1 (
  echo.
  echo Build failed.
  pause
  exit /b 1
)

echo.
echo Build finished: %EXE%
if not defined NOINSTALL echo Installers:      %BUNDLES%
pause
