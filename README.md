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

## Releasing

Push a version tag to build and publish a Windows installer:

```bash
git tag v0.1.0
git push --tags
```

`.github/workflows/release.yml` builds on `windows-latest`, runs
`desktop/`'s test suite, then `npm run build:desktop`, and attaches the
resulting MSI + NSIS installers to a **draft** GitHub Release (review and
publish it by hand — nothing goes live automatically). Also runnable
manually via `workflow_dispatch` from the Actions tab, without a tag.

Windows only for now, no code signing — the installer will show an
"unknown publisher" warning until a certificate is added. macOS/Linux
targets can be added as additional platform jobs later if needed.
