# Delivery

Written 2026-09-22, after the owner asked for a working application rather than another
milestone of scaffolding. This file outranks the unfinished paperwork in `docs/STATUS.md`
until a person can start an English window and see Russian text drawn over it.

The owner does not configure engines, store secrets, create tokens, or rehearse tags before
that. Those steps are release bureaucracy. They are not the product.

## The promise, said plainly

DynoTranslate is a Windows application. The owner starts it, then starts a game or any other
window whose text is English. From that moment the text on that window is drawn in Russian,
where the original stood, and it updates when the screen changes. It is not a translation of
one saved picture. The owner does not select sentences and does not paste them anywhere.

## What already exists, and what it is not

Milestones M0–M5 are on `main`. They are a shell, a test pipeline, and stand-ins:

- Recognition in tests does not read pixels. It replays text that the test planted.
- Translation in tests is a scripted stand-in, not a model.
- The window does not drive the pipeline. An installed build is a settings shell.
- The Windows installer in pull request #13 is a package of that shell. It was built by
  GitHub Actions. It was not installed, and it does not translate a screen.

None of that is a result the owner can see. More tests around the stand-ins are not progress.

## The ceiling, so the plan does not lie

This kind of application exists. Translumo, Luna Translator, and Game Overlay Translator
(Steam, 2026) do the same job: capture the window, read the pixels, translate, draw on top.
Their published limits are the limits of the method, not of this repository:

- Delay is a fraction of a second to about a second, not zero. A commercial overlay measured
  a median around half a second on a GPU and up to about a second on a CPU. Scanning the
  whole screen many times a second costs the game frames. The honest target already in
  `docs/PLAN.md` is 250 ms typical and 500 ms worst for a change, on a normal six-core PC.
  That is the number to measure, not "no delay".
- Text that is pixels can be misread. Clean interface text is readable. Outlined, glowing,
  pixel, and animated text is often wrong. A product in this category says so on its store
  page. A 3 % error rate on stylized game text is a hope, not a fact, until it is measured.
- Exclusive fullscreen often cannot be captured or covered. Borderless windowed mode is
  required for many games. The application must say that in one sentence and keep working
  for windows that can be captured.
- Competitive anti-cheat can still punish an overlay even when the application does not
  inject, hook, or read memory. The safety rule in the plan stays: no injection, no hooks,
  no drivers. It does not mean every anti-cheat will allow the overlay. Online competitive
  games are at the owner's risk. Story games and ordinary windows are the target.
- A compact offline model will not match an official localization. Names, jokes, and invented
  terms come out awkward. A glossary and a memory make repeated interface words stable. They
  do not make a novel into a published translation.
- Moving world text (nameplates, damage numbers) is not tracked as a moving object. Text is
  translated where it is when it is read.

So the application can exist. The sentence "any English game becomes a perfect Russian
release, instantly, with no errors, on every PC, beside every anti-cheat" cannot be promised.
The sentence "an English window or a cooperative game with readable text becomes playable
Russian, updating on its own, with visible mistakes on fancy fonts" can.

## Work order

Do not open M7. Do not add another stand-in. Do not ask the owner for a signing secret, a
token, a license choice, or a product-name decision before the slice below runs.

### 1. One real loop, one installer

A Windows build the owner can double-click. On launch it translates the foreground window
into Russian with no setup screen in the way.

- Capture with Windows Graphics Capture. The interim copy path may stay as a fallback. If
  capture fails, the overlay disappears and the game is left alone.
- Read pixels with Windows.Media.Ocr first. It is on Windows 10 and 11, it is a real engine,
  and it does not add a model download. English language data is installed silently if the
  PC does not have it. RapidOCR is a later engine for fonts Windows OCR misses, not a
  prerequisite for the first build the owner can see.
- Translate English to Russian with a real offline model, not `StubTranslationEngine`.
  Prefer a compact English–Russian model (OPUS-MT class, int8) run locally. The installer
  may download that pack once, silently, on first launch, then work offline. The owner is
  not asked to pick a file or an engine. The stand-in may remain in tests. It must not be
  the engine in the shipped path.
- Draw with the renderer that already exists: erase or cover the original, fit the Russian
  line, click-through, never focused, never captured by its own capture.
- Reuse the change detector. A static screen is not re-read. A changed region is.
- Remember repeated lines so the same button is not translated differently each frame.
- The shell starts this loop. Settings may keep their defaults (target Russian, source
  automatic). Persistence to disk can follow. A forgotten setting is acceptable. A dead
  translate button is not.

Done means: GitHub Actions produced an installer, and the owner can be told, in one
sentence, to download it and start a window. It does not mean the agent played the game.
This environment cannot.

### 2. Numbers from a real window

After the owner runs it, or on any Windows session that can open a real window, record
what actually happened: delay, what was misread, whether the overlay sat above the window,
CPU while the screen was still. Write the numbers in `docs/STATUS.md`. If a target in the
plan was missed, write the miss. Do not replace a miss with another test of the stand-in.

### 3. Only then, the rest of the plan

In this order, each one visible:

- Better recognition for outlined and small text, behind the same engine trait.
- Erasing the original more cleanly, using the inpainter that already exists.
- More language pairs, each a pack, never a setting the owner must discover.
- Settings saved on disk. The window calling the shell for hotkeys that pause the overlay.
- The updater and a signed release. Secrets, a version tag, a license, a final name, and
  an Authenticode certificate belong here, asked one at a time, each with the reason.

## What the owner is asked to do, and when

Before the first real installer: nothing.

When that installer exists: download it and start the English window in borderless mode if
the game has that option. That is the acceptance test. It cannot be performed here.

Later, only if they want a public release: a license choice, a product name, a publisher
certificate, and two repository secrets for automatic updates. Not before.

## Chances

- That a double-click application can put Russian over readable English text and update it
  when the text changes: high, if work follows section 1 and stops polishing stand-ins.
  Similar applications already ship. The remaining risk is integration time, not an unsolved
  principle. Expect several sessions, not an afternoon, because the model and the Windows
  capture path have never been compiled into this application.
- That the first build feels like an official localization of an arbitrary game: low. Readable
  menus and dialogue in a story game are the realistic first success. Pixel fonts, heavy
  effects, and exclusive fullscreen will fail in the open.
- That every number in `docs/PLAN.md` is hit together, for every window, in twenty languages,
  with no anti-cheat friction: low. Some of those numbers contradict each other once a real
  model and a real recognizer are running. Measure them. Do not treat them as already true.
