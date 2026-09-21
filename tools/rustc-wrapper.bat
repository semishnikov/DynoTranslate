#!/bin/sh
export SHIM_REPORT="${TMPDIR:-/tmp}/shim-report-$$"
exec python3 "$(dirname "$0")/rustc-wrapper-shim.py" "$@" || exec python "$(dirname "$0")/rustc-wrapper-shim.py" "$@"
@if "%OS%"=="Windows_NT" set SHIM_REPORT=%TEMP%\shim-report-%RANDOM%.txt
@if "%OS%"=="Windows_NT" python "%~dp0rustc-wrapper-shim.py" %*
@if "%OS%"=="Windows_NT" set SHIM_RC=%errorlevel%
@if "%OS%"=="Windows_NT" if exist "%SHIM_REPORT%" type "%SHIM_REPORT%"
@if "%OS%"=="Windows_NT" exit /b %SHIM_RC%
