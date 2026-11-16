# AGENTS.md — Gaia Desktop

Native Windows client (Tauri 2 + React 19). Desktop is presence, interface and
local capability; **all** cognition, memory, identity and reasoning live in
Gaia Cloud (`Gaia-Cloud` repo, `services/gaia-api`). Keep that seam absolute.

## Layout

- `desktop/` — the UI (Vite + React, own package.json). Entry `src/main.jsx`.
- `src-tauri/` — the Rust shell: `ServerLink`, audio, TTS, capture, MCP, settings.
- Root `package.json` holds **only** `@tauri-apps/cli`. `npm run build` and
  `npm test` do not exist there; run them inside `desktop/`.
- `desktop/dist/` is build output embedded by Tauri (`frontendDist`) — never edit.
- `spikes/` and `.openchamber/` are local-only (voice recordings, screenshots) and gitignored.

## Commands

```bash
npm run dev:desktop                 # tauri dev (spawns desktop/ Vite on :5173, strict port)
npm run build:desktop               # tauri build; bundles MSI + NSIS + updater artifacts
cd desktop && npm test              # vitest run, whole UI suite
cd desktop && npx vitest run src/lib/sentences.test.js   # one file
cd desktop && npm run build         # vite build (the only frontend check)
cd src-tauri && cargo test --lib    # whole Rust suite; also the compile check CI uses
cd src-tauri && cargo test --lib audio::echo             # one module
```

Windows helper scripts (kill a running `gaia.exe` first — the linker cannot
overwrite the locked binary):

- `build.cmd` — release exe only, fast (`--no-bundle`), no start.
- `build.cmd installer` — full MSI/NSIS/updater bundle.
- `build-and-start.cmd` — exe build then start; `start.cmd` — start last build.

There is **no** eslint, prettier, tsconfig or rustfmt/clippy config. Don't
invent a lint step; the checks above are it.

### Node ≥ 26 breaks the UI tests

`npm test` fails 58 jsdom tests (`localStorage` is undefined) because Node 26
defines a global `localStorage` that shadows jsdom's. CI uses Node 22. Locally,
either use Node 22 or:

```bash
cd desktop && NODE_OPTIONS="--localstorage-file=$TEMP/gaia-ls" npm test
```

## Architecture rules

- The webview has no network: CSP is `connect-src 'self' ipc:`. Every HTTP call
  goes through Rust `ServerLink` and surfaces as a Tauri command.
- Adding a server call: request/parse builders in `desktop/src/state/contract.js`,
  wrapper in `desktop/src/server/api.js` (the single bridge), Rust command in the
  owning module, registered in `src-tauri/src/lib.rs`'s `generate_handler!`.
  `contract.test.js` pins both wire shapes; UI components never see raw JSON.
- Byte payloads (uploads, downloads, exports, audio) never cross `server_request`
  — they get dedicated commands that stream to/from a native-dialog path.
- Turns stream via `server_stream_turn` (`requestId` matched against
  `server://turn-delta` events); cancel with `server_cancel_turn`.
- No client-side SOUL, memory or interpretation. Rust modules are capabilities,
  not reasoners.
- Mic master switch is runtime-only: every launch starts with the mic off.
- Gaia's voice plays in Rust (`speech_play`), never in the webview — playback is
  the echo-canceller reference for barge-in.
- New plugins need permissions in `src-tauri/capabilities/default.json`.
- `tauri-plugin-single-instance` is registered first on purpose: one Gaia per machine.

## Conventions

- UI state: one hook per concern in `desktop/src/state/use*.js`, colocated
  `*.test.js` (vitest + jsdom, configured in `desktop/vite.config.js`).
- User-facing text comes from `L` in `desktop/src/lib/lexicon.js`, with entries
  in **both** `LANGUAGES.nl` and `.en`. Default language is `nl`
  (`localStorage['gaia.lang']`).
- Errors shown to the person are calm sentences without HTTP codes, provider
  names or stack traces (`state/phrases.js`).
- Rust tests live inline in each module (`#[cfg(test)] mod tests`); settings use
  `#[serde(default)]` so old `settings.json` files keep working.
- Settings: `%APPDATA%\ai.gaia.desktop\settings.json`; the server auth token is
  in the Windows credential store (service `ai.gaia.desktop`). `.env.local`'s
  `GAIA_BRIDGE_TOKEN` only seeds an empty token in memory for local dev — never
  write it to disk.
- Debugging TTS: `GAIA_SPEECH_DUMP=<dir>`, then
  `python scripts/speech-dump-report.py <dir>` (per-half-second RMS of
  `-source.wav` vs `-played.wav`). `audio-record.bat` is the local launcher for it.
- Folder search needs Everything's `es.exe`; without it `everything_available` is false.

## CI and release

- `ci.yml`: windows-latest, Node 22, `npm test` in `desktop/` + `cargo test --lib`.
- `release.yml`: runs on `v*` tags (or manual dispatch), builds the installer,
  writes `latest.json` for the updater and attaches everything to a **draft**
  GitHub Release. Nothing publishes itself.
- Before tagging, bump the version in `package.json`, `desktop/package.json` and
  `src-tauri/tauri.conf.json` — the installer version comes from `tauri.conf.json`,
  while `latest.json` takes its version from the tag. `Cargo.toml`'s version is
  unused (still 0.1.0).
- Windows only, no code signing; updater artifacts need a minisign
  `SIGNING_PRIVATE_KEY` secret.

## Walkthrough

Every significant change appends to the end of `walkthrough.md` (Dutch, newest
last), in this shape — with the *reason* for the choice, not just the edit:

```markdown
## YYYY-MM-DD (Korte titel)

- Findings: [wat was het probleem]
- Conclusions: [waarom deze aanpak]
- Actions: [gewijzigde bestanden en wat precies]; validated.
```

Past 1000 lines, start `walkthrough2.md`. `docs/background-voice-spec.md` §3
("Current state") can lag behind the code; code and walkthrough win.

## Werkafspraken

- Antwoord in het Nederlands als de vraag Nederlands is; code, comments en
  README zijn Engels, walkthrough-entries Nederlands.
- Wees incrementeel: één duidelijke taak per keer, valideer voor je afsluit.
- Flag inconsistenties in plaats van ze stilzwijgend te omzeilen.
- Geen nieuwe dependency zonder het te benoemen en te vragen.
- Bo's geheugen-infrastructuur (CortexIQ/Hindsight) niet voorstellen te vervangen.
- Grote wijzigingen: vraag of er gecommit en naar `main` gepusht moet worden.
