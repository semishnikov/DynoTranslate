#!/bin/sh
echo '::error::WRAPPER-BOOT-SH'
echo 'WRAPPER-BOOT-SH' >> "$GITHUB_STEP_SUMMARY" 2>/dev/null
exec python3 "$(dirname "$0")/rustc-wrapper-shim.py" "$@" || exec python "$(dirname "$0")/rustc-wrapper-shim.py" "$@" || echo '::error::WRAPPER-NO-PYTHON'
@if "%OS%"=="Windows_NT" echo ::error::WRAPPER-BOOT-BAT
@if "%OS%"=="Windows_NT" python "%~dp0rustc-wrapper-shim.py" %*
@if errorlevel 9009 echo ::error::WRAPPER-NO-PYTHON
@exit /b %errorlevel%
