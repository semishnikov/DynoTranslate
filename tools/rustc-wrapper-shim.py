#!/usr/bin/env python3
"""Cargo rustc-wrapper shim: relay to the real compiler, surface failures as annotations.

Invoked as: rustc-wrapper <program> <args...>, where <program> is rustc or clippy-driver
(a bare name or a full path). On failure, diagnostics are written to $SHIM_REPORT (echoed by
the wrapper, which reaches the runner's annotation parser) and to the job summary.
TEMPORARY.
"""
import json
import os
import subprocess
import sys


def escape(text):
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def report(line):
    print("::error::" + escape(line), flush=True)
    path = os.environ.get("SHIM_REPORT")
    if path:
        try:
            with open(path, "a", encoding="utf-8") as handle:
                handle.write("::error::" + escape(line) + "\n")
        except OSError:
            pass
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        try:
            with open(summary, "a", encoding="utf-8") as handle:
                handle.write(line[:4000] + "\n")
        except OSError:
            pass


def main():
    program = sys.argv[1]
    args = sys.argv[2:]
    proc = subprocess.run([program] + args, capture_output=True, close_fds=False)
    sys.stdout.buffer.write(proc.stdout)
    sys.stdout.flush()
    sys.stderr.buffer.write(proc.stderr)
    sys.stderr.flush()
    if proc.returncode != 0:
        report("SHIM-RAN %s exit=%s" % (program, proc.returncode))
        seen = 0
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
            report(where + message)
            seen += 1
            if seen >= 20:
                break
    sys.exit(proc.returncode)


try:
    main()
except SystemExit:
    raise
except Exception as exc:  # never fail silently
    report("SHIM-CRASH %s" % repr(exc))
    sys.exit(97)
