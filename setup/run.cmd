@echo off
setlocal
set "EXE=%~dp0..\target\release\win_app_delete_controller.exe"
if not exist "%EXE%" set "EXE=%~dp0win_app_delete_controller.exe"
if not exist "%EXE%" (
  echo Cannot find win_app_delete_controller.exe.
  echo Run: cargo build --release
  pause
  exit /b 1
)
start "" "%EXE%"
