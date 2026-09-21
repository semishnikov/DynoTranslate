# 1. Software compositor for the overlay bitmap

Status: accepted

## Context

The overlay must render translated text over arbitrary application pixels at interactive rates, and its output must be
verifiable in continuous integration. Direct2D and DirectWrite are the obvious native choice, but they only run on
Windows, produce device-dependent output, and cannot be exercised on the Linux build agents that run the bulk of the
test suite.

## Decision

Compose the overlay bitmap with a portable software renderer (tiny-skia, cosmic-text, rustybuzz, fontdb) and present it
through a layered DirectComposition window, updating dirty rectangles only. Direct2D will be revisited only if
measurements show the software path cannot hold the latency budget.

## Consequences

Golden-image tests run on every pull request on Linux and catch fitting, inpainting and font-matching regressions.
Presentation stays a thin platform adapter. The cost is CPU time for compositing, bounded by the dirty-rectangle
update strategy and the tile-based change detection upstream.
