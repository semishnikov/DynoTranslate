#!/bin/sh
exec python3 "$(dirname "$0")/rustc-wrapper-shim.py" "$@" || exec python "$(dirname "$0")/rustc-wrapper-shim.py" "$@" || echo '::error::WRAPPER-NO-PYTHON'
@if "%OS%"=="Windows_NT" python "%~dp0rustc-wrapper-shim.py" %*
@if errorlevel 9009 echo ::error::WRAPPER-NO-PYTHON
@exit /b %errorlevel%
