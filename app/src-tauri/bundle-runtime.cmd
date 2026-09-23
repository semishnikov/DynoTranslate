@echo off
setlocal
cd /d "%~dp0\.."
if not exist "src-tauri\resources" mkdir "src-tauri\resources"
set "RUNTIME="
for /f "delims=" %%F in ('dir /s /b "src-tauri\target\onnxruntime*.dll" 2^>nul') do (
  if not defined RUNTIME set "RUNTIME=%%F"
)
if not defined RUNTIME (
  echo the translation runtime dll was not produced
  exit /b 1
)
copy /Y "%RUNTIME%" "src-tauri\resources\onnxruntime.dll"
if errorlevel 1 exit /b 1
for /f "delims=" %%F in ('dir /s /b "src-tauri\target\DirectML.dll" 2^>nul') do (
  copy /Y "%%F" "src-tauri\resources\DirectML.dll"
  goto :copied
)
:copied
dir /b "src-tauri\resources\*.dll"
exit /b 0
