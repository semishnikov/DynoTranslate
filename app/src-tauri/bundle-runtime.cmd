@echo off
setlocal
cd /d "%~dp0\.."
if not exist "src-tauri\resources" mkdir "src-tauri\resources"
rem The translation runtime is linked into the program. Windows still needs
rem DirectML.dll beside the installed program before it can start.
rem Always overwrite: the build script may have left an empty placeholder
rem there so clippy could run; the installer must ship the real library.
if exist "%SystemRoot%\System32\DirectML.dll" (
  copy /Y "%SystemRoot%\System32\DirectML.dll" "src-tauri\resources\DirectML.dll" >nul
)
if not exist "src-tauri\resources\DirectML.dll" (
  echo the graphics library was not found
  exit /b 1
)
exit /b 0
