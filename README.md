# Gaia Desktop

A lifelong personal intelligence — the native desktop client. Extracted
from the `Gaia-Cloud` monorepo (Phase 1 of `docs/split-plan.md`,
2026-08-19; Milestone 8 already gave Desktop its own frontend, decoupled
from Gaia Web — see `evolution.md` in Gaia-Cloud).

## Structure

- `desktop/` — the desktop's own UI (Vite + React): sidebar, conversation
  view, composer, presence bar, settings panel.
- `src-tauri/` — the Rust/Tauri shell: communication (`ServerLink`),
  capture, audio, notifications, settings, presence.

Desktop is a first-class client of Gaia Cloud, never a wrapper around Gaia
Web. It performs no reasoning, loads no SOUL, and calls no Hermes/Hindsight/
cognition service directly — identity, memory, intent and reasoning are
server-side by contract, reached through the Rust `ServerLink`
(`server_request` → `services/gaia-api`'s `POST conversation/turn`, in the
`Gaia-Cloud` repo). It has no dependency on `docs/` or Cloud's identity
files — it never builds a client-side SOUL.

## Scripts

```bash
npm run dev:desktop      # tauri dev
npm run build:desktop    # tauri build
```

Inside `desktop/`: `npm run dev` (Vite dev server), `npm test` (vitest).
