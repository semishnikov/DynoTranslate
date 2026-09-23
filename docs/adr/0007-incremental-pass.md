# 7. The incremental pass: reuse the previous reading on unchanged tiles

Status: accepted

## Context

M4 measured the pipeline whole-frame: every pass re-read the entire window, re-merged,
re-analysed and re-composed, even when nothing moved. The plan's quality targets do not
allow that: a static screen must cost almost no CPU, a changing screen must translate in
well under half a second, and visible flicker over ten minutes must be zero. Re-presenting
pixels the overlay already drew is exactly how flicker gets born on a layered window, and
the cost of a full pass on a static screen is spent on a screen that changed nothing.

The change detector already knows which tiles changed. The missing piece was making the
rest of the pipeline trust it.

## Decision

1. **A static frame runs nothing beyond change detection.** When the detector reports no
   changed tiles and a previous pass exists, the pipeline reuses the previous pass state —
   merged runs, layout, blocks — skips the read, the merge, the layout, the stability
   pass, the translation and the compose, and presents nothing. The overlay stands, the
   host application is untouched, and the cost of the frame is the tile hash itself.
2. **A changed frame re-reads only the changed regions.** The previous pass's merged runs
   that do not intersect a changed region are kept; a read is restricted to the changed
   regions (with a configurable margin, default zero, because change detection is
   tile-accurate: an unchanged tile cannot hold new text); the kept and the fresh runs are
   merged as usual. A real recognition engine that wants context pixels around a region
   sets the margin where it wraps its adapter, so the pass itself stays exact.
3. **The compose is skipped when the overlay content is unchanged.** A changed frame whose
   layout is identical to the previous one and whose changed regions do not touch what the
   compositor last painted reuses the previous composition and presents nothing. When a
   changed region does touch painted pixels the compose runs again, because seamless
   erasure samples the source frame and must follow it.
4. **Fail-open invalidates the pass state.** When the target window is lost the overlay
   clears itself (an empty layout over the last painted regions, presented in full), the
   pass state is dropped, and the next pass re-reads the whole frame. Clearing without the
   full re-read would leave text on screen that no pass state carries, which is the same
   violation as a stale overlay.
5. **Text sources step by frame, not by call.** A source whose answer depends on which
   frame is being shown implements `TextSource::advance`, which the pipeline calls once
   per captured frame. Skipping reads on static frames would otherwise desync a scripted
   scene from its pixels; sources that answer from the pixels alone keep the no-op
   default.

## Consequences

- The static-frame cost drops to the tile hash. The report now separates `detect`,
  `read`, `stage` (layout, stability, translation) and `compose` per frame, and the
  budget test asserts a skipped frame has zero read, stage, compose and presented cost,
  plus that the static p95 is a small fraction of the changed p95.
- A re-read region that only partially overlaps a label is re-read whole: the source is
  asked for the region, the region carries the label, and the merge de-duplicates it
   against the kept run. A label that does not touch a changed region is never re-read
  while its pixels stand, which is also what stops the translation engine from being
  called for text that has not changed.
- The scene text source must stay in step with the capture by `advance`, which is why the
  trait gained the method rather than the harness counting frames per source.
- The `--no-reuse` flag keeps the M4 whole-frame behaviour available, and a test runs the
  menu scene both ways and asserts the overlay blocks, the language and the damage are
  identical frame for frame. That is the guard that reuse is an optimisation, not a
  different pipeline.
- The Windows capture adapter will reuse the same pass; when it lands, its own context
  margin is the one place the margin setting moves.
