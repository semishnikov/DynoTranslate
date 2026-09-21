#!/usr/bin/env python3
"""Cargo rustc-wrapper shim: relay to the real compiler, surface failures as annotations.

Invoked as: rustc-wrapper <program> <args...>, where <program> is rustc or clippy-driver
(a bare name or a full path). Diagnostics are re-emitted as GitHub Actions annotations so
they are readable through the API even when job logs are unreachable. TEMPORARY.
"""
import json
import os
import subprocess
import sys


def escape(text):
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


program = sys.argv[1]
args = sys.argv[2:]
proc = subprocess.run([program] + args, capture_output=True, close_fds=False)
sys.stdout.buffer.write(proc.stdout)
sys.stdout.flush()
sys.stderr.buffer.write(proc.stderr)
sys.stderr.flush()
if proc.returncode != 0:
    for line in proc.stderr.decode("utf-8", "replace").splitlines():
        try:
            diag = json.loads(line)
        except ValueError:
            continue
        if diag.get("$message_type") != "diagnostic" or diag.get("level") != "error":
            continue
        message = diag.get("message", "")
        span = next((s for s in diag.get("spans", []) if s.get("is_primary")), None)
        where = ""
        if span:
            where = "%s:%s:%s: " % (span["file_name"], span["line_start"], span["column_start"])
        print("::error::" + escape(where + message), flush=True)
sys.exit(proc.returncode)
