# 3. Capture path and the adapter boundary

Status: accepted

## Context

Capture is the only stage that cannot run on the machines where most of the project is tested, and it is also the stage
that decides whether the product is safe next to anti-cheat software. Windows Graphics Capture is the intended
production path, but its session plumbing needs WinRT interop, a D3D device and a working compositor, none of which
exist on a build agent or in the development environment.

## Decision

Capture is expressed as one trait, `CaptureSource`, returning frames in straight BGRA8. Three implementations exist or
are planned behind it:

- `SyntheticSource` renders a scripted scene deterministically. It is the source used by tests, golden images and
  benchmarks, so scheduling and change detection are measured identically everywhere.
- `DesktopCopySource` copies a window's pixels through the desktop device context. It works today on any Windows build
  and keeps the rest of the pipeline runnable against real windows.
- A Windows Graphics Capture session replaces `DesktopCopySource` for production use once its interop lands. It is
  strictly faster and avoids the redraw the GDI path can cause on some compositors.

Frame geometry comes from `DWMWA_EXTENDED_FRAME_BOUNDS` rather than `GetWindowRect`, because the window rectangle
includes an invisible resize border that would offset every recognised bounding box.

## Consequences

The pipeline never depends on which source is active, so the production capture path can be swapped in without
touching recognition, composition or presentation. The cost is that the GDI path ships in the meantime and must be
removed rather than merely superseded, so it is marked in the module documentation as interim.
