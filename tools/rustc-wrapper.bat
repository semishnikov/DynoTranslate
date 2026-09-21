#!/bin/sh
exec python3 "$(dirname "$0")/rustc-wrapper-shim.py" "$@"
@goto :win
:win
@python3 "%~dp0rustc-wrapper-shim.py" %*
@exit /b %errorlevel%
