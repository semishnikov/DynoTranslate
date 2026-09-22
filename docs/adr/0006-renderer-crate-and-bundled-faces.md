# 6. Portable renderer crate, bundled faces, and how erasure meets drawing

Status: accepted

## Context

M4 has to erase the original text, match its font, fit the translation without truncation, set
right-to-left and vertical text, and keep all of that under a visual regression suite. ADR 0001
already chose a portable software compositor (tiny-skia, cosmic-text, rustybuzz, fontdb) so the
suite can run on Linux build agents; ADR 0005 already gave each portable stage its own crate.
Neither decision said where the rendering code itself lives, which faces it may use, or what
happened to rustybuzz.

## Decision

1. **`lumen-render` is the portable rendering stage**, following ADR 0005: fitting, font
   matching, glyph drawing and local inpainting live there; `lumen-overlay` depends on it and
   keeps only damage tracking and style policy.
2. **Six DejaVu faces are bundled and loaded from memory.** Golden images compared against
   whatever fonts the build agent installed would not be golden. The same bytes ship in the
   binary on every platform, and the generic CSS families are retargeted at the bundled names
   so matching never reaches outside the bundle.
3. **Shaping goes through cosmic-text 0.19, which shapes with harfrust**, not rustybuzz.
   cosmic-text moved from rustybuzz to harfrust before M4 started; the ADR's intent — one
   portable shaper with bidi — is unchanged, the implementation detail of the shaper inside
   cosmic-text is not ours to pin. tiny-skia stays available for path work but the current
   pass rasterises glyphs through swash and blends them into the frame directly, which is one
   less conversion in the hot path.
4. **Fitting is a product rule in code**: never ellipsise, never drop below 80 % of the
   measured size, wrap before shrinking. When the floor is reached the text is drawn anyway;
   a slight overflow is readable, a truncation is not.

## Consequences

- The visual regression suite can assert byte identity on every platform.
- Adding a face means adding a file under `crates/render/fonts/` and a line in `fonts.rs`.
- System-font matching (using a family name if OCR ever reports one) is deliberately not in
  this milestone; the bundled set is the whole universe until it is.
- `lumen-overlay` grows a dependency on cosmic-text's tree; that cost is accepted because the
  compositor could not draw without it.
