# Lumen

Lumen translates the text on your screen where it stands. Start a game or an application written in another language
and its menus, tooltips and dialogue appear in your own language, in the same place and the same style, as if the
software had shipped that way.

## How it works

Lumen reads the picture the operating system already composes for the window you are using, recognises the text in it,
translates it locally and draws the result over the original through a transparent overlay that never takes focus and
never receives input.

It does not inject code, hook functions, read process memory or install drivers. That is a deliberate constraint: it
keeps Lumen safe to run next to anti-cheat software and impossible to confuse with a cheat.

## Privacy

Translation runs on your computer using language packs stored on disk. Screenshots and recognised text are never
written to disk and never sent anywhere. Capture pauses automatically for password fields, the sign-in screen and UAC
prompts. Online translation engines are off by default and can only be enabled per application, with a clear statement
of what would be sent.

## Status

This repository is under active development. The interface and the design system are built; the recognition,
translation and overlay pipeline is being implemented milestone by milestone. See `docs/PLAN.md` for the milestone
list and `docs/STATUS.md` for what currently works.

## Build from source

The interface requires Node.js 20 or newer.

```
cd app
npm install
npm run dev
```

`npm run build` type-checks with the TypeScript compiler and produces a production bundle in `app/dist`.

## Documentation

- `docs/PLAN.md` — milestones and working protocol
- `docs/ARCHITECTURE.md` — pipeline layers and where platform code is allowed
- `docs/adr/` — architecture decision records
- `docs/STATUS.md` — what is done, what is next, what is blocked
