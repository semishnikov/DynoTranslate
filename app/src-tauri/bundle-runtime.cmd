@echo off
setlocal
cd /d "%~dp0\.."
if not exist "src-tauri\resources" mkdir "src-tauri\resources"
if not exist "src-tauri\target\release\onnxruntime.dll" (
  echo onnxruntime.dll was not produced by the build
  exit /b 1
)
copy /Y "src-tauri\target\release\onnxruntime.dll" "src-tauri\resources\onnxruntime.dll"
if errorlevel 1 exit /b 1
if exist "src-tauri\target\release\DirectML.dll" (
  copy /Y "src-tauri\target\release\DirectML.dll" "src-tauri\resources\DirectML.dll"
  if errorlevel 1 exit /b 1
)
dir /b "src-tauri\resources\*.dll"
exit /b 0
