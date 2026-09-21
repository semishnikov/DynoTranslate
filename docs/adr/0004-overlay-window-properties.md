# 4. Overlay window properties are asserted, not assumed

Status: accepted

## Context

The overlay carries most of the product's safety promises. If it takes focus it interrupts a game; if it receives input
it breaks the application underneath; if it appears in Alt-Tab it looks like a defect; and if the pipeline's own
capture can see it, translated text is fed back into recognition and degrades into noise on the next frame.

Each of these is a single flag on the window, easy to lose in a refactor and invisible until a user reports it.

## Decision

The properties are modelled as `SurfaceProperties`, which every surface reports. `SurfaceProperties::REQUIRED` states
the contract; `missing()` names whichever properties are absent so a failure produces a specific log line rather than a
generic warning. The Windows surface queries the live window with `GetWindowDisplayAffinity` instead of returning the
value it intended to set.

`WDA_EXCLUDEFROMCAPTURE` requires Windows 10 version 2004. On older builds the exclusion is reported as absent, and the
caller subtracts the overlay's rectangles from captured frames instead of trusting the flag.

## Consequences

A surface that silently loses a property fails an assertion in the test suite rather than reaching a user. The contract
is duplicated in one place only, and the fallback for older builds is driven by an observed value rather than by a
version check.
