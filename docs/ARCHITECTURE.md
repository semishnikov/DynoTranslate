# Architecture

## Layers

```
 capture ──► change detection ──► text sources ──► layout ──► translation ──► compositor ──► overlay
   (OS)          (portable)       (OS + models)   (portable)   (portable)      (portable)      (OS)
```

Platform-specific code lives only at the two ends of the pipeline. Everything between them is deterministic and
testable without a display, which is what makes golden-image tests and CI benchmarks possible.

### capture (platform)
Windows Graphics Capture per window or monitor, DXGI Desktop Duplication as fallback. GPU-side crop and downscale,
HDR to SDR tone mapping before recognition, per-monitor DPI v2. The overlay excludes itself from capture with
`SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`; where that is unavailable the compositor subtracts the overlay's
own rectangles from the frame.

### change detection (portable)
The frame is split into tiles; only tiles whose hash changed are handed to recognition. Idle scenes cost no
recognition calls. Sampling rate adapts between 2 Hz and the configured cap based on how fast text is changing, and
backs off when the foreground application starts dropping frames.

### text sources (platform + models)
UI Automation provides exact text and bounds for native apps and browsers. OCR covers everything else behind a single
trait with several implementations: `Windows.Media.Ocr` for speed, RapidOCR/PaddleOCR on ONNX Runtime with DirectML for
stylized game fonts, Tesseract as the last fallback. Results from both sources are merged and de-duplicated.

### layout (portable)
Lines are grouped into blocks and blocks classified as button, tooltip, dialogue, menu or subtitle from geometry,
alignment, colour and font metrics. Foreground and background colours are sampled per block; font size, weight and
effects are estimated. Reading order is preserved.

### translation (portable)
Normalize, protect tokens (numbers, key hints such as `[E]`, placeholders `{0}` and `%s`, tags, URLs, do-not-translate
names), look up the translation memory (SQLite; key = normalized source + language pair + glossary version), then call
an engine. The offline engine runs OPUS-MT class int8 models through CTranslate2. Online engines are opt-in per app,
with timeouts, retries, a circuit breaker and automatic fallback to offline. Dialogue blocks are translated together so
sentence context survives.

### compositor (portable)
A software renderer (tiny-skia with cosmic-text and rustybuzz) produces the overlay bitmap. It is deterministic, so the
same input frame always yields the same image and golden-image tests are meaningful on Linux. Per block: erase the
original by local inpainting, then draw the translation with the closest matching font, matching weight, colour,
outline, alignment and line spacing. Fitting never falls below 80% of the original size and never ellipsizes.

### overlay (platform)
A layered DirectComposition window, topmost, `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, hidden from
Alt-Tab, updating dirty rectangles only. Any pipeline error hides the overlay and leaves the host application
untouched.

## Shell

Tauri 2 hosts the React and TypeScript interface. The interface talks to the pipeline through a narrow command surface
so it can be developed and screenshot-tested against a mocked backend in a browser. State lives in a single store; all
settings apply immediately and are written atomically with a versioned schema.
