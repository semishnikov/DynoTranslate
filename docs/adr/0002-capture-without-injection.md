# 2. Capture without injection or hooking

Status: accepted

## Context

In-place translation needs the text a game or application is displaying. Prior art splits into two families: hooking
the process (text hooks, memory reading, graphics API interception) and reading what the operating system already
composes. Hooking gives cleaner text but is indistinguishable from cheating to anti-cheat services and risks bans,
crashes and false positives.

## Decision

Only OS-sanctioned sources are used: Windows Graphics Capture with DXGI Desktop Duplication as fallback, UI Automation
for native text, and a transparent click-through overlay for output. No injection, no hooks, no memory reads, no
drivers.

## Consequences

The product is safe to run alongside anti-cheat software, and that guarantee can be stated plainly to users. Text
quality depends on OCR rather than on exact in-memory strings, which is why recognition quality and temporal stability
carry their own measured budgets. Exclusive-fullscreen applications cannot be captured and are handled by detecting the
mode and guiding the user to borderless.
